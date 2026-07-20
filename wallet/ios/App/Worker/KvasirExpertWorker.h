// C bridge: run one MoE expert-shard serve loop (linkcpp-expert static lib)
// inside the app. iOS cannot exec a bundled binary, so — unlike Android, which
// spawns linkcpp-expert-worker — the phone links the library and serves the
// expert dispatch on 127.0.0.1:port from a worker thread.
#ifndef KVASIR_EXPERT_WORKER_H
#define KVASIR_EXPERT_WORKER_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Starts serving the expert slice at model_path on 127.0.0.1:port for `layer`
// in a detached worker thread. Returns false if a worker is already running.
// n_embd is supplied per model (a mini slice does not carry it).
bool kvasir_expert_start(const char *model_path, int32_t port, int32_t layer, int32_t n_embd);

bool kvasir_expert_running(void);

#ifdef __cplusplus
}
#endif

#endif
