// linkcpp-moe-dispatch.h — the MoE expert-dispatch router shared by the
// linkcpp-moe-verify harness and the linkcpp-server coordinator.
//
// One dispatched layer's routed-expert FFN is served by a set of workers, each
// owning experts [a,b). Same-range entries are replicas in preference order
// (list the nearest first). Per llama_decode callback the router:
//
//   1. hot-expert cache — pairs whose expert is currently "hot" (top-K by
//      dispatch count) compute on the local fallback slice, skipping the wire;
//   2. assigns every remaining (token,slot) pair to the first live replica
//      covering its expert and sends all per-worker sub-requests CONCURRENTLY
//      (per-token latency = slowest worker, not the sum);
//   3. a failed worker is marked dead and its pairs reassign to the next
//      replica on the following round;
//   4. pairs nobody covers fall back to the in-process slice — the backbone as
//      replica of last resort.
//
// Sub-requests reuse the ordinary worker wire protocol with n_used=1 and
// n_tokens=n_pairs (one hidden row + one LOCAL expert id per pair), so workers
// serve full and partial shards with the same code.
//
// linkcpp-server enables it from the environment (inert when unset):
//   LINKCPP_MOE_DISPATCH_MAP      "0-128@listen:52910,0-128@10.0.0.2:52903,..."
//   LINKCPP_MOE_DISPATCH_LAYER    dispatched layer index (default 0)
//   LINKCPP_MOE_DISPATCH_FALLBACK path of a local expert slice (last resort / cache)
//   LINKCPP_MOE_HOT_EXPERTS       K hottest experts served from the local slice (0=off)

#pragma once

#include "llama.h"
#include "ggml.h"
#include "ggml-backend.h"
#include "ggml-cpu.h"
#include "gguf.h"

#include <arpa/inet.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

namespace linkcpp_moe {

// ---- local expert slice (full or partial shard) ------------------------------
struct expert_slice {
    gguf_context *        gguf    = nullptr;
    ggml_context *        meta    = nullptr;
    ggml_backend_t        backend = nullptr;
    ggml_backend_buffer_t buffer  = nullptr;

    ggml_tensor * find(const std::string & n) const { return ggml_get_tensor(meta, n.c_str()); }

    // force_cpu: the coordinator's *fallback* slice must run on CPU — computing a
    // second GPU expert graph inside the dispatch callback while the main model
    // decodes on the same device faults the GPU allocator (observed on ROCm).
    bool load(const char * path, bool force_cpu = false) {
        if (!force_cpu) backend = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_GPU, nullptr);
        bool cpu_backend = false;
        if (!backend) { backend = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_CPU, nullptr); cpu_backend = true; }
        if (!backend) return false;
        // The coordinator's fallback slice computes *inside* the backbone decode's
        // OpenMP region (the dispatch custom op runs on an outer ggml compute
        // thread). A multi-threaded CPU compute here calls GOMP_parallel, opening
        // a second libgomp team while the outer team is mid-barrier — libgomp's
        // global pool doesn't allow that and segfaults (confirmed with -ngl>0; a
        // dedicated std::thread doesn't help since the conflict is global, not
        // nested). Pin the CPU slice to one thread so ggml-cpu takes its serial
        // path (ggml-cpu.c only enters `omp parallel` when n_threads > 1). The
        // fallback is a handful of experts, so single-threaded costs ~nothing.
        if (cpu_backend) ggml_backend_cpu_set_n_threads(backend, 1);
        gguf_init_params gp = { /*.no_alloc=*/ true, /*.ctx=*/ &meta };
        gguf = gguf_init_from_file(path, gp);
        if (!gguf) return false;
        buffer = ggml_backend_alloc_ctx_tensors(meta, backend);
        if (!buffer) return false;
        FILE * f = std::fopen(path, "rb");
        if (!f) return false;
        const size_t data_off = gguf_get_data_offset(gguf);
        const int n = gguf_get_n_tensors(gguf);
        std::vector<uint8_t> tmp;
        for (int i = 0; i < n; ++i) {
            ggml_tensor * t = ggml_get_tensor(meta, gguf_get_tensor_name(gguf, i));
            const size_t sz = ggml_nbytes(t);
            tmp.resize(sz);
            std::fseek(f, (long) (data_off + gguf_get_tensor_offset(gguf, i)), SEEK_SET);
            if (std::fread(tmp.data(), 1, sz, f) != sz) { std::fclose(f); return false; }
            ggml_backend_tensor_set(t, tmp.data(), 0, sz);
        }
        std::fclose(f);
        return true;
    }
};

// experts_out [n_embd, n_used, n_tokens] = per-(token,slot) expert FFN of `cur`.
inline bool compute_experts(const expert_slice & s, int il, int n_embd, int n_used,
                            int n_tokens, const float * cur, const int32_t * sel, float * out) {
    const std::string p = "blk." + std::to_string(il) + ".ffn_";
    ggml_tensor * up_exps   = s.find(p + "up_exps.weight");
    ggml_tensor * gate_exps = s.find(p + "gate_exps.weight");
    ggml_tensor * down_exps = s.find(p + "down_exps.weight");
    if (!up_exps || !gate_exps || !down_exps) return false;
    // Sanitize expert ids before mul_mat_id: a bad id indexes past the expert
    // weight tensor and faults the matmul. Under a GPU backbone (-ngl>0) the
    // reduce op hands us padding/uninitialised slots with garbage ids (seen as
    // 0xBEBEBEBE at warmup). Clamp any out-of-range id to 0 so the matmul is
    // safe, remember which slots were bad, and zero their output afterwards —
    // those slots carry no real routed expert, so a zero contribution is correct
    // and the decode proceeds instead of failing (which would abort warmup).
    const int    n_pairs_ce      = n_used * n_tokens;
    const int    n_expert_slice  = (int) up_exps->ne[2];
    std::vector<int32_t> sel_safe(sel, sel + (size_t) n_pairs_ce);
    std::vector<char>    bad_slot((size_t) n_pairs_ce, 0);
    int n_bad = 0;
    for (int i = 0; i < n_pairs_ce; ++i)
        if (sel_safe[i] < 0 || sel_safe[i] >= n_expert_slice) { sel_safe[i] = 0; bad_slot[i] = 1; ++n_bad; }

    ggml_init_params cp = { ggml_tensor_overhead() * 32 + ggml_graph_overhead(), nullptr, true };
    ggml_context * ctx = ggml_init(cp);
    ggml_tensor * h  = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, n_embd, 1, n_tokens);
    ggml_tensor * id = ggml_new_tensor_2d(ctx, GGML_TYPE_I32, n_used, n_tokens);
    ggml_set_input(h); ggml_set_input(id);
    ggml_tensor * up   = ggml_mul_mat_id(ctx, up_exps, h, id);
    ggml_tensor * gate = ggml_mul_mat_id(ctx, gate_exps, h, id);
    ggml_tensor * act  = ggml_swiglu_split(ctx, gate, up);
    ggml_tensor * o    = ggml_mul_mat_id(ctx, down_exps, act, id);
    ggml_set_output(o);
    ggml_cgraph * gf = ggml_new_graph(ctx);
    ggml_build_forward_expand(gf, o);
    ggml_gallocr_t alloc = ggml_gallocr_new(ggml_backend_get_default_buffer_type(s.backend));
    bool ok = ggml_gallocr_alloc_graph(alloc, gf);
    if (ok) {
        ggml_backend_tensor_set(h,  cur, 0, (size_t) n_embd * n_tokens * sizeof(float));
        ggml_backend_tensor_set(id, sel_safe.data(), 0, (size_t) n_pairs_ce * sizeof(int32_t));
        // The fallback CPU backend is pinned to one thread (see expert_slice::load)
        // so this compute takes ggml-cpu's serial path and never opens a libgomp
        // team that would collide with the backbone decode's active OpenMP region.
        ok = ggml_backend_graph_compute(s.backend, gf) == GGML_STATUS_SUCCESS;
    }
    if (ok) {
        ggml_backend_tensor_get(o, out, 0, (size_t) n_embd * n_pairs_ce * sizeof(float));
        // Slots whose id was garbage carry no real expert — zero them so the
        // clamped-to-0 matmul result never leaks into the reduced output.
        for (int i = 0; i < n_pairs_ce; ++i)
            if (bad_slot[i]) std::memset(out + (size_t) i * n_embd, 0, (size_t) n_embd * sizeof(float));
    }
    ggml_gallocr_free(alloc);
    ggml_free(ctx);
    return ok;
}

// ---- wire helpers -------------------------------------------------------------
inline bool io_send(int fd, const void * p, size_t n) {
    const char * c = (const char *) p;
    while (n) { ssize_t k = ::send(fd, c, n, 0); if (k <= 0) return false; c += k; n -= (size_t) k; }
    return true;
}
inline bool io_recv(int fd, void * p, size_t n) {
    char * c = (char *) p;
    while (n) { ssize_t k = ::recv(fd, c, n, 0); if (k <= 0) return false; c += k; n -= (size_t) k; }
    return true;
}
inline int connect_host(const char * host, int port) {
    int fd = ::socket(AF_INET, SOCK_STREAM, 0);
    sockaddr_in a {}; a.sin_family = AF_INET; a.sin_port = htons((uint16_t) port);
    if (::inet_pton(AF_INET, host, &a.sin_addr) != 1) { ::close(fd); return -1; }
    if (::connect(fd, (sockaddr *) &a, sizeof(a))) { ::close(fd); return -1; }
    int one = 1; ::setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
    return fd;
}
// A NAT'd worker can't be dialed: the backbone listens and the worker dials IN
// (directly or via the hub's /api/expert-relay WS bridge).
inline int listen_once(int port) {
    int srv = ::socket(AF_INET, SOCK_STREAM, 0);
    int one = 1; ::setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    sockaddr_in a {}; a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_ANY); a.sin_port = htons((uint16_t) port);
    if (::bind(srv, (sockaddr *) &a, sizeof(a)) || ::listen(srv, 1)) { ::close(srv); return -1; }
    std::fprintf(stderr, "moe-dispatch: waiting for a worker to dial :%d ...\n", port);
    int fd = ::accept(srv, nullptr, nullptr);
    ::close(srv);
    if (fd >= 0) { int one2 = 1; ::setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one2, sizeof(one2)); }
    return fd;
}

// ---- router state -------------------------------------------------------------
inline uint64_t now_ms() {
    return (uint64_t) std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
}

// Auto-switch (default ON): when no worker is *effectively* serving, un-flag the
// dispatch layer so build_moe_ffn takes its local GPU path instead of dispatch +
// slow single-threaded fallback. A worker gets a grace window on attach to prove
// it actually serves (attached != serving — a phone can dial the relay yet never
// compute). Env: LINKCPP_MOE_AUTOSWITCH=0 to disable, _SERVE_TTL_MS (how long a
// successful serve keeps dispatch on), _GRACE_MS (how long a fresh attach does).
inline bool g_autoswitch() {
    const char * v = std::getenv("LINKCPP_MOE_AUTOSWITCH");
    return !(v && (v[0] == '0' || v[0] == 'n' || v[0] == 'N'));
}
inline uint64_t g_serve_ttl_ms() {
    const char * v = std::getenv("LINKCPP_MOE_SERVE_TTL_MS");
    return v ? (uint64_t) std::strtoull(v, nullptr, 10) : 20000;
}
inline uint64_t g_grace_ms() {
    const char * v = std::getenv("LINKCPP_MOE_GRACE_MS");
    return v ? (uint64_t) std::strtoull(v, nullptr, 10) : 15000;
}
// Load-adaptive dispatch (Phase 1): when the coordinator's own inference slots are
// saturated (>= _SATURATE_SLOTS busy) the poll thread flags dispatch to offload
// experts onto available workers even if the local GPU is faster per token —
// trading a little latency for aggregate throughput under load. LINKCPP_MOE_SELF is
// the coordinator's own "host:port" whose /slots is polled; unset => feature off.
inline const char * g_self() { return std::getenv("LINKCPP_MOE_SELF"); }
inline int g_saturate_slots() {
    const char * v = std::getenv("LINKCPP_MOE_SATURATE_SLOTS");
    return v ? std::atoi(v) : 2;
}

struct worker_ep {
    int expert_begin = 0, expert_end = 0;
    std::string host; int port = 0; bool listen = false;
    int fd = -1; bool dead = false;
    std::string worker_id;              // stable identity across dynamic poll cycles
    bool has_listener = false;          // a background accept thread owns this ep's port
};

struct dispatch_state {
    // Reserved so push_back never reallocates — the per-worker listener threads
    // hold stable &eps[i] pointers (dynamic auto-wiring, up to 64 workers).
    std::vector<worker_ep> eps;
    std::mutex eps_mu;                  // guards eps between the poll thread + dispatch
    dispatch_state() { eps.reserve(64); }
    expert_slice slice;                 // local fallback / hot cache source
    bool have_slice = false;

    int hot_k = 0;                      // 0 = hot cache off
    std::vector<long> counts;           // per-expert dispatch counts
    std::vector<char> hot;              // hot[e] = 1 if served locally
    long calls = 0, pairs_total = 0, pairs_hot = 0;

    // P2-2 batch amortization: with the coordinator served --parallel N +
    // continuous batching, concurrent requests share one decode ubatch, so a
    // dispatch call carries rows from many streams in ONE round-trip. tokens_sum
    // / calls is the mean tokens amortized per dispatch (>1 == batching works).
    long tokens_sum = 0, tokens_max = 0;
    bool batch_stats = false;

    // Auto-switch health signals (see g_autoswitch). Stamped by the dispatch
    // callbacks (decode thread) and the ep listeners; read by the poll thread.
    std::atomic<uint64_t> last_serve_ms{0};    // a dispatch to a worker last succeeded
    std::atomic<uint64_t> last_attach_ms{0};   // a worker last attached its socket
    bool dispatch_flagged = true;              // poll-thread-only: current keystone flag state

    void note_batch(int n_tokens) {
        tokens_sum += n_tokens;
        if (n_tokens > tokens_max) tokens_max = n_tokens;
        if (batch_stats && (calls == 1 || calls % 32 == 0)) {
            std::fprintf(stderr, "moe-dispatch: batch mean=%.2f max=%ld over %ld calls (%ld tokens)\n",
                         (double) tokens_sum / (double) calls, tokens_max, calls, tokens_sum);
            std::fflush(stderr);
        }
    }
};

// "0-128@listen:52910,0-128@127.0.0.1:52901,128-256@127.0.0.1:52902"
inline bool parse_map(const char * spec, std::vector<worker_ep> & out) {
    std::string s(spec);
    size_t p = 0;
    while (p < s.size()) {
        size_t q = s.find(',', p);
        if (q == std::string::npos) q = s.size();
        std::string e = s.substr(p, q - p);
        p = q + 1;
        const size_t dash = e.find('-'), at = e.find('@'), col = e.rfind(':');
        if (dash == std::string::npos || at == std::string::npos || col == std::string::npos || col < at) return false;
        worker_ep w;
        w.expert_begin = std::atoi(e.substr(0, dash).c_str());
        w.expert_end   = std::atoi(e.substr(dash + 1, at - dash - 1).c_str());
        w.host         = e.substr(at + 1, col - at - 1);
        w.port         = std::atoi(e.substr(col + 1).c_str());
        w.listen       = (w.host == "listen");
        if (w.expert_end <= w.expert_begin || w.port <= 0) return false;
        out.push_back(w);
    }
    return !out.empty();
}

inline bool ep_dispatch(worker_ep & w, int n_embd, int n_pairs,
                        const float * rows, const int32_t * local_ids, float * out_rows) {
    int32_t hdr[2] = { 1, n_pairs };
    if (!io_send(w.fd, hdr, sizeof(hdr))) return false;
    if (!io_send(w.fd, rows, (size_t) n_embd * n_pairs * sizeof(float))) return false;
    if (!io_send(w.fd, local_ids, (size_t) n_pairs * sizeof(int32_t))) return false;
    return io_recv(w.fd, out_rows, (size_t) n_embd * n_pairs * sizeof(float));
}

// Gather rows/local ids for a pair subset and scatter results back.
struct sub_req {
    worker_ep *          w = nullptr;
    std::vector<int>     idx;
    std::vector<float>   rows;
    std::vector<int32_t> ids;
    std::vector<float>   out;
    bool                 ok = false;
};

inline void fill_sub(sub_req & r, int n_embd, int n_used, const float * cur,
                     const int32_t * sel, bool local_ids) {
    const int m = (int) r.idx.size();
    r.rows.resize((size_t) m * n_embd);
    r.ids.resize((size_t) m);
    r.out.resize((size_t) m * n_embd);
    for (int i = 0; i < m; ++i) {
        const int p = r.idx[i], tok = p / n_used;
        std::memcpy(r.rows.data() + (size_t) i * n_embd, cur + (size_t) tok * n_embd,
                    (size_t) n_embd * sizeof(float));
        r.ids[i] = local_ids && r.w ? sel[p] - r.w->expert_begin : sel[p];
    }
}

inline void scatter_sub(const sub_req & r, int n_embd, float * experts_out, std::vector<char> & done, int & remaining) {
    for (size_t i = 0; i < r.idx.size(); ++i) {
        std::memcpy(experts_out + (size_t) r.idx[i] * n_embd, r.out.data() + i * (size_t) n_embd,
                    (size_t) n_embd * sizeof(float));
        if (!done[r.idx[i]]) { done[r.idx[i]] = 1; --remaining; }
    }
}

inline bool map_dispatch_cb(int32_t il, int32_t n_embd, int32_t n_used, int32_t n_tokens,
                            const float * cur, const int32_t * sel, float * experts_out, void * ud) {
    dispatch_state & st = *(dispatch_state *) ud;
    std::lock_guard<std::mutex> _eps_lk(st.eps_mu);  // vs the dynamic poll thread
    const int n_pairs = n_used * n_tokens;      // pair p = t*n_used + u, expert sel[p]
    std::vector<char> done((size_t) n_pairs, 0);
    int remaining = n_pairs;

    st.calls++;
    st.pairs_total += n_pairs;
    st.note_batch(n_tokens);

    // hot-expert accounting + refresh every 64 calls
    if (st.hot_k > 0 && st.have_slice) {
        for (int p = 0; p < n_pairs; ++p) {
            if (sel[p] >= (int) st.counts.size()) { st.counts.resize((size_t) sel[p] + 1, 0); st.hot.resize((size_t) sel[p] + 1, 0); }
            st.counts[sel[p]]++;
        }
        if (st.calls % 64 == 1) {
            std::vector<int> order((int) st.counts.size());
            for (size_t i = 0; i < order.size(); ++i) order[i] = (int) i;
            std::partial_sort(order.begin(), order.begin() + std::min((size_t) st.hot_k, order.size()), order.end(),
                              [&](int a, int b) { return st.counts[a] > st.counts[b]; });
            std::fill(st.hot.begin(), st.hot.end(), 0);
            for (int i = 0; i < st.hot_k && i < (int) order.size(); ++i) st.hot[order[i]] = 1;
        }
        sub_req local;
        for (int p = 0; p < n_pairs; ++p)
            if (sel[p] < (int) st.hot.size() && st.hot[sel[p]]) local.idx.push_back(p);
        if (!local.idx.empty()) {
            fill_sub(local, n_embd, n_used, cur, sel, false);
            if (compute_experts(st.slice, il, n_embd, 1, (int) local.idx.size(),
                                local.rows.data(), local.ids.data(), local.out.data())) {
                scatter_sub(local, n_embd, experts_out, done, remaining);
                st.pairs_hot += (long) local.idx.size();
            }
        }
        if (st.calls % 128 == 0)
            std::fprintf(stderr, "moe-dispatch: hot-cache %ld/%ld pairs local (%.1f%%)\n",
                         st.pairs_hot, st.pairs_total, 100.0 * st.pairs_hot / (double) st.pairs_total);
    }

    // rounds: assign each pair to its first live replica, dispatch all workers
    // concurrently, mark failures dead, retry survivors next round.
    while (remaining > 0) {
        std::vector<sub_req> subs;
        std::vector<char> claimed((size_t) n_pairs, 0);
        for (auto & w : st.eps) {
            if (w.dead || w.fd < 0) continue;
            sub_req r; r.w = &w;
            for (int p = 0; p < n_pairs; ++p)
                if (!done[p] && !claimed[p] && sel[p] >= w.expert_begin && sel[p] < w.expert_end) {
                    r.idx.push_back(p); claimed[p] = 1;
                }
            if (!r.idx.empty()) subs.push_back(std::move(r));
        }
        if (subs.empty()) break;
        std::vector<std::thread> ts;
        ts.reserve(subs.size());
        for (auto & r : subs)
            ts.emplace_back([&r, n_embd, n_used, cur, sel] {
                fill_sub(r, n_embd, n_used, cur, sel, true);
                r.ok = ep_dispatch(*r.w, n_embd, (int) r.idx.size(), r.rows.data(), r.ids.data(), r.out.data());
            });
        for (auto & t : ts) t.join();
        bool progressed = false;
        for (auto & r : subs) {
            if (r.ok) {
                scatter_sub(r, n_embd, experts_out, done, remaining);
                progressed = true;
                st.last_serve_ms = now_ms();   // a real worker served — keep dispatch on
            } else {
                std::fprintf(stderr, "moe-dispatch: worker %d-%d@%s:%d FAILED — failing over\n",
                             r.w->expert_begin, r.w->expert_end, r.w->host.c_str(), r.w->port);
                r.w->dead = true; ::close(r.w->fd); r.w->fd = -1;
            }
        }
        if (!progressed) break;
    }

    if (remaining > 0) {                         // last resort: compute locally
        if (!st.have_slice) return false;
        sub_req local;
        for (int p = 0; p < n_pairs; ++p) if (!done[p]) local.idx.push_back(p);
        std::fprintf(stderr, "moe-dispatch: local fallback for %d pair(s)\n", (int) local.idx.size());
        fill_sub(local, n_embd, n_used, cur, sel, false);
        if (!compute_experts(st.slice, il, n_embd, 1, (int) local.idx.size(),
                             local.rows.data(), local.ids.data(), local.out.data())) return false;
        scatter_sub(local, n_embd, experts_out, done, remaining);
    }
    return true;
}

#ifdef LLAMA_LINKCPP_MOE_REDUCE
// ---- v2 wire: row-dedup dispatch + weighted partial-sum combine ---------------
// P1-1: when true, hidden rows + probs go out F16 and the reduced result comes
// back F16 (2× on both directions). We already accept cross-backend cosine
// ~0.998, so F16's ~1e-3 rounding is within budget; gate on the argmax/cosine
// harness. Selected by LINKCPP_MOE_DISPATCH_WIRE=v2f16.
inline bool & g_wire_f16() { static bool v = false; return v; }

// v2  request : i32{-2, n_rows} + i32 n_pairs + rows f32[n_embd·n_rows]
//               + row_idx i32[n_pairs] + local_ids i32[n_pairs] + probs f32[n_pairs]
//     response: f32[n_embd·n_rows]
// v2f16 request: i32{-3, n_rows} + i32 n_pairs + rows f16 + row_idx i32
//               + local_ids i32 + probs f16 ; response: f16[n_embd·n_rows]
// Per-row Σ probs·expert; partial sums add across workers by linearity. Dispatch
// stops duplicating a token's hidden row per expert; combine shrinks by
// n_expert_used vs the v1 per-expert wire (and another 2× under f16).
inline bool ep_reduce(worker_ep & w, int n_embd, int n_rows, int n_pairs,
                      const float * rows, const int32_t * row_idx, const int32_t * local_ids,
                      const float * probs, float * out_rows) {
    if (g_wire_f16()) {
        int32_t hdr[3] = { -3, n_rows, n_pairs };
        std::vector<ggml_fp16_t> rows16((size_t) n_embd * n_rows), probs16((size_t) n_pairs), out16((size_t) n_embd * n_rows);
        ggml_fp32_to_fp16_row(rows,  rows16.data(),  (int64_t) n_embd * n_rows);
        ggml_fp32_to_fp16_row(probs, probs16.data(), (int64_t) n_pairs);
        if (!io_send(w.fd, hdr, sizeof(hdr))) return false;
        if (!io_send(w.fd, rows16.data(), rows16.size() * sizeof(ggml_fp16_t))) return false;
        if (!io_send(w.fd, row_idx, (size_t) n_pairs * sizeof(int32_t))) return false;
        if (!io_send(w.fd, local_ids, (size_t) n_pairs * sizeof(int32_t))) return false;
        if (!io_send(w.fd, probs16.data(), probs16.size() * sizeof(ggml_fp16_t))) return false;
        if (!io_recv(w.fd, out16.data(), out16.size() * sizeof(ggml_fp16_t))) return false;
        ggml_fp16_to_fp32_row(out16.data(), out_rows, (int64_t) n_embd * n_rows);
        return true;
    }
    int32_t hdr[3] = { -2, n_rows, n_pairs };
    if (!io_send(w.fd, hdr, sizeof(hdr))) return false;
    if (!io_send(w.fd, rows, (size_t) n_embd * n_rows * sizeof(float))) return false;
    if (!io_send(w.fd, row_idx, (size_t) n_pairs * sizeof(int32_t))) return false;
    if (!io_send(w.fd, local_ids, (size_t) n_pairs * sizeof(int32_t))) return false;
    if (!io_send(w.fd, probs, (size_t) n_pairs * sizeof(float))) return false;
    return io_recv(w.fd, out_rows, (size_t) n_embd * n_rows * sizeof(float));
}

struct reduce_req {
    worker_ep *          w = nullptr;
    std::vector<int>     idx;        // pair indices
    std::vector<int>     toks;       // unique tokens, in first-seen order
    std::vector<float>   rows;       // one hidden row per unique token
    std::vector<int32_t> row_idx;    // per pair -> index into toks/rows
    std::vector<int32_t> ids;        // per pair, worker-local expert id
    std::vector<float>   probs;      // per pair, final router weight
    std::vector<float>   out;        // per unique token, weighted partial sum
    bool                 ok = false;
};

inline void fill_reduce(reduce_req & r, int n_embd, int n_used, const float * cur,
                        const int32_t * sel, const float * w_probs, bool local_ids) {
    const int m = (int) r.idx.size();
    r.row_idx.resize((size_t) m);
    r.ids.resize((size_t) m);
    r.probs.resize((size_t) m);
    std::vector<int> tok_slot;                    // token -> row slot (+1), sparse
    for (int i = 0; i < m; ++i) {
        const int p = r.idx[i], tok = p / n_used;
        if ((int) tok_slot.size() <= tok) tok_slot.resize((size_t) tok + 1, 0);
        if (!tok_slot[tok]) {
            r.toks.push_back(tok);
            tok_slot[tok] = (int) r.toks.size();
        }
        r.row_idx[i] = tok_slot[tok] - 1;
        r.ids[i]     = local_ids && r.w ? sel[p] - r.w->expert_begin : sel[p];
        r.probs[i]   = w_probs[p];
    }
    r.rows.resize(r.toks.size() * (size_t) n_embd);
    for (size_t t = 0; t < r.toks.size(); ++t)
        std::memcpy(r.rows.data() + t * (size_t) n_embd, cur + (size_t) r.toks[t] * n_embd,
                    (size_t) n_embd * sizeof(float));
    r.out.assign(r.toks.size() * (size_t) n_embd, 0.0f);
}

// Compute a reduce_req on a local slice (hot cache / fallback): per-expert FFN
// then the weighted per-row accumulation the worker would have done.
inline bool local_reduce(const expert_slice & s, int il, int n_embd, reduce_req & r) {
    const int m = (int) r.idx.size();
    std::vector<float> h((size_t) m * n_embd), e((size_t) m * n_embd);
    for (int i = 0; i < m; ++i)
        std::memcpy(h.data() + (size_t) i * n_embd, r.rows.data() + (size_t) r.row_idx[i] * n_embd,
                    (size_t) n_embd * sizeof(float));
    if (!compute_experts(s, il, n_embd, 1, m, h.data(), r.ids.data(), e.data())) return false;
    for (int i = 0; i < m; ++i) {
        float *       dst = r.out.data() + (size_t) r.row_idx[i] * n_embd;
        const float * src = e.data() + (size_t) i * n_embd;
        const float   p   = r.probs[i];
        for (int k = 0; k < n_embd; ++k) dst[k] += p * src[k];
    }
    return true;
}

inline bool map_reduce_cb(int32_t il, int32_t n_embd, int32_t n_used, int32_t n_tokens,
                          const float * cur, const int32_t * sel, const float * probs,
                          float * reduced_out, void * ud) {
    dispatch_state & st = *(dispatch_state *) ud;
    std::lock_guard<std::mutex> _eps_lk(st.eps_mu);  // vs the dynamic poll thread
    const int n_pairs = n_used * n_tokens;
    std::vector<char> done((size_t) n_pairs, 0);
    int remaining = n_pairs;
    std::memset(reduced_out, 0, (size_t) n_embd * n_tokens * sizeof(float));

    st.calls++;
    st.pairs_total += n_pairs;
    st.note_batch(n_tokens);

    auto accumulate = [&](const reduce_req & r) {
        for (size_t t = 0; t < r.toks.size(); ++t) {
            float *       dst = reduced_out + (size_t) r.toks[t] * n_embd;
            const float * src = r.out.data() + t * (size_t) n_embd;
            for (int k = 0; k < n_embd; ++k) dst[k] += src[k];
        }
        for (int p : r.idx) if (!done[p]) { done[p] = 1; --remaining; }
    };

    if (st.hot_k > 0 && st.have_slice) {
        for (int p = 0; p < n_pairs; ++p) {
            if (sel[p] >= (int) st.counts.size()) { st.counts.resize((size_t) sel[p] + 1, 0); st.hot.resize((size_t) sel[p] + 1, 0); }
            st.counts[sel[p]]++;
        }
        if (st.calls % 64 == 1) {
            std::vector<int> order((int) st.counts.size());
            for (size_t i = 0; i < order.size(); ++i) order[i] = (int) i;
            std::partial_sort(order.begin(), order.begin() + std::min((size_t) st.hot_k, order.size()), order.end(),
                              [&](int a, int b) { return st.counts[a] > st.counts[b]; });
            std::fill(st.hot.begin(), st.hot.end(), 0);
            for (int i = 0; i < st.hot_k && i < (int) order.size(); ++i) st.hot[order[i]] = 1;
        }
        reduce_req local;
        for (int p = 0; p < n_pairs; ++p)
            if (sel[p] < (int) st.hot.size() && st.hot[sel[p]]) local.idx.push_back(p);
        if (!local.idx.empty()) {
            fill_reduce(local, n_embd, n_used, cur, sel, probs, false);
            if (local_reduce(st.slice, il, n_embd, local)) {
                accumulate(local);
                st.pairs_hot += (long) local.idx.size();
            }
        }
    }

    while (remaining > 0) {
        std::vector<reduce_req> subs;
        std::vector<char> claimed((size_t) n_pairs, 0);
        for (auto & w : st.eps) {
            if (w.dead || w.fd < 0) continue;
            reduce_req r; r.w = &w;
            for (int p = 0; p < n_pairs; ++p)
                if (!done[p] && !claimed[p] && sel[p] >= w.expert_begin && sel[p] < w.expert_end) {
                    r.idx.push_back(p); claimed[p] = 1;
                }
            if (!r.idx.empty()) subs.push_back(std::move(r));
        }
        if (subs.empty()) break;
        std::vector<std::thread> ts;
        ts.reserve(subs.size());
        for (auto & r : subs)
            ts.emplace_back([&r, n_embd, n_used, cur, sel, probs] {
                fill_reduce(r, n_embd, n_used, cur, sel, probs, true);
                r.ok = ep_reduce(*r.w, n_embd, (int) r.toks.size(), (int) r.idx.size(),
                                 r.rows.data(), r.row_idx.data(), r.ids.data(), r.probs.data(),
                                 r.out.data());
            });
        for (auto & t : ts) t.join();
        bool progressed = false;
        for (auto & r : subs) {
            if (r.ok) {
                accumulate(r);
                progressed = true;
                st.last_serve_ms = now_ms();   // a real worker served — keep dispatch on
            } else {
                std::fprintf(stderr, "moe-dispatch: worker %d-%d@%s:%d FAILED — failing over\n",
                             r.w->expert_begin, r.w->expert_end, r.w->host.c_str(), r.w->port);
                r.w->dead = true; ::close(r.w->fd); r.w->fd = -1;
            }
        }
        if (!progressed) break;
    }

    if (remaining > 0) {
        if (!st.have_slice) return false;
        reduce_req local;
        for (int p = 0; p < n_pairs; ++p) if (!done[p]) local.idx.push_back(p);
        std::fprintf(stderr, "moe-dispatch: local fallback for %d pair(s)\n", (int) local.idx.size());
        fill_reduce(local, n_embd, n_used, cur, sel, probs, false);
        if (!local_reduce(st.slice, il, n_embd, local)) return false;
        accumulate(local);
    }
    return true;
}
#endif // LLAMA_LINKCPP_MOE_REDUCE

// Wire selection from LINKCPP_MOE_DISPATCH_WIRE: v1 (per-expert), v2 (reduce,
// default when the fork exposes the reduce hook), v2f16 (reduce + F16 payloads).
// As a side effect, records the F16 choice for ep_reduce.
inline bool use_reduce_wire() {
#ifdef LLAMA_LINKCPP_MOE_REDUCE
    const char * w = std::getenv("LINKCPP_MOE_DISPATCH_WIRE");
    if (w && std::strcmp(w, "v1") == 0) { g_wire_f16() = false; return false; }
    g_wire_f16() = (w && std::strcmp(w, "v2f16") == 0);
    return true;
#else
    return false;
#endif
}

// ---- dynamic auto-wiring: poll the hub, attach/detach workers live ----------
// A minimal raw-socket HTTP GET (the hub is plain HTTP on 127.0.0.1) — avoids
// pulling in an HTTP client. Returns the response body, or "" on failure.
inline std::string http_get(const std::string & host, int port, const std::string & path,
                            const std::string & token) {
    int fd = connect_host(host.c_str(), port);
    if (fd < 0) return "";
    // Bound this control-plane query: poll_loop calls it while holding eps_mu, so a
    // stalled /slots or hub must never wedge dispatch. (Worker data sockets dialed
    // via connect_host stay untouched — a slow expert compute must not time out.)
    struct timeval tv { 3, 0 };
    ::setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    ::setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
    std::string req = "GET " + path + " HTTP/1.1\r\nHost: " + host + "\r\nConnection: close\r\n";
    if (!token.empty()) req += "x-linkcpp-service-token: " + token + "\r\n";
    req += "\r\n";
    if (!io_send(fd, req.data(), req.size())) { ::close(fd); return ""; }
    std::string resp; char buf[4096];
    for (;;) { ssize_t k = ::recv(fd, buf, sizeof(buf), 0); if (k <= 0) break; resp.append(buf, (size_t) k); }
    ::close(fd);
    auto p = resp.find("\r\n\r\n");
    return p == std::string::npos ? "" : resp.substr(p + 4);
}

// Count the coordinator's own busy inference slots by polling its /slots (each
// slot object carries "is_processing":true while serving). -1 on query failure so
// the caller can leave the saturation state unchanged.
inline int count_busy_slots(const std::string & host, int port) {
    std::string body = http_get(host, port, "/slots", "");   // llama-server /slots needs no auth
    if (body.empty()) return -1;   // query failed; leave saturation state unchanged
    // Count directly in the body — /slots may be chunked (body then starts with a
    // hex chunk size, not '['), but the "is_processing":true tokens are still there.
    int busy = 0;
    for (size_t i = 0; (i = body.find("\"is_processing\":true", i)) != std::string::npos; i += 20) busy++;
    return busy;
}

// Extract every {experts:[a,b], listen_port:P, worker_id:"..."} row from the
// dispatch-map JSON. Deliberately tiny: scans for the three keys per object.
struct map_row { int a = 0, b = 0, port = 0; std::string worker_id; };
inline std::vector<map_row> parse_dispatch_rows(const std::string & body) {
    std::vector<map_row> rows;
    // Anchor on worker_id (the first field of each row object) and read experts
    // + listen_port that belong to THAT object — i.e. before the next worker_id.
    size_t i = 0;
    while ((i = body.find("\"worker_id\"", i)) != std::string::npos) {
        const size_t next = body.find("\"worker_id\"", i + 11);
        const size_t lim = next == std::string::npos ? body.size() : next;
        map_row r;
        size_t q1 = body.find('"', body.find(':', i));
        size_t q2 = q1 == std::string::npos ? q1 : body.find('"', q1 + 1);
        if (q1 != std::string::npos && q2 != std::string::npos && q2 < lim)
            r.worker_id = body.substr(q1 + 1, q2 - q1 - 1);
        size_t ep = body.find("\"experts\"", i);
        if (ep != std::string::npos && ep < lim) {
            size_t lb = body.find('[', ep);
            if (lb != std::string::npos && lb < lim) {
                r.a = std::atoi(body.c_str() + lb + 1);
                size_t comma = body.find(',', lb);
                if (comma != std::string::npos) r.b = std::atoi(body.c_str() + comma + 1);
            }
        }
        size_t pp = body.find("\"listen_port\"", i);
        if (pp != std::string::npos && pp < lim) {
            size_t colon = body.find(':', pp);
            if (colon != std::string::npos) r.port = std::atoi(body.c_str() + colon + 1);
        }
        if (r.b > r.a && r.port > 0 && !r.worker_id.empty()) rows.push_back(r);
        i += 11;
    }
    return rows;
}

// One background thread per dynamically-attached worker: owns the ep's listen
// port, (re)accepts the connection the hub bridges from the worker's 443 dial,
// and keeps ep.fd live. Exits when the ep is marked dead by the poll thread.
inline void ep_listener(worker_ep * w, dispatch_state * st) {
    int srv = ::socket(AF_INET, SOCK_STREAM, 0);
    int one = 1; ::setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    sockaddr_in a {}; a.sin_family = AF_INET; a.sin_addr.s_addr = htonl(INADDR_ANY);
    a.sin_port = htons((uint16_t) w->port);
    if (::bind(srv, (sockaddr *) &a, sizeof(a)) || ::listen(srv, 4)) { ::close(srv); return; }
    std::fprintf(stderr, "moe-dispatch: listening for worker %s on :%d (experts [%d,%d))\n",
                 w->worker_id.c_str(), w->port, w->expert_begin, w->expert_end);
    while (!w->dead) {
        int c = ::accept(srv, nullptr, nullptr);
        if (c < 0) { if (w->dead) break; continue; }
        int one2 = 1; ::setsockopt(c, IPPROTO_TCP, TCP_NODELAY, &one2, sizeof(one2));
        { std::lock_guard<std::mutex> lk(st->eps_mu); w->fd = c; w->dead = false; }
        st->last_attach_ms = now_ms();   // grace window: give this worker a chance to prove it serves
        std::fprintf(stderr, "moe-dispatch: worker %s attached on :%d\n", w->worker_id.c_str(), w->port);
        // hold this connection until it drops; the dispatch path uses w->fd.
        char probe; while (::recv(c, &probe, 1, MSG_PEEK | MSG_DONTWAIT) != 0) {
            if (w->dead) break;
            std::this_thread::sleep_for(std::chrono::milliseconds(200));
        }
        { std::lock_guard<std::mutex> lk(st->eps_mu); if (w->fd == c) { w->fd = -1; } }
        ::close(c);
        std::fprintf(stderr, "moe-dispatch: worker %s detached from :%d\n", w->worker_id.c_str(), w->port);
    }
    ::close(srv);
}

// Poll the hub's dispatch-map and reconcile eps: add a listener for each new
// worker row, mark eps whose row disappeared dead. Runs until process exit.
inline void poll_loop(dispatch_state * st, std::string hub_host, int hub_port,
                      std::string model, int layer, std::string token, int interval_s) {
    const std::string path = "/api/expert-dispatch-map?model=" + model +
                             "&layer=" + std::to_string(layer);
    // Coordinator's own endpoint, polled for slot saturation (load-adaptive dispatch).
    std::string self_host = "127.0.0.1"; int self_port = 0;
    if (const char * s = g_self()) {
        std::string ss(s); auto c = ss.rfind(':');
        if (c != std::string::npos) { self_host = ss.substr(0, c); self_port = std::atoi(ss.c_str() + c + 1); }
    }
    for (;;) {
        std::string body = http_get(hub_host, hub_port, path, token);
        auto rows = parse_dispatch_rows(body);
        std::lock_guard<std::mutex> lk(st->eps_mu);
        // add new
        for (auto & r : rows) {
            bool found = false;
            for (auto & w : st->eps) if (w.worker_id == r.worker_id && !w.dead) { found = true; break; }
            if (found) continue;
            if (st->eps.size() >= st->eps.capacity()) continue;   // reserved cap
            worker_ep w; w.expert_begin = r.a; w.expert_end = r.b; w.listen = true;
            w.port = r.port; w.worker_id = r.worker_id; w.has_listener = true;
            st->eps.push_back(std::move(w));
            worker_ep * wp = &st->eps.back();
            std::thread(ep_listener, wp, st).detach();
        }
        // mark rows that vanished dead (stop dispatching to them)
        for (auto & w : st->eps) {
            if (w.dead) continue;
            bool present = false;
            for (auto & r : rows) if (r.worker_id == w.worker_id) { present = true; break; }
            if (!present) { w.dead = true; if (w.fd >= 0) { ::close(w.fd); w.fd = -1; } }
        }
        // Auto-switch: keep dispatch on only while a worker is *effectively* serving
        // (recently succeeded, or freshly attached and still in its grace window).
        // Otherwise un-flag the layer so build_moe_ffn runs experts on the local GPU
        // — fast hub-only serving instead of dispatch + slow single-threaded fallback.
        // Held under eps_mu (acquired above), so no dispatch callback runs concurrently
        // with the flag change.
        if (g_autoswitch()) {
            bool live = false;
            for (auto & w : st->eps) if (!w.dead && w.fd >= 0) { live = true; break; }
            const uint64_t nowms = now_ms();
            const bool served  = live && (nowms - st->last_serve_ms.load())  < g_serve_ttl_ms();
            const bool freshly = live && (nowms - st->last_attach_ms.load()) < g_grace_ms();
            // Load-adaptive (Phase 1): if the coordinator's own slots are saturated
            // and there is a *proven* worker to offload to, dispatch even when the
            // local GPU would be faster per token — aggregate throughput over
            // per-request latency. Only recruit a worker that has served at least
            // once (last_serve_ms>0); dispatching to an attached-but-never-serving
            // worker just trades the fast local path for slow per-pair fallback.
            // A brand-new worker still gets its shot via the grace window (freshly).
            // Query fails (-1) => leave the health-only decision alone.
            const bool proven = st->last_serve_ms.load() > 0;
            int busy = self_port ? count_busy_slots(self_host, self_port) : -1;
            const bool saturated = live && proven && busy >= 0 && busy >= g_saturate_slots();
            const bool want = served || freshly || saturated;
            if (want != st->dispatch_flagged) {
                st->dispatch_flagged = want;
                const int lay = want ? layer : -1;
#ifdef LLAMA_LINKCPP_MOE_REDUCE
                if (use_reduce_wire()) llama_linkcpp_set_moe_reduce(map_reduce_cb, st, lay);
                else
#endif
                llama_linkcpp_set_moe_dispatch(map_dispatch_cb, st, lay);
                const char * why = !want ? "LOCAL GPU (no effective worker)"
                                   : (saturated && !served && !freshly) ? "DISPATCH (load-adaptive: slots saturated)"
                                   : "DISPATCH (worker serving)";
                std::fprintf(stderr, "moe-dispatch: layer-%d auto-switch -> %s (live=%d served=%d fresh=%d busy=%d/%d)\n",
                             layer, why, (int) live, (int) served, (int) freshly, busy, g_saturate_slots());
                std::fflush(stderr);
            }
        }
        std::this_thread::sleep_for(std::chrono::seconds(interval_s > 0 ? interval_s : 3));
    }
}

// Connect/listen every endpoint, load the fallback slice, install the callback.
inline bool activate(dispatch_state & st, int layer, const char * fallback_path) {
    if (fallback_path && *fallback_path && st.slice.load(fallback_path, /*force_cpu=*/true)) {
        st.have_slice = true;
        std::fprintf(stderr, "moe-dispatch: local fallback slice ready (backend=%s)\n",
                     ggml_backend_name(st.slice.backend));
    }
    for (auto & w : st.eps) {
        w.fd = w.listen ? listen_once(w.port) : connect_host(w.host.c_str(), w.port);
        if (w.fd < 0) w.dead = true;
        std::fprintf(stderr, "moe-dispatch: worker %d-%d@%s:%d %s\n", w.expert_begin, w.expert_end,
                     w.host.c_str(), w.port, w.fd >= 0 ? "connected" : "UNAVAILABLE");
    }
#ifdef LLAMA_LINKCPP_MOE_REDUCE
    if (use_reduce_wire()) {
        llama_linkcpp_set_moe_reduce(map_reduce_cb, &st, layer);
        std::fprintf(stderr, "moe-dispatch: layer-%d -> expert-range map (%zu workers, hot_k=%d, wire=%s)\n",
                     layer, st.eps.size(), st.hot_k, g_wire_f16() ? "v2f16" : "v2");
        return true;
    }
#endif
    llama_linkcpp_set_moe_dispatch(map_dispatch_cb, &st, layer);
    std::fprintf(stderr, "moe-dispatch: layer-%d -> expert-range map (%zu workers, hot_k=%d, wire=v1)\n",
                 layer, st.eps.size(), st.hot_k);
    return true;
}

// Dynamic mode: no static map. Install the callback with empty eps and start a
// background thread polling the hub's /api/expert-dispatch-map — workers attach
// and detach live as they register/drop coverage, with no operator action.
inline bool activate_dynamic(dispatch_state & st, int layer, const char * fallback_path,
                             const std::string & hub, const std::string & model,
                             const std::string & token, int interval_s) {
    if (fallback_path && *fallback_path && st.slice.load(fallback_path, /*force_cpu=*/true)) {
        st.have_slice = true;
        std::fprintf(stderr, "moe-dispatch: local fallback slice ready (backend=%s)\n",
                     ggml_backend_name(st.slice.backend));
    }
    std::string host = "127.0.0.1"; int port = 19000;
    auto colon = hub.rfind(':');
    if (colon != std::string::npos) { host = hub.substr(0, colon); port = std::atoi(hub.c_str() + colon + 1); }
    const char * wire = "v1";
    // With auto-switch, start on the local GPU path (layer un-flagged): there are
    // no workers yet, so serve fast immediately (this also keeps warmup off the
    // dispatch/fallback path). The poll thread flags the layer the moment a worker
    // attaches and un-flags it again when none is effectively serving.
    const int init_layer = g_autoswitch() ? -1 : layer;
    st.dispatch_flagged = (init_layer == layer);
#ifdef LLAMA_LINKCPP_MOE_REDUCE
    if (use_reduce_wire()) { llama_linkcpp_set_moe_reduce(map_reduce_cb, &st, init_layer); wire = g_wire_f16() ? "v2f16" : "v2"; }
    else
#endif
    llama_linkcpp_set_moe_dispatch(map_dispatch_cb, &st, init_layer);
    std::thread(poll_loop, &st, host, port, model, layer, token, interval_s).detach();
    std::fprintf(stderr, "moe-dispatch: layer-%d DYNAMIC via hub %s:%d model=%s (wire=%s, hot_k=%d, autoswitch=%d)\n",
                 layer, host.c_str(), port, model.c_str(), wire, st.hot_k, (int) g_autoswitch());
    return true;
}

// linkcpp-server entrypoint: configure everything from the environment. Inert
// unless a static map (LINKCPP_MOE_DISPATCH_MAP) or dynamic hub
// (LINKCPP_MOE_DISPATCH_HUB=host:port + _MODEL) is set.
inline bool setup_from_env(dispatch_state & st) {
    const char * lv = std::getenv("LINKCPP_MOE_DISPATCH_LAYER");
    const char * hk = std::getenv("LINKCPP_MOE_HOT_EXPERTS");
    const char * fb = std::getenv("LINKCPP_MOE_DISPATCH_FALLBACK");
    const int layer = lv ? std::atoi(lv) : 0;
    st.hot_k = hk ? std::atoi(hk) : 0;
    st.batch_stats = std::getenv("LINKCPP_MOE_BATCH_STATS") != nullptr;

    const char * hub = std::getenv("LINKCPP_MOE_DISPATCH_HUB");
    const char * model = std::getenv("LINKCPP_MOE_DISPATCH_MODEL");
    if (hub && *hub && model && *model) {
        const char * tok = std::getenv("LINKCPP_HUB_SERVICE_TOKEN");
        const char * iv  = std::getenv("LINKCPP_MOE_DISPATCH_POLL_SEC");
        return activate_dynamic(st, layer, fb, hub, model, tok ? tok : "", iv ? std::atoi(iv) : 3);
    }

    const char * map = std::getenv("LINKCPP_MOE_DISPATCH_MAP");
    if (!map || !*map) return false;
    if (!parse_map(map, st.eps)) {
        std::fprintf(stderr, "moe-dispatch: bad LINKCPP_MOE_DISPATCH_MAP\n");
        return false;
    }
    return activate(st, layer, fb);
}

} // namespace linkcpp_moe
