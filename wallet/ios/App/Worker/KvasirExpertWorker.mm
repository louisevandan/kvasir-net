#import "KvasirExpertWorker.h"

#include <atomic>
#include <pthread.h>
#include <string>

// From liblinkcpp-expert.a (apps/linkcpp-expert-worker/expert_api.h). Declared
// here so this bridge needs no extra header search path — the symbol is linked
// via OTHER_LDFLAGS (-llinkcpp-expert). Blocks for the life of the serve loop.
extern "C" int linkcpp_expert_run(const char *model_path, int port, int layer, int n_embd);

namespace {
std::atomic<bool> g_running{false};

struct ExpertArgs {
    std::string model;
    int port;
    int layer;
    int n_embd;
};

void *expert_thread(void *p) {
    ExpertArgs *a = static_cast<ExpertArgs *>(p);
    linkcpp_expert_run(a->model.c_str(), a->port, a->layer, a->n_embd);  // blocks
    g_running = false;
    delete a;
    return nullptr;
}
}  // namespace

bool kvasir_expert_start(const char *model_path, int32_t port, int32_t layer, int32_t n_embd) {
    if (g_running.load()) return false;
    // Set running synchronously so a caller polling kvasir_expert_running() right
    // after start never sees a false gap before the thread schedules.
    g_running = true;
    ExpertArgs *a = new ExpertArgs{std::string(model_path), (int)port, (int)layer, (int)n_embd};
    pthread_t t;
    if (pthread_create(&t, nullptr, expert_thread, a) != 0) {
        g_running = false;
        delete a;
        return false;
    }
    pthread_detach(t);
    return true;
}

bool kvasir_expert_running(void) { return g_running.load(); }
