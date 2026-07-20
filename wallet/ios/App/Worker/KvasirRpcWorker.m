#import "KvasirRpcWorker.h"
#import <Foundation/Foundation.h>

#include "ggml.h"
#include "ggml-backend.h"
#include "ggml-rpc.h"

// Metal first, CPU fallback second — the same device order a macOS Metal
// ggml-rpc-server advertises, which the hub's --device selection assumes.
#define KVASIR_MAX_DEVICES 2

static _Atomic bool g_running = false;
static ggml_backend_dev_t g_devices[KVASIR_MAX_DEVICES];
static size_t g_n_devices = 0;

static void kvasir_collect_devices(void) {
    if (g_n_devices > 0) return;
    ggml_backend_dev_t gpu = NULL, cpu = NULL;
    for (size_t i = 0; i < ggml_backend_dev_count(); i++) {
        ggml_backend_dev_t dev = ggml_backend_dev_get(i);
        switch (ggml_backend_dev_type(dev)) {
            case GGML_BACKEND_DEVICE_TYPE_GPU:
            case GGML_BACKEND_DEVICE_TYPE_IGPU:
                if (!gpu) gpu = dev;
                break;
            case GGML_BACKEND_DEVICE_TYPE_CPU:
                if (!cpu) cpu = dev;
                break;
            default:
                break;  // no ACCEL devices: a 0-MiB BLAS device crashes RPC graph_compute
        }
    }
    if (gpu) g_devices[g_n_devices++] = gpu;
    if (cpu) g_devices[g_n_devices++] = cpu;
}

bool kvasir_rpc_worker_start(const char *host, int32_t port, uint32_t n_threads) {
    if (g_running) return true;
    kvasir_collect_devices();
    if (g_n_devices == 0) return false;

    NSString *endpoint = [NSString stringWithFormat:@"%s:%d", host, port];
    NSString *cache = [NSTemporaryDirectory() stringByAppendingPathComponent:@"kvasir-rpc-cache"];
    [[NSFileManager defaultManager] createDirectoryAtPath:cache withIntermediateDirectories:YES attributes:nil error:nil];

    g_running = true;
    NSThread *thread = [[NSThread alloc] initWithBlock:^{
        // Blocks in the accept loop for the process lifetime (ggml has no stop API).
        ggml_backend_rpc_start_server(endpoint.UTF8String, cache.UTF8String,
                                      n_threads, g_n_devices, g_devices);
        g_running = false;  // only reached if the listen socket fails
    }];
    thread.name = @"kvasir-rpc-worker";
    thread.qualityOfService = NSQualityOfServiceUserInitiated;
    [thread start];
    return true;
}

bool kvasir_rpc_worker_running(void) { return g_running; }

const char *kvasir_rpc_worker_device_name(void) {
    kvasir_collect_devices();
    return g_n_devices ? ggml_backend_dev_description(g_devices[0]) : "unknown";
}

uint64_t kvasir_rpc_worker_device_total_mem(void) {
    kvasir_collect_devices();
    if (!g_n_devices) return 0;
    size_t free_mem = 0, total = 0;
    ggml_backend_dev_memory(g_devices[0], &free_mem, &total);
    return total;
}

uint64_t kvasir_rpc_worker_device_free_mem(void) {
    kvasir_collect_devices();
    if (!g_n_devices) return 0;
    size_t free_mem = 0, total = 0;
    ggml_backend_dev_memory(g_devices[0], &free_mem, &total);
    return free_mem;
}
