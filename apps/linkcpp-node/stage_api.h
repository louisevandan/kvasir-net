// Public C entry for running one linkcpp ring stage in-process.
//
// The desktop linkcpp-node binary wraps this API with a CLI; mobile apps
// (iOS cannot exec bundled binaries) link the linkcpp-stage static library
// and call it from a worker thread instead. The call blocks for the life of
// the stage — the same accept/serve loop the binary runs.
#ifndef LINKCPP_STAGE_API_H
#define LINKCPP_STAGE_API_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct linkcpp_stage_params {
    const char * model_path;     // GGUF path; only [layer_begin, layer_end) is loaded
    int32_t      layer_begin;
    int32_t      layer_end;
    const char * role;           // "first" | "middle" | "last"
    int32_t      listen_port;    // LKS1 ring listener
    const char * next_endpoint;  // "host:port" of the next stage
    const char * dial_prev_endpoint; // NAT: dial the predecessor ("" = accept it)
    bool         accept_next;    // NAT-neighbour: accept the successor (don't dial)
    int32_t      gpu_layers;     // -1 = offload the whole window
    int32_t      ctx;
    int32_t      parallel;
    const char * cache_type_k;   // f16|bf16|q8_0|q5_1|q5_0|q4_1|q4_0|iq4_nl (NULL = f16)
    const char * cache_type_v;
    bool         kv_offload;     // false keeps the rank-local KV cache in host RAM
    bool         embeddings;
    const char * pooling;        // NULL/"" | none|mean|cls|last|rank
} linkcpp_stage_params;

// Runs one ring stage; blocks until the stage exits. Returns the same codes as
// the linkcpp-node CLI (0 ok, 1 runtime failure, 2 bad parameters). Safe to
// call once per process at a time.
int linkcpp_stage_run(const linkcpp_stage_params * params);

// The stage runtime identity as a static JSON string — the same payload the
// binary prints for --runtime-info (protocol, adapter_abi, build_id, ...).
const char * linkcpp_stage_runtime_info_json(void);

#ifdef __cplusplus
}
#endif

#endif
