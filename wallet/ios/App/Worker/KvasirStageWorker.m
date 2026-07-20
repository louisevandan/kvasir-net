#import "KvasirStageWorker.h"
#import <Foundation/Foundation.h>

#include <stdatomic.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <unistd.h>

#include "stage_api.h"

static _Atomic bool g_stage_running = false;
static _Atomic int32_t g_stage_exit = 0;
static _Atomic int32_t g_stage_port = 0;

bool kvasir_stage_start(const char *model_path, int32_t layer_begin, int32_t layer_end,
                        const char *role, int32_t listen_port, const char *next_endpoint,
                        int32_t gpu_layers, int32_t ctx, int32_t parallel,
                        const char *cache_type_k, const char *cache_type_v,
                        bool kv_offload) {
    if (g_stage_running) return false;
    NSString *model = model_path ? @(model_path) : nil;
    NSString *roleS = role ? @(role) : nil;
    NSString *next = next_endpoint ? @(next_endpoint) : nil;
    NSString *ctk = cache_type_k ? @(cache_type_k) : @"f16";
    NSString *ctv = cache_type_v ? @(cache_type_v) : @"f16";
    if (!model || !roleS || !next) return false;

    g_stage_running = true;
    g_stage_port = listen_port;
    NSThread *thread = [[NSThread alloc] initWithBlock:^{
        linkcpp_stage_params params = {
            .model_path = model.UTF8String,
            .layer_begin = layer_begin, .layer_end = layer_end,
            .role = roleS.UTF8String, .listen_port = listen_port,
            .next_endpoint = next.UTF8String,
            .gpu_layers = gpu_layers, .ctx = ctx, .parallel = parallel,
            .cache_type_k = ctk.UTF8String, .cache_type_v = ctv.UTF8String,
            .kv_offload = kv_offload,
            .embeddings = false, .pooling = NULL,
        };
        const int rc = linkcpp_stage_run(&params);
        g_stage_exit = rc;
        g_stage_running = false;
        g_stage_port = 0;
    }];
    thread.name = @"kvasir-ring-stage";
    thread.qualityOfService = NSQualityOfServiceUserInitiated;
    thread.stackSize = 4 << 20;  // llama graph code is stack-hungry
    [thread start];
    return true;
}

bool kvasir_stage_running(void) { return g_stage_running; }
int32_t kvasir_stage_last_exit(void) { return g_stage_exit; }

bool kvasir_stage_request_stop(void) {
    if (!g_stage_running) return false;
    const int32_t port = g_stage_port;
    if (port <= 0) return true;
    // A stage blocked in accept() exits after a bogus peer: connect and hang up.
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd >= 0) {
        struct sockaddr_in addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_port = htons((uint16_t) port);
        addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        connect(fd, (struct sockaddr *) &addr, sizeof(addr));
        close(fd);
    }
    return true;
}

const char *kvasir_stage_runtime_info_json(void) {
    return linkcpp_stage_runtime_info_json();
}

void kvasir_worker_redirect_stderr(const char *path) {
    if (path && *path) freopen(path, "a", stderr);
    setvbuf(stderr, NULL, _IONBF, 0);
}
