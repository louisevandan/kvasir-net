// C bridge: run a ggml RPC worker (data plane) inside the iOS app.
// The server loop is ggml's own ggml_backend_rpc_start_server — the same code
// path as the desktop ggml-rpc-server binary, so the hub master drives this
// device exactly like any other RPC worker.
#ifndef KVASIR_RPC_WORKER_H
#define KVASIR_RPC_WORKER_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Starts the RPC server on a detached thread. Idempotent: returns true if the
// server is (already) running. ggml's server has no stop API, so the worker
// lives until the process exits; "stopping" a node is a control-plane state.
bool kvasir_rpc_worker_start(const char *host, int32_t port, uint32_t n_threads);

bool kvasir_rpc_worker_running(void);

// Device the worker serves (Metal on iOS) + its memory, for status reporting.
const char *kvasir_rpc_worker_device_name(void);
uint64_t kvasir_rpc_worker_device_total_mem(void);
uint64_t kvasir_rpc_worker_device_free_mem(void);

#ifdef __cplusplus
}
#endif

#endif
