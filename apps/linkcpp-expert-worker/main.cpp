// linkcpp-expert-worker — the MoE expert-parallel worker (M1 of
// docs/design/moe-expert-sharding.md).
//
// It is NOT a transformer: no attention, no KV, no sampler, no router. It loads
// only an expert-sliced mini-GGUF (blk.N.ffn_{gate,up,down}_exps for experts
// [expert_begin, expert_end) of some layers) and evaluates, for dispatched
// hidden states and per-token LOCAL expert ids, the exact per-expert function
// from llama.cpp build_moe_ffn:
//
//     up   = mul_mat_id(up_exps,   hidden, ids)     // [n_ff,  n_used, n_tokens]
//     gate = mul_mat_id(gate_exps, hidden, ids)
//     cur  = swiglu_split(gate, up)                 // silu(gate) * up
//     out  = mul_mat_id(down_exps, cur, ids)        // [n_embd, n_used, n_tokens]
//
// The backbone applies routing weights and sums across experts, so `out` is the
// raw per-(token,slot) expert output. This is the kernel the numerical oracle
// docs/design/moe_expert_parallel_reference.py verified (3.6e-12 vs monolithic).
//
// --self-test mode reads hidden/ids from binary files and writes `out`, so a
// Python harness can compare against the oracle on any backend (CUDA/ROCm/…).

#include "ggml.h"
#include "ggml-backend.h"
#include "gguf.h"

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <string>
#include <vector>
#include <chrono>
#include <mutex>
#include <thread>

#include <arpa/inet.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <sys/socket.h>
#include <unistd.h>

namespace {

const char * arg_value(int argc, char ** argv, const char * key) {
    for (int i = 1; i + 1 < argc; ++i) {
        if (!std::strcmp(argv[i], key)) return argv[i + 1];
    }
    return nullptr;
}

int32_t meta_i32(const gguf_context * gguf, const char * key, int32_t fallback) {
    const int64_t id = gguf_find_key(gguf, key);
    return id < 0 ? fallback : gguf_get_val_i32(gguf, id);
}

std::vector<uint8_t> read_file(const char * path, size_t expect_bytes) {
    FILE * f = std::fopen(path, "rb");
    if (!f) { std::fprintf(stderr, "cannot open %s\n", path); return {}; }
    std::fseek(f, 0, SEEK_END);
    const long n = std::ftell(f);
    std::fseek(f, 0, SEEK_SET);
    std::vector<uint8_t> buf((size_t) n);
    if (n > 0 && std::fread(buf.data(), 1, (size_t) n, f) != (size_t) n) buf.clear();
    std::fclose(f);
    if (expect_bytes && buf.size() != expect_bytes) {
        std::fprintf(stderr, "%s: expected %zu bytes, got %zu\n", path, expect_bytes, buf.size());
        buf.clear();
    }
    return buf;
}

// Loads the expert-sliced GGUF's tensors onto `backend`. Metadata comes from a
// no_alloc ggml_context; raw tensor bytes are streamed from the file into the
// backend buffer so quantized experts live where the matmuls run.
struct expert_shard {
    gguf_context *        gguf   = nullptr;
    ggml_context *        meta   = nullptr;
    ggml_backend_buffer_t buffer = nullptr;

    int32_t expert_begin = 0, expert_end = 0, n_expert_global = 0;
    int32_t n_embd = 0, n_layer = 0;   // model dims carried on the slice (self-describing)

    ggml_tensor * find(const std::string & name) const {
        return ggml_get_tensor(meta, name.c_str());
    }

    bool load(const char * path, ggml_backend_t backend) {
        ggml_init_params mp = { /*.mem_size=*/ 0, /*.mem_buffer=*/ nullptr, /*.no_alloc=*/ true };
        gguf_init_params gp = { /*.no_alloc=*/ true, /*.ctx=*/ &meta };
        gguf = gguf_init_from_file(path, gp);
        (void) mp;
        if (!gguf) { std::fprintf(stderr, "failed to open gguf %s\n", path); return false; }
        expert_begin    = meta_i32(gguf, "linkcpp.expert_shard.expert_begin", 0);
        expert_end      = meta_i32(gguf, "linkcpp.expert_shard.expert_end", 0);
        n_expert_global = meta_i32(gguf, "linkcpp.expert_shard.n_expert_global", 0);
        n_embd          = meta_i32(gguf, "linkcpp.expert_shard.n_embd", 0);
        n_layer         = meta_i32(gguf, "linkcpp.expert_shard.n_layer", 0);

        buffer = ggml_backend_alloc_ctx_tensors(meta, backend);
        if (!buffer) { std::fprintf(stderr, "backend buffer alloc failed\n"); return false; }

        FILE * f = std::fopen(path, "rb");
        if (!f) return false;
        const size_t data_off = gguf_get_data_offset(gguf);
        const int n = gguf_get_n_tensors(gguf);
        std::vector<uint8_t> tmp;
        for (int i = 0; i < n; ++i) {
            const char * tname = gguf_get_tensor_name(gguf, i);
            ggml_tensor * t = ggml_get_tensor(meta, tname);
            const size_t sz  = ggml_nbytes(t);
            const size_t off = data_off + gguf_get_tensor_offset(gguf, i);
            tmp.resize(sz);
            std::fseek(f, (long) off, SEEK_SET);
            if (std::fread(tmp.data(), 1, sz, f) != sz) { std::fclose(f); return false; }
            ggml_backend_tensor_set(t, tmp.data(), 0, sz);
        }
        std::fclose(f);
        return true;
    }

    ~expert_shard() {
        if (buffer) ggml_backend_buffer_free(buffer);
        if (meta)   ggml_free(meta);
        if (gguf)   gguf_free(gguf);
    }
};

// One expert-FFN evaluation for `layer`: (hidden[n_embd,n_used,n_tokens],
// ids[n_used,n_tokens] local) -> out[n_embd,n_used,n_tokens].
bool run_ffn(const expert_shard & shard, ggml_backend_t backend, int layer,
             int n_embd, int n_ff, int n_used, int n_tokens,
             const float * hidden, const int32_t * ids, std::vector<float> & out) {
    const std::string p = "blk." + std::to_string(layer) + ".ffn_";
    ggml_tensor * up_exps   = shard.find(p + "up_exps.weight");
    ggml_tensor * gate_exps = shard.find(p + "gate_exps.weight");
    ggml_tensor * down_exps = shard.find(p + "down_exps.weight");
    if (!up_exps || !gate_exps || !down_exps) {
        std::fprintf(stderr, "layer %d expert tensors missing in shard\n", layer);
        return false;
    }

    ggml_init_params cp = { /*.mem_size=*/ ggml_tensor_overhead() * 32 + ggml_graph_overhead(),
                            /*.mem_buffer=*/ nullptr, /*.no_alloc=*/ true };
    ggml_context * ctx = ggml_init(cp);

    ggml_tensor * h  = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, n_embd, n_used, n_tokens);
    ggml_tensor * id = ggml_new_tensor_2d(ctx, GGML_TYPE_I32, n_used, n_tokens);
    ggml_set_name(h, "hidden");
    ggml_set_name(id, "ids");
    ggml_set_input(h);
    ggml_set_input(id);

    ggml_tensor * up   = ggml_mul_mat_id(ctx, up_exps,   h,  id);   // [n_ff, n_used, n_tokens]
    ggml_tensor * gate = ggml_mul_mat_id(ctx, gate_exps, h,  id);
    ggml_tensor * act  = ggml_swiglu_split(ctx, gate, up);          // silu(gate) * up
    ggml_tensor * o    = ggml_mul_mat_id(ctx, down_exps, act, id);  // [n_embd, n_used, n_tokens]
    ggml_set_name(o, "out");
    ggml_set_output(o);

    ggml_cgraph * gf = ggml_new_graph(ctx);
    ggml_build_forward_expand(gf, o);

    ggml_gallocr_t alloc = ggml_gallocr_new(ggml_backend_get_default_buffer_type(backend));
    if (!ggml_gallocr_alloc_graph(alloc, gf)) {
        std::fprintf(stderr, "graph alloc failed\n");
        ggml_gallocr_free(alloc); ggml_free(ctx); return false;
    }
    ggml_backend_tensor_set(h,  hidden, 0, (size_t) n_embd * n_used * n_tokens * sizeof(float));
    ggml_backend_tensor_set(id, ids,    0, (size_t) n_used * n_tokens * sizeof(int32_t));

    if (ggml_backend_graph_compute(backend, gf) != GGML_STATUS_SUCCESS) {
        std::fprintf(stderr, "graph compute failed\n");
        ggml_gallocr_free(alloc); ggml_free(ctx); return false;
    }
    out.resize((size_t) n_embd * n_used * n_tokens);
    ggml_backend_tensor_get(o, out.data(), 0, out.size() * sizeof(float));

    ggml_gallocr_free(alloc);
    ggml_free(ctx);
    return true;
}

// ---- serving mode: dispatch experts over TCP (M2 productionization) --------
bool send_all(int fd, const void * p, size_t n) {
    const char * c = (const char *) p;
    while (n) { ssize_t k = ::send(fd, c, n, 0); if (k <= 0) return false; c += k; n -= (size_t) k; }
    return true;
}
bool recv_all(int fd, void * p, size_t n) {
    char * c = (char *) p;
    while (n) { ssize_t k = ::recv(fd, c, n, 0); if (k <= 0) return false; c += k; n -= (size_t) k; }
    return true;
}

// cur [n_embd,1,n_tokens], sel [n_used,n_tokens] -> experts [n_embd,n_used,n_tokens]
bool compute_dispatch(const expert_shard & shard, ggml_backend_t backend, int layer,
                      int n_embd, int n_used, int n_tokens,
                      const float * cur, const int32_t * sel, std::vector<float> & out) {
    const std::string p = "blk." + std::to_string(layer) + ".ffn_";
    ggml_tensor * up_exps   = shard.find(p + "up_exps.weight");
    ggml_tensor * gate_exps = shard.find(p + "gate_exps.weight");
    ggml_tensor * down_exps = shard.find(p + "down_exps.weight");
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
    ggml_gallocr_t alloc = ggml_gallocr_new(ggml_backend_get_default_buffer_type(backend));
    bool ok = ggml_gallocr_alloc_graph(alloc, gf);
    if (ok) {
        ggml_backend_tensor_set(h,  cur, 0, (size_t) n_embd * n_tokens * sizeof(float));
        ggml_backend_tensor_set(id, sel, 0, (size_t) n_used * n_tokens * sizeof(int32_t));
        ok = ggml_backend_graph_compute(backend, gf) == GGML_STATUS_SUCCESS;
    }
    if (ok) {
        out.resize((size_t) n_embd * n_used * n_tokens);
        ggml_backend_tensor_get(o, out.data(), 0, out.size() * sizeof(float));
    }
    ggml_gallocr_free(alloc);
    ggml_free(ctx);
    return ok;
}

// One connection's request loop. `compute_mu` serializes the actual GPU work —
// the ggml backend is not safe for concurrent graph compute — while I/O (recv of
// the next request, send of the last result) runs off-lock. Returning ends the
// connection; a dead/half-open peer errors out here without wedging the worker.
void handle_conn(int c, const expert_shard & shard, ggml_backend_t backend,
                 int layer, int n_embd, std::mutex & compute_mu) {
    int one2 = 1; ::setsockopt(c, IPPROTO_TCP, TCP_NODELAY, &one2, sizeof(one2));
    for (;;) {
        int32_t hdr[2];
        if (!recv_all(c, hdr, sizeof(hdr))) break;
        if (hdr[0] == -3) {
            // v2f16 wire: same framing as v2 but rows/probs/out are F16 (P1-1).
            const int n_rows = hdr[1];
            int32_t n_pairs = 0;
            if (!recv_all(c, &n_pairs, sizeof(n_pairs))) break;
            std::vector<ggml_fp16_t> rows16((size_t) n_embd * n_rows), probs16((size_t) n_pairs);
            std::vector<int32_t> ridx((size_t) n_pairs), ids((size_t) n_pairs);
            if (!recv_all(c, rows16.data(), rows16.size() * sizeof(ggml_fp16_t))) break;
            if (!recv_all(c, ridx.data(),  ridx.size()  * sizeof(int32_t)))       break;
            if (!recv_all(c, ids.data(),   ids.size()   * sizeof(int32_t)))       break;
            if (!recv_all(c, probs16.data(), probs16.size() * sizeof(ggml_fp16_t))) break;
            std::vector<float> rows((size_t) n_embd * n_rows), probs((size_t) n_pairs);
            ggml_fp16_to_fp32_row(rows16.data(),  rows.data(),  (int64_t) n_embd * n_rows);
            ggml_fp16_to_fp32_row(probs16.data(), probs.data(), (int64_t) n_pairs);
            std::vector<float> h((size_t) n_embd * n_pairs);
            for (int i = 0; i < n_pairs; ++i)
                std::memcpy(h.data() + (size_t) i * n_embd,
                            rows.data() + (size_t) ridx[i] * n_embd,
                            (size_t) n_embd * sizeof(float));
            std::vector<float> e;
            { std::lock_guard<std::mutex> lk(compute_mu);
              if (!compute_dispatch(shard, backend, layer, n_embd, 1, n_pairs, h.data(), ids.data(), e)) break; }
            std::vector<float> out((size_t) n_embd * n_rows, 0.0f);
            for (int i = 0; i < n_pairs; ++i) {
                float *       dst = out.data() + (size_t) ridx[i] * n_embd;
                const float * src = e.data() + (size_t) i * n_embd;
                const float   p   = probs[i];
                for (int k = 0; k < n_embd; ++k) dst[k] += p * src[k];
            }
            std::vector<ggml_fp16_t> out16((size_t) n_embd * n_rows);
            ggml_fp32_to_fp16_row(out.data(), out16.data(), (int64_t) n_embd * n_rows);
            if (!send_all(c, out16.data(), out16.size() * sizeof(ggml_fp16_t))) break;
            continue;
        }
        if (hdr[0] == -2) {
            // v2 wire: row-dedup dispatch + weighted partial-sum combine.
            // {-2, n_rows} + i32 n_pairs + rows f32[n_embd*n_rows]
            // + row_idx i32[n_pairs] + ids i32[n_pairs] + probs f32[n_pairs]
            // -> f32[n_embd*n_rows] per-row sums of probs*expert.
            const int n_rows = hdr[1];
            int32_t n_pairs = 0;
            if (!recv_all(c, &n_pairs, sizeof(n_pairs))) break;
            std::vector<float>   rows((size_t) n_embd * n_rows);
            std::vector<int32_t> ridx((size_t) n_pairs);
            std::vector<int32_t> ids((size_t) n_pairs);
            std::vector<float>   probs((size_t) n_pairs);
            if (!recv_all(c, rows.data(),  rows.size()  * sizeof(float)))   break;
            if (!recv_all(c, ridx.data(),  ridx.size()  * sizeof(int32_t))) break;
            if (!recv_all(c, ids.data(),   ids.size()   * sizeof(int32_t))) break;
            if (!recv_all(c, probs.data(), probs.size() * sizeof(float)))   break;
            std::vector<float> h((size_t) n_embd * n_pairs);
            for (int i = 0; i < n_pairs; ++i)
                std::memcpy(h.data() + (size_t) i * n_embd,
                            rows.data() + (size_t) ridx[i] * n_embd,
                            (size_t) n_embd * sizeof(float));
            std::vector<float> e;
            { std::lock_guard<std::mutex> lk(compute_mu);
              if (!compute_dispatch(shard, backend, layer, n_embd, 1, n_pairs, h.data(), ids.data(), e)) break; }
            std::vector<float> out((size_t) n_embd * n_rows, 0.0f);
            for (int i = 0; i < n_pairs; ++i) {
                float *       dst = out.data() + (size_t) ridx[i] * n_embd;
                const float * src = e.data() + (size_t) i * n_embd;
                const float   p   = probs[i];
                for (int k = 0; k < n_embd; ++k) dst[k] += p * src[k];
            }
            if (!send_all(c, out.data(), out.size() * sizeof(float))) break;
            continue;
        }
        const int n_used = hdr[0], n_tokens = hdr[1];
        std::vector<float>   cur((size_t) n_embd * n_tokens);
        std::vector<int32_t> sel((size_t) n_used * n_tokens);
        if (!recv_all(c, cur.data(), cur.size() * sizeof(float)))   break;
        if (!recv_all(c, sel.data(), sel.size() * sizeof(int32_t))) break;
        std::vector<float> out;
        { std::lock_guard<std::mutex> lk(compute_mu);
          if (!compute_dispatch(shard, backend, layer, n_embd, n_used, n_tokens, cur.data(), sel.data(), out)) break; }
        if (!send_all(c, out.data(), out.size() * sizeof(float))) break;
    }
    ::close(c);
}

// Wire protocol per request: [int32 n_used, int32 n_tokens] + cur f32[n_embd*n_tokens]
// + sel i32[n_used*n_tokens] -> experts f32[n_embd*n_used*n_tokens]. Layer + n_embd fixed
// at load. A long-lived connection carries many requests (the ring/443 relay tunnels it);
// each connection gets its own thread so a stalled/dead peer can never wedge the accept
// loop and a restarted coordinator connects immediately (a real hang seen under --parallel).
int serve_loop(const expert_shard & shard, ggml_backend_t backend, int layer, int n_embd, int port) {
    int srv = ::socket(AF_INET, SOCK_STREAM, 0);
    int one = 1; ::setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    sockaddr_in addr {}; addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK); addr.sin_port = htons((uint16_t) port);
    if (::bind(srv, (sockaddr *) &addr, sizeof(addr)) || ::listen(srv, 8)) {
        std::perror("bind/listen"); return 1;
    }
    std::fprintf(stderr, "expert worker serving layer %d on 127.0.0.1:%d\n", layer, port);
    static std::mutex compute_mu;
    for (;;) {
        int c = ::accept(srv, nullptr, nullptr);
        if (c < 0) continue;
        std::thread(handle_conn, c, std::cref(shard), backend, layer, n_embd,
                    std::ref(compute_mu)).detach();
    }
    return 0;
}

// ---- throughput bench (M4): batching amortizes per-dispatch overhead --------
void bench_run(const expert_shard & shard, ggml_backend_t backend, int layer, int n_embd, int n_used) {
    const int batches[] = {1, 16, 64, 256, 512};
    std::printf("=== expert-FFN throughput (layer %d, n_used=%d, backend=%s) ===\n",
                layer, n_used, ggml_backend_name(backend));
    std::printf("  %-8s %12s %12s %12s\n", "batch", "ms/call", "tok/s", "ms/tok");
    for (int B : batches) {
        std::vector<float>   cur((size_t) n_embd * B);
        std::vector<int32_t> sel((size_t) n_used * B);
        for (size_t i = 0; i < cur.size(); ++i) cur[i] = 0.05f * (float) ((int) (i % 97) - 48);
        for (size_t i = 0; i < sel.size(); ++i) sel[i] = (int32_t) (i % 256);
        std::vector<float> out;
        compute_dispatch(shard, backend, layer, n_embd, n_used, B, cur.data(), sel.data(), out); // warmup
        const int iters = B >= 256 ? 5 : 20;
        auto t0 = std::chrono::steady_clock::now();
        for (int it = 0; it < iters; ++it)
            compute_dispatch(shard, backend, layer, n_embd, n_used, B, cur.data(), sel.data(), out);
        auto t1 = std::chrono::steady_clock::now();
        const double ms = std::chrono::duration<double, std::milli>(t1 - t0).count() / iters;
        std::printf("  %-8d %12.3f %12.0f %12.4f\n", B, ms, B / (ms / 1000.0), ms / B);
    }
}

} // namespace

#include "expert_api.h"

// In-process serve entry for mobile — iOS cannot exec a bundled binary, so the
// app links the linkcpp-expert static library and calls this from a worker
// thread. Mirrors main's --serve branch: pick a backend, load the slice, run
// the blocking serve loop. n_embd is supplied by the caller (per model).
extern "C" int linkcpp_expert_run(const char * model_path, int port, int layer, int n_embd) {
    ggml_backend_t backend = nullptr;
    for (size_t i = 0; i < ggml_backend_dev_count(); i++) {
        ggml_backend_dev_t dev = ggml_backend_dev_get(i);
        if (ggml_backend_dev_type(dev) != GGML_BACKEND_DEVICE_TYPE_CPU) {
            backend = ggml_backend_dev_init(dev, nullptr);
            if (backend) break;
        }
    }
    if (!backend) backend = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_CPU, nullptr);
    if (!backend) { std::fprintf(stderr, "no ggml backend\n"); return 1; }
    std::fprintf(stderr, "backend: %s\n", ggml_backend_name(backend));
    expert_shard shard;
    if (!shard.load(model_path, backend)) { ggml_backend_free(backend); return 1; }
    std::fprintf(stderr, "loaded shard experts [%d,%d) of %d global\n",
                 shard.expert_begin, shard.expert_end, shard.n_expert_global);
    if (n_embd <= 0) n_embd = shard.n_embd;   // self-describing slice; caller may pass 0
    int rc = serve_loop(shard, backend, layer, n_embd, port);
    ggml_backend_free(backend);
    return rc;
}

#ifndef LINKCPP_EXPERT_NO_MAIN
int main(int argc, char ** argv) {
    const char * model = arg_value(argc, argv, "--model");
    if (!model) {
        std::fprintf(stderr,
            "usage: linkcpp-expert-worker --model <expert-slice.gguf> --self-test \\\n"
            "         --layer N --n-embd E --n-ff F --tokens T \\\n"
            "         --hidden h.bin --ids ids.bin --out out.bin\n");
        return 2;
    }

    const bool force_cpu = arg_value(argc, argv, "--cpu")
        || (argc > 1 && std::string(argv[argc - 1]) == "--cpu");
    // Pick the first non-CPU device (any accelerator). Integrated GPUs — e.g. the
    // NVIDIA GB10 Grace-Blackwell iGPU — register as device type ACCEL rather than
    // GPU, so ggml_backend_init_by_type(GPU) misses them and silently falls back to
    // CPU. Selecting any non-CPU device is robust across discrete and integrated parts
    // (verified on GB10 sm_121a: CUDA0 backend, cosine 1.0 vs ROCm gfx90a).
    ggml_backend_t backend = nullptr;
    if (!force_cpu) {
        for (size_t i = 0; i < ggml_backend_dev_count(); i++) {
            ggml_backend_dev_t dev = ggml_backend_dev_get(i);
            if (ggml_backend_dev_type(dev) != GGML_BACKEND_DEVICE_TYPE_CPU) {
                backend = ggml_backend_dev_init(dev, nullptr);
                if (backend) break;
            }
        }
    }
    if (!backend) backend = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_CPU, nullptr);
    if (!backend) { std::fprintf(stderr, "no ggml backend\n"); return 1; }
    std::fprintf(stderr, "backend: %s\n", ggml_backend_name(backend));

    expert_shard shard;
    if (!shard.load(model, backend)) { ggml_backend_free(backend); return 1; }
    std::fprintf(stderr, "loaded shard experts [%d,%d) of %d global\n",
                 shard.expert_begin, shard.expert_end, shard.n_expert_global);

    if (const char * sp = arg_value(argc, argv, "--serve")) {
        const int layer  = std::atoi(arg_value(argc, argv, "--layer")  ? arg_value(argc, argv, "--layer")  : "0");
        int n_embd = std::atoi(arg_value(argc, argv, "--n-embd") ? arg_value(argc, argv, "--n-embd") : "0");
        if (n_embd <= 0) n_embd = shard.n_embd;   // self-describing slice; --n-embd optional
        int rc = serve_loop(shard, backend, layer, n_embd, std::atoi(sp));
        ggml_backend_free(backend);
        return rc;
    }

    if (arg_value(argc, argv, "--bench")
        || (argc > 1 && std::string(argv[argc - 1]) == "--bench")) {
        const int layer  = std::atoi(arg_value(argc, argv, "--layer")  ? arg_value(argc, argv, "--layer")  : "0");
        const int n_embd = std::atoi(arg_value(argc, argv, "--n-embd") ? arg_value(argc, argv, "--n-embd") : "0");
        const int n_used = std::atoi(arg_value(argc, argv, "--n-used") ? arg_value(argc, argv, "--n-used") : "8");
        bench_run(shard, backend, layer, n_embd, n_used);
        ggml_backend_free(backend);
        return 0;
    }

    if (arg_value(argc, argv, "--self-test")
        || (argc > 1 && std::string(argv[argc - 1]) == "--self-test")
        || arg_value(argc, argv, "--hidden")) {
        const int layer    = std::atoi(arg_value(argc, argv, "--layer")  ? arg_value(argc, argv, "--layer")  : "0");
        const int n_embd   = std::atoi(arg_value(argc, argv, "--n-embd") ? arg_value(argc, argv, "--n-embd") : "0");
        const int n_ff     = std::atoi(arg_value(argc, argv, "--n-ff")   ? arg_value(argc, argv, "--n-ff")   : "0");
        const int n_tokens = std::atoi(arg_value(argc, argv, "--tokens") ? arg_value(argc, argv, "--tokens") : "0");
        const int n_used   = 1; // one local expert per (token,slot) in the harness
        const char * hp = arg_value(argc, argv, "--hidden");
        const char * ip = arg_value(argc, argv, "--ids");
        const char * op = arg_value(argc, argv, "--out");
        if (!n_embd || !n_ff || !n_tokens || !hp || !ip || !op) {
            std::fprintf(stderr, "self-test needs --n-embd --n-ff --tokens --hidden --ids --out\n");
            ggml_backend_free(backend); return 2;
        }
        std::vector<uint8_t> hbuf = read_file(hp, (size_t) n_embd * n_used * n_tokens * sizeof(float));
        std::vector<uint8_t> ibuf = read_file(ip, (size_t) n_used * n_tokens * sizeof(int32_t));
        if (hbuf.empty() || ibuf.empty()) { ggml_backend_free(backend); return 1; }
        std::vector<float> out;
        if (!run_ffn(shard, backend, layer, n_embd, n_ff, n_used, n_tokens,
                     (const float *) hbuf.data(), (const int32_t *) ibuf.data(), out)) {
            ggml_backend_free(backend); return 1;
        }
        FILE * f = std::fopen(op, "wb");
        if (!f) { std::fprintf(stderr, "cannot write %s\n", op); ggml_backend_free(backend); return 1; }
        std::fwrite(out.data(), sizeof(float), out.size(), f);
        std::fclose(f);
        std::fprintf(stderr, "wrote %zu floats to %s\n", out.size(), op);
    }

    ggml_backend_free(backend);
    return 0;
}
#endif // LINKCPP_EXPERT_NO_MAIN
