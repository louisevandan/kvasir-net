// linkcpp-moe-verify — functional check of the build_moe_ffn expert-dispatch hook.
//
// Loads a MoE model, decodes a prompt with the dispatch hook OFF (experts computed
// locally) and again with it ON for one layer (that layer's routed-expert FFN handed
// to a callback that runs the expert-worker graph over a same-layer expert slice),
// and compares the two logit vectors. If the hook is wired correctly, the outputs
// match to fp precision (both computed on the same backend from the same weights).
//
// This exercises the keystone end-to-end inside a real llama_decode. Standalone,
// pure llama+ggml.

#include "llama.h"
#include "ggml.h"
#include "ggml-backend.h"
#include "gguf.h"

#include "../linkcpp-moe-dispatch.h"   // shared expert-range router (M3/M4)

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>
#include <chrono>

#include <arpa/inet.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <sys/socket.h>
#include <unistd.h>

// ---- expert slice (mirrors linkcpp-expert-worker) --------------------------
struct expert_slice {
    gguf_context *        gguf   = nullptr;
    ggml_context *        meta   = nullptr;
    ggml_backend_t        backend = nullptr;
    ggml_backend_buffer_t buffer = nullptr;

    ggml_tensor * find(const std::string & n) const { return ggml_get_tensor(meta, n.c_str()); }

    bool load(const char * path) {
        backend = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_GPU, nullptr);
        if (!backend) backend = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_CPU, nullptr);
        if (!backend) return false;
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
static bool compute_experts(const expert_slice & s, int il, int n_embd, int n_used,
                            int n_tokens, const float * cur, const int32_t * sel, float * out) {
    const std::string p = "blk." + std::to_string(il) + ".ffn_";
    ggml_tensor * up_exps   = s.find(p + "up_exps.weight");
    ggml_tensor * gate_exps = s.find(p + "gate_exps.weight");
    ggml_tensor * down_exps = s.find(p + "down_exps.weight");
    if (!up_exps || !gate_exps || !down_exps) return false;

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
        ggml_backend_tensor_set(id, sel, 0, (size_t) n_used * n_tokens * sizeof(int32_t));
        ok = ggml_backend_graph_compute(s.backend, gf) == GGML_STATUS_SUCCESS;
    }
    if (ok) ggml_backend_tensor_get(o, out, 0, (size_t) n_embd * n_used * n_tokens * sizeof(float));
    ggml_gallocr_free(alloc);
    ggml_free(ctx);
    return ok;
}

static const expert_slice * g_slice = nullptr;

static bool dispatch_cb(int32_t il, int32_t n_embd, int32_t n_used, int32_t n_tokens,
                        const float * cur, const int32_t * sel, float * experts_out, void * ud) {
    (void) ud;
    return compute_experts(*g_slice, il, n_embd, n_used, n_tokens, cur, sel, experts_out);
}

// ---- remote dispatch: send (cur, sel) to a worker process, receive experts --
static int g_worker_fd = -1;
static bool rio_send(int fd, const void * p, size_t n) {
    const char * c = (const char *) p;
    while (n) { ssize_t k = ::send(fd, c, n, 0); if (k <= 0) return false; c += k; n -= (size_t) k; }
    return true;
}
static bool rio_recv(int fd, void * p, size_t n) {
    char * c = (char *) p;
    while (n) { ssize_t k = ::recv(fd, c, n, 0); if (k <= 0) return false; c += k; n -= (size_t) k; }
    return true;
}
static bool remote_dispatch_cb(int32_t il, int32_t n_embd, int32_t n_used, int32_t n_tokens,
                               const float * cur, const int32_t * sel, float * experts_out, void * ud) {
    (void) il; (void) ud;
    int32_t hdr[2] = { n_used, n_tokens };
    if (!rio_send(g_worker_fd, hdr, sizeof(hdr))) return false;
    if (!rio_send(g_worker_fd, cur, (size_t) n_embd * n_tokens * sizeof(float))) return false;
    if (!rio_send(g_worker_fd, sel, (size_t) n_used * n_tokens * sizeof(int32_t))) return false;
    return rio_recv(g_worker_fd, experts_out, (size_t) n_embd * n_used * n_tokens * sizeof(float));
}
static int connect_worker(int port) {
    int fd = ::socket(AF_INET, SOCK_STREAM, 0);
    sockaddr_in a {}; a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK); a.sin_port = htons((uint16_t) port);
    if (::connect(fd, (sockaddr *) &a, sizeof(a))) { ::close(fd); return -1; }
    int one = 1; ::setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
    return fd;
}
// The inverse: a NAT'd worker can't be dialed, so the backbone listens and the
// worker dials IN (directly or via the hub's /api/expert-relay WS bridge). Same
// wire protocol — the stream is bidirectional regardless of who connected.
static int listen_worker(int port) {
    int srv = ::socket(AF_INET, SOCK_STREAM, 0);
    int one = 1; ::setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    sockaddr_in a {}; a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_ANY); a.sin_port = htons((uint16_t) port);
    if (::bind(srv, (sockaddr *) &a, sizeof(a)) || ::listen(srv, 1)) { ::close(srv); return -1; }
    std::fprintf(stderr, "waiting for a worker to dial dispatch port :%d ...\n", port);
    int fd = ::accept(srv, nullptr, nullptr);
    ::close(srv);
    if (fd >= 0) { int one2 = 1; ::setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one2, sizeof(one2)); }
    return fd;
}

static int argmax(const float * v, int n) { int a = 0; for (int i = 1; i < n; ++i) if (v[i] > v[a]) a = i; return a; }

// ---- greedy-generate `gen` tokens, timing the loop -> TPS -------------------
static std::vector<llama_token> generate_tps(llama_model * model, const std::vector<llama_token> & prompt,
                                             int n_vocab, int gen, double & tps) {
    llama_context_params cp = llama_context_default_params();
    cp.n_ctx = 512; cp.n_batch = 512;
    llama_context * ctx = llama_init_from_model(model, cp);
    std::vector<llama_token> toks = prompt, produced;
    llama_batch pb = llama_batch_get_one(toks.data(), (int32_t) toks.size());
    if (llama_decode(ctx, pb) != 0) { llama_free(ctx); tps = 0; return produced; }
    int last_idx = (int) toks.size() - 1;
    auto t0 = std::chrono::steady_clock::now();
    for (int i = 0; i < gen; ++i) {
        const float * lg = llama_get_logits_ith(ctx, last_idx);
        llama_token nt = (llama_token) argmax(lg, n_vocab);
        produced.push_back(nt);
        if (llama_decode(ctx, llama_batch_get_one(&nt, 1)) != 0) break;
        last_idx = 0;
    }
    auto t1 = std::chrono::steady_clock::now();
    tps = gen / std::chrono::duration<double>(t1 - t0).count();
    llama_free(ctx);
    return produced;
}

// ---- decode a prompt, return a copy of the last-token logits ----------------
static std::vector<float> decode_logits(llama_model * model, const std::vector<llama_token> & toks, int n_vocab) {
    llama_context_params cp = llama_context_default_params();
    cp.n_ctx = 128; cp.n_batch = 128;
    llama_context * ctx = llama_init_from_model(model, cp);
    llama_batch batch = llama_batch_get_one(const_cast<llama_token *>(toks.data()), (int32_t) toks.size());
    std::vector<float> out;
    if (llama_decode(ctx, batch) == 0) {
        const float * lg = llama_get_logits_ith(ctx, (int32_t) toks.size() - 1);
        out.assign(lg, lg + n_vocab);
    }
    llama_free(ctx);
    return out;
}

int main(int argc, char ** argv) {
    const char * model_path = argc > 1 ? argv[1] : nullptr;
    const char * slice_path = argc > 2 ? argv[2] : nullptr;
    const int    layer      = argc > 3 ? std::atoi(argv[3]) : 0;
    const char * prompt     = argc > 4 ? argv[4] : "The capital of France is";
    if (!model_path || !slice_path) {
        std::fprintf(stderr, "usage: linkcpp-moe-verify <model.gguf> <expert-slice.gguf> [layer] [prompt]\n");
        return 2;
    }
    llama_backend_init();

    llama_model_params mp = llama_model_default_params();
    mp.n_gpu_layers = 0; // CPU backbone for a clean same-backend comparison
    llama_model * model = llama_model_load_from_file(model_path, mp);
    if (!model) { std::fprintf(stderr, "model load failed\n"); return 1; }
    const llama_vocab * vocab = llama_model_get_vocab(model);
    const int n_vocab = llama_vocab_n_tokens(vocab);

    const char * dport = nullptr, * dlisten = nullptr, * dmap = nullptr;
    for (int i = 1; i + 1 < argc; ++i) if (!std::strcmp(argv[i], "--dispatch-port"))   dport   = argv[i + 1];
    for (int i = 1; i + 1 < argc; ++i) if (!std::strcmp(argv[i], "--dispatch-listen")) dlisten = argv[i + 1];
    for (int i = 1; i + 1 < argc; ++i) if (!std::strcmp(argv[i], "--dispatch-map"))    dmap    = argv[i + 1];
    llama_linkcpp_moe_dispatch_t on_cb = nullptr;
    void * on_ud = nullptr;
    expert_slice slice;
    static linkcpp_moe::dispatch_state map_state;
    if (dmap) {
        if (!linkcpp_moe::parse_map(dmap, map_state.eps)) { std::fprintf(stderr, "bad --dispatch-map\n"); return 1; }
        const char * hx = nullptr;
        for (int i = 1; i + 1 < argc; ++i) if (!std::strcmp(argv[i], "--hot-experts")) hx = argv[i + 1];
        map_state.hot_k = hx ? std::atoi(hx) : 0;
        map_state.batch_stats = std::getenv("LINKCPP_MOE_BATCH_STATS") != nullptr;
        if (map_state.slice.load(slice_path)) {   // backbone = replica of last resort
            map_state.have_slice = true;
            std::fprintf(stderr, "local fallback slice ready (backend=%s)\n",
                         ggml_backend_name(map_state.slice.backend));
        }
        for (auto & w : map_state.eps) {
            w.fd = w.listen ? linkcpp_moe::listen_once(w.port)
                            : linkcpp_moe::connect_host(w.host.c_str(), w.port);
            if (w.fd < 0) w.dead = true;
            std::fprintf(stderr, "worker %d-%d@%s:%d %s\n", w.expert_begin, w.expert_end,
                         w.host.c_str(), w.port, w.fd >= 0 ? "connected" : "UNAVAILABLE");
        }
        on_cb = linkcpp_moe::map_dispatch_cb;
        on_ud = &map_state;
        const bool reduce = linkcpp_moe::use_reduce_wire();
        std::fprintf(stderr, "layer-%d dispatch -> expert-range map (%zu workers, hot_k=%d, wire=%s)\n",
                     layer, map_state.eps.size(), map_state.hot_k,
                     reduce ? (linkcpp_moe::g_wire_f16() ? "v2f16" : "v2") : "v1");
    } else if (dport) {
        g_worker_fd = connect_worker(std::atoi(dport));
        if (g_worker_fd < 0) { std::fprintf(stderr, "connect worker :%s failed\n", dport); return 1; }
        on_cb = remote_dispatch_cb;
        std::fprintf(stderr, "layer-%d dispatch -> SEPARATE worker process 127.0.0.1:%s\n", layer, dport);
    } else if (dlisten) {
        g_worker_fd = listen_worker(std::atoi(dlisten));
        if (g_worker_fd < 0) { std::fprintf(stderr, "listen :%s failed\n", dlisten); return 1; }
        on_cb = remote_dispatch_cb;
        std::fprintf(stderr, "layer-%d dispatch -> worker that DIALED IN on :%s\n", layer, dlisten);
    } else {
        if (!slice.load(slice_path)) { std::fprintf(stderr, "slice load failed\n"); return 1; }
        g_slice = &slice;
        on_cb = dispatch_cb;
        std::fprintf(stderr, "layer-%d in-process dispatch; slice backend=%s\n",
                     layer, ggml_backend_name(slice.backend));
    }

    std::vector<llama_token> toks(64);
    int n = llama_tokenize(vocab, prompt, (int32_t) strlen(prompt), toks.data(), (int32_t) toks.size(), true, false);
    if (n < 0) { std::fprintf(stderr, "tokenize failed\n"); return 1; }
    toks.resize(n);
    std::fprintf(stderr, "prompt tokens: %d\n", n);

    // Install/clear the dispatch hook — the map mode prefers the v2 (reduce)
    // wire when the fork exposes it; every other mode stays on the v1 hook.
    auto hook_on = [&]() {
#ifdef LLAMA_LINKCPP_MOE_REDUCE
        if (dmap && linkcpp_moe::use_reduce_wire()) {
            llama_linkcpp_set_moe_reduce(linkcpp_moe::map_reduce_cb, &map_state, layer);
            return;
        }
#endif
        llama_linkcpp_set_moe_dispatch(on_cb, on_ud, layer);
    };
    auto hook_off = [&]() {
#ifdef LLAMA_LINKCPP_MOE_REDUCE
        llama_linkcpp_set_moe_reduce(nullptr, nullptr, -1);
#endif
        llama_linkcpp_set_moe_dispatch(nullptr, nullptr, -1);
    };

    int gen = 0;
    for (int i = 1; i + 1 < argc; ++i) if (!std::strcmp(argv[i], "--gen")) gen = std::atoi(argv[i + 1]);
    if (gen > 0) {
        hook_off();
        double tps_off = 0; auto seq_off = generate_tps(model, toks, n_vocab, gen, tps_off);
        hook_on();
        double tps_on = 0; auto seq_on = generate_tps(model, toks, n_vocab, gen, tps_on);
        hook_off();
        int match = 0;
        for (size_t i = 0; i < seq_off.size() && i < seq_on.size(); ++i) if (seq_off[i] == seq_on[i]) ++match;
        std::printf("=== generate %d tokens, layer %d dispatched ===\n", gen, layer);
        std::printf("TPS   OFF(local)=%.3f   ON(dispatched)=%.3f\n", tps_off, tps_on);
        std::printf("token-seq match ON vs OFF: %d/%zu %s\n", match, seq_off.size(),
                    match == (int) seq_off.size() ? "IDENTICAL" : "differ");
        char buf[512];
        std::string txt;
        for (llama_token t : seq_on) { int m = llama_token_to_piece(vocab, t, buf, sizeof(buf), 0, true); if (m > 0) txt.append(buf, m); }
        std::printf("generated (ON): \"%s\"\n", txt.c_str());
        if (dmap && map_state.hot_k > 0)
            std::printf("hot-cache: %ld/%ld pairs served locally (%.1f%%)\n",
                        map_state.pairs_hot, map_state.pairs_total,
                        100.0 * map_state.pairs_hot / (double) (map_state.pairs_total ? map_state.pairs_total : 1));
        llama_model_free(model); llama_backend_free();
        return 0;
    }

    hook_off();
    std::vector<float> off = decode_logits(model, toks, n_vocab);
    std::fprintf(stderr, "decode OFF done (%zu logits)\n", off.size());

    hook_on();
    std::vector<float> on = decode_logits(model, toks, n_vocab);
    hook_off();
    std::fprintf(stderr, "decode ON (layer %d dispatched) done (%zu logits)\n", layer, on.size());

    if (off.size() != on.size() || off.empty()) { std::fprintf(stderr, "size mismatch/empty\n"); return 1; }
    double maxd = 0, dot = 0, no = 0, non = 0; int amax_off = 0, amax_on = 0;
    for (size_t i = 0; i < off.size(); ++i) {
        maxd = std::max(maxd, (double) std::fabs(off[i] - on[i]));
        dot += (double) off[i] * on[i]; no += (double) off[i] * off[i]; non += (double) on[i] * on[i];
        if (off[i] > off[amax_off]) amax_off = (int) i;
        if (on[i]  > on[amax_on])   amax_on  = (int) i;
    }
    const double cos = dot / (std::sqrt(no) * std::sqrt(non) + 1e-12);
    std::printf("=== dispatch-hook verify (layer %d) : OFF vs ON ===\n", layer);
    std::printf("n_vocab=%d  max|dlogit|=%.3e  cosine=%.8f\n", n_vocab, maxd, cos);
    std::printf("argmax OFF=%d  ON=%d  %s\n", amax_off, amax_on, amax_off == amax_on ? "MATCH" : "DIFFER");

    llama_model_free(model);
    llama_backend_free();
    return 0;
}
