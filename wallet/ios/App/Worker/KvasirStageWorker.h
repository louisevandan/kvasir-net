// C bridge: run one linkcpp ring stage (linkcpp-stage static lib) inside the app.
// Mirrors what controller/proxy/stage_service.py does with the linkcpp-node
// binary on desktop nodes — same wire protocol, same ready marker in the log.
#ifndef KVASIR_STAGE_WORKER_H
#define KVASIR_STAGE_WORKER_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Starts a stage thread. Returns false if a stage is already running or the
// parameters are rejected. The stage ends on its own when the ring tears down.
bool kvasir_stage_start(const char *model_path, int32_t layer_begin, int32_t layer_end,
                        const char *role, int32_t listen_port, const char *next_endpoint,
                        int32_t gpu_layers, int32_t ctx, int32_t parallel,
                        const char *cache_type_k, const char *cache_type_v,
                        bool kv_offload);

bool kvasir_stage_running(void);
int32_t kvasir_stage_last_exit(void);

// Asks a waiting stage to exit by nudging its own listener; returns whether a
// stage was running. An in-session stage exits when the ring closes.
bool kvasir_stage_request_stop(void);

// The stage identity JSON (protocol / adapter_abi / build_id ...).
const char *kvasir_stage_runtime_info_json(void);

// Redirect this process's stderr to a file — llama/ring logs (including the
// "ring stage ready:" marker the hub polls for) become readable by the app.
void kvasir_worker_redirect_stderr(const char *path);

#ifdef __cplusplus
}
#endif

#endif
