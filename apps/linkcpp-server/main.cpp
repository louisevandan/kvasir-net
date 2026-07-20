#include "ring-executor.h"
#include "../linkcpp-ring-protocol.h"
#if defined(LINKCPP_MOE_DISPATCH_AVAILABLE)
#include "../linkcpp-moe-dispatch.h"
#endif

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

int llama_server(int argc, char ** argv);

#if defined(LINKCPP_MOE_DISPATCH_AVAILABLE)
// Expert-dispatch router state — env-configured, inert without
// LINKCPP_MOE_DISPATCH_MAP (see linkcpp-moe-dispatch.h). Static: the callback
// fires from inside llama_decode for the server's whole lifetime.
static linkcpp_moe::dispatch_state g_moe_dispatch;
#endif

int main(int argc, char ** argv) {
    if (argc == 2 && !std::strcmp(argv[1], "--runtime-info")) {
        std::printf("{\"protocol\":\"%s\",\"adapter_abi\":%u,"
                    "\"build_id\":\"%s\",\"state_snapshot\":true,"
                    "\"chunked_state\":true}\n",
                    LINKCPP_RING_PROTOCOL_NAME, LINKCPP_RING_ADAPTER_ABI,
                    LINKCPP_RING_BUILD_ID_STRING);
        return 0;
    }
    linkcpp_ring_options ring;
    std::vector<char *> forwarded;
    forwarded.reserve((size_t) argc);
    forwarded.push_back(argv[0]);
    std::string model_path;
    for (int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];
        auto value = [&](const char * option) -> const char * {
            if (arg == option && i + 1 < argc) return argv[++i];
            return nullptr;
        };
        if (const char * v = value("--linkcpp-layers")) {
            const char * colon = std::strchr(v, ':');
            if (!colon) return 2;
            ring.layer_begin = std::atoi(v);
            ring.layer_end = std::atoi(colon + 1);
        } else if (const char * v = value("--linkcpp-listen")) {
            ring.listen_port = std::atoi(v);
        } else if (const char * v = value("--linkcpp-next")) {
            ring.next_endpoint = v;
        } else if (const char * v = value("--linkcpp-dial-prev")) {
            ring.dial_prev_endpoint = v;
        } else if (arg == "--linkcpp-accept-next") {
            ring.accept_next = true;
        } else {
            forwarded.push_back(argv[i]);
            if ((arg == "-m" || arg == "--model") && i + 1 < argc) {
                model_path = argv[i + 1];
            } else if (arg.rfind("--model=", 0) == 0) {
                model_path = arg.substr(8);
            }
        }
    }
#if defined(LINKCPP_MOE_DISPATCH_AVAILABLE)
    linkcpp_moe::setup_from_env(g_moe_dispatch);
#endif
    if (!ring.enabled()) {
        return llama_server(argc, argv);
    }
    if (model_path.empty() || !ring.valid()) {
        std::fprintf(stderr, "proxy mode requires --model, --linkcpp-layers 0:N, --linkcpp-listen, and --linkcpp-next\n");
        return 2;
    }

    linkcpp_ring_executor executor(ring);
    if (!executor.configure(model_path)) return 1;
    const int rc = llama_server((int) forwarded.size(), forwarded.data());
    llama_linkcpp_runtime_clear();
    return rc;
}
