#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#endif

#include "server.hpp"
#include "server/plan.hpp"
#include "server/startup_plan.hpp"

#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <cstdio>
#include <filesystem>
#include <iostream>
#include <limits>
#include <memory>
#include <string_view>
#include <string>
#include <thread>
#include <utility>
#include <vector>

#ifdef P4_STAGED_WITH_LLAMA
#include "arg.h"
#include "llama_stage_runtime.hpp"
#endif

#ifdef _WIN32
#include <io.h>
#include <fcntl.h>
#else
#include <unistd.h>
#endif

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
using socket_type = SOCKET;
constexpr socket_type invalid_socket = INVALID_SOCKET;
#else
#include <arpa/inet.h>
#include <cerrno>
#include <netinet/in.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <unistd.h>
using socket_type = int;
constexpr socket_type invalid_socket = -1;
#endif

namespace {

constexpr std::size_t kHeaderBytes = staged::protocol::kHeaderBytes;
constexpr std::size_t kMaxPlanBytes = 16U * 1024U * 1024U;

void close_socket(socket_type socket) {
    if (socket == invalid_socket) return;
#ifdef _WIN32
    closesocket(socket);
#else
    close(socket);
#endif
}

void close_stdin() {
#ifdef _WIN32
    _close(_fileno(stdin));
#else
    close(STDIN_FILENO);
#endif
}

struct Socket final {
    socket_type value = invalid_socket;
    ~Socket() { close_socket(value); }
    Socket() = default;
    explicit Socket(socket_type value_in) : value(value_in) {}
    Socket(const Socket &) = delete;
    Socket & operator=(const Socket &) = delete;
    Socket(Socket && other) noexcept : value(other.value) {
        other.value = invalid_socket;
    }
    Socket & operator=(Socket && other) noexcept {
        if (this != &other) {
            close_socket(value);
            value = other.value;
            other.value = invalid_socket;
        }
        return *this;
    }
};

bool wait_readable(socket_type socket, std::atomic<bool> &parent_alive) {
    while (parent_alive.load()) {
        fd_set readable;
        FD_ZERO(&readable);
        FD_SET(socket, &readable);
        timeval timeout{0, 200000};
        const auto result = select(static_cast<int>(socket) + 1, &readable,
                                   nullptr, nullptr, &timeout);
        if (result > 0) return true;
        if (result < 0) return false;
    }
    return false;
}

bool receive_all(socket_type socket, std::uint8_t *destination, std::size_t size,
                 std::atomic<bool> &parent_alive) {
    std::size_t offset = 0;
    while (offset < size && parent_alive.load()) {
        if (!wait_readable(socket, parent_alive)) return false;
        const auto count = recv(socket, reinterpret_cast<char *>(destination + offset),
                                static_cast<int>(size - offset), 0);
        if (count <= 0) return false;
        offset += static_cast<std::size_t>(count);
    }
    return offset == size;
}

bool send_all(socket_type socket, const std::vector<std::uint8_t> &bytes) {
    std::size_t offset = 0;
    while (offset < bytes.size()) {
        const auto count = send(socket,
                                reinterpret_cast<const char *>(bytes.data() + offset),
                                static_cast<int>(bytes.size() - offset), 0);
        if (count <= 0) return false;
        offset += static_cast<std::size_t>(count);
    }
    return true;
}

bool receive_frame(socket_type socket, staged::protocol::Frame &frame,
                   std::atomic<bool> &parent_alive) {
    std::vector<std::uint8_t> header(kHeaderBytes);
    if (!receive_all(socket, header.data(), header.size(), parent_alive)) return false;
    const auto body_bytes = staged::server::read_u32_le(header.data() + 8);
    if (body_bytes > staged::protocol::ProtocolLimits{}.max_frame_bytes - kHeaderBytes) {
        return false;
    }
    header.resize(kHeaderBytes + body_bytes);
    if (body_bytes != 0 &&
        !receive_all(socket, header.data() + kHeaderBytes, body_bytes, parent_alive)) {
        return false;
    }
    try {
        frame = staged::protocol::Frame::decode(
            header, staged::protocol::ProtocolLimits{});
        return true;
    } catch (const staged::protocol::ProtocolError &exception) {
        std::cerr << "FRAME_DECODE_ERROR detail=" << exception.what() << '\n';
        return false;
    }
}

socket_type listen_socket(const std::string &bind_address, std::uint16_t port) {
    const auto socket = ::socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (socket == invalid_socket) return invalid_socket;
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_port = htons(port);
    if (inet_pton(AF_INET, bind_address.c_str(), &address.sin_addr) != 1 ||
        bind(socket, reinterpret_cast<const sockaddr *>(&address), sizeof(address)) != 0 ||
        listen(socket, 1) != 0) {
        close_socket(socket);
        return invalid_socket;
    }
    return socket;
}

std::uint16_t option_port(int argc, char **argv) {
    for (int i = 1; i + 1 < argc; ++i) {
        if (std::string(argv[i]) == "--port") {
            const auto value = std::strtoul(argv[i + 1], nullptr, 10);
            if (value <= 65535U) {
                return static_cast<std::uint16_t>(value);
            }
        }
    }
    return 0;
}

std::string option_bind(int argc, char **argv) {
    for (int i = 1; i + 1 < argc; ++i) {
        if (std::string(argv[i]) == "--bind") return argv[i + 1];
    }
    return "127.0.0.1";
}


} // namespace

int main(int argc, char **argv) {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    WSADATA wsa_data{};
    if (WSAStartup(MAKEWORD(2, 2), &wsa_data) != 0) return 2;
#endif
    const auto port = option_port(argc, argv);
    if (port == 0) {
        std::cerr << "usage: p4_staged_server --port <port> [--bind <ipv4>]\n";
#ifdef _WIN32
        WSACleanup();
#endif
        return 2;
    }

    std::uint8_t length_bytes[4]{};
    if (!staged::server::read_stdin_exact(length_bytes, sizeof(length_bytes))) {
        std::cerr << "startup plan prefix is incomplete\n";
        return 3;
    }
    const auto plan_size = static_cast<std::size_t>(
        staged::server::read_u32_le(length_bytes));
    if (plan_size > kMaxPlanBytes) {
        std::cerr << "startup plan is too large\n";
        return 3;
    }
    std::vector<std::uint8_t> plan(plan_size);
    if (!staged::server::read_stdin_exact(plan.data(), plan.size())) {
        std::cerr << "startup plan is incomplete\n";
        return 3;
    }

#ifdef P4_STAGED_WITH_LLAMA
    std::vector<std::string> plan_tokens;
    std::string plan_parse_error;
    const std::string plan_text(reinterpret_cast<const char *>(plan.data()), plan.size());
    if (!staged::server::parse_plan_tokens(plan_text, &plan_tokens, &plan_parse_error)) {
        std::cerr << "startup plan tokenization failed: " << plan_parse_error << '\n';
        return 3;
    }
    staged::server::ParsedLlamaOptions parsed_options;
    if (!staged::server::parse_llama_options(argc, argv, plan_tokens, &parsed_options,
                             &plan_parse_error)) {
        std::cerr << "startup plan parsing failed: " << plan_parse_error << '\n';
        return 3;
    }
    std::cerr << "PLAN_APPLIED n_parallel=" << parsed_options.params.n_parallel()
              << " model=" << parsed_options.model_path << '\n';
    const auto option_capabilities = staged::server::capability_report(parsed_options);
    std::cerr << "CAPABILITY_REPORT " << option_capabilities.serialize() << '\n';
    if (parsed_options.validate_plan) return 0;
    staged::llama_runtime::LoadConfig parsed_load_config;
    parsed_load_config.model_path = parsed_options.model_path;
    parsed_load_config.layer_begin = parsed_options.layer_begin;
    parsed_load_config.layer_end = parsed_options.layer_end;
    parsed_load_config.kv_gpu_layer_start = parsed_options.kv_layer_begin;
    parsed_load_config.kv_gpu_layer_end = parsed_options.kv_layer_end;
    parsed_load_config.kv_root = parsed_options.kv_root;
    parsed_load_config.model_identity = parsed_options.model_identity;
    parsed_load_config.memory_topology = parsed_options.memory_topology;
    if (parsed_options.inspect_memory_plan) {
        staged::llama_runtime::StageMemoryPlan memory_plan;
        std::string memory_error;
        if (!staged::llama_runtime::inspect_stage_memory(
                parsed_options.params, parsed_load_config, &memory_plan, &memory_error)) {
            std::cerr << memory_error << '\n';
            return 7;
        }
        std::cerr << "MEMORY_PLAN "
                  << staged::llama_runtime::serialize_stage_memory_plan(memory_plan)
                  << '\n';
        return memory_plan.complete && memory_plan.fits_current_free ? 0 : 7;
    }
    if (parsed_options.speculative_requested
        && !option_capabilities.speculative_execution) {
        std::cerr << "CAPABILITY_UNAVAILABLE: "
                  << option_capabilities.execution_blocker << '\n';
        return 6;
    }
    std::unique_ptr<staged::llama_runtime::StageRuntime> stage_runtime;
#endif
    staged::server::Capabilities capabilities{};
    std::filesystem::path transaction_root;
#ifdef P4_STAGED_WITH_LLAMA
    capabilities.llama_options = option_capabilities;
#endif
#ifdef P4_STAGED_WITH_LLAMA
    if (!parsed_options.model_path.empty()) {
        auto load_config = parsed_load_config;
        transaction_root = load_config.kv_root;
        auto runtime = std::make_unique<staged::llama_runtime::StageRuntime>();
        std::string load_error;
        if (!runtime->load(std::move(parsed_options.params), load_config, &load_error)) {
            std::cerr << load_error << '\n';
            return 5;
        }
        capabilities.llama_runtime = true;
        capabilities.hop = true;
        capabilities.kv = !load_config.kv_root.empty();
        stage_runtime = std::move(runtime);
    }
#endif
    staged::server::Session session(capabilities
#ifdef P4_STAGED_WITH_LLAMA
                                    , stage_runtime.get()
#endif
                                    , {}
                                    , transaction_root
                                    );
    std::vector<std::uint8_t> framed_plan;
    framed_plan.reserve(sizeof(length_bytes) + plan.size());
    framed_plan.insert(framed_plan.end(), length_bytes,
                       length_bytes + sizeof(length_bytes));
    framed_plan.insert(framed_plan.end(), plan.begin(), plan.end());
    std::string plan_error;
    if (!session.feed_plan(framed_plan, &plan_error)) {
        std::cerr << plan_error << '\n';
        return 3;
    }

    std::atomic<bool> parent_alive{true};
    std::thread stdin_liveness([&parent_alive] {
        char buffer[4096];
        while (std::cin.read(buffer, sizeof(buffer)) || std::cin.gcount() != 0) {}
        parent_alive.store(false);
    });

    Socket listener(listen_socket(option_bind(argc, argv), port));
    if (listener.value == invalid_socket) {
        parent_alive.store(false);
        stdin_liveness.join();
        return 4;
    }
    std::cerr << "READY port=" << port << "\n";

    while (parent_alive.load()) {
        if (!wait_readable(listener.value, parent_alive)) break;
        Socket client(accept(listener.value, nullptr, nullptr));
        if (client.value == invalid_socket) continue;
        while (parent_alive.load()) {
            staged::protocol::Frame request;
            if (!receive_frame(client.value, request, parent_alive)) break;
            bool close_after = false;
            staged::protocol::Frame response;
            const bool trace_protocol = std::getenv("P4_STAGED_TRACE_PROTOCOL") != nullptr;
            if (trace_protocol) {
                std::cerr << "P4_STAGED_FRAME_IN operation="
                          << static_cast<int>(request.header.operation) << '\n';
            }
            try {
                response = session.handle(request, &close_after);
            } catch (const std::exception &exception) {
                std::cerr << "SESSION_EXCEPTION operation="
                          << static_cast<int>(request.header.operation)
                          << " detail=" << exception.what() << '\n';
                const std::string detail = std::string("session exception: ")
                    + exception.what();
                response = staged::protocol::Frame::make(
                    staged::protocol::Operation::Error,
                    std::vector<std::uint8_t>(detail.begin(), detail.end()));
            } catch (...) {
                std::cerr << "SESSION_EXCEPTION operation="
                          << static_cast<int>(request.header.operation)
                          << " detail=unknown\n";
                response = staged::protocol::Frame::make(
                    staged::protocol::Operation::Error,
                    std::vector<std::uint8_t>{'u', 'n', 'k', 'n', 'o', 'w', 'n'});
            }
            try {
                if (trace_protocol) {
                    std::cerr << "P4_STAGED_FRAME_OUT operation="
                              << static_cast<int>(response.header.operation) << '\n';
                }
                if (!send_all(client.value,
                              response.encode(staged::protocol::ProtocolLimits{}))) break;
            } catch (const std::exception &exception) {
                std::cerr << "RESPONSE_EXCEPTION operation="
                          << static_cast<int>(response.header.operation)
                          << " detail=" << exception.what() << '\n';
                break;
            } catch (...) {
                std::cerr << "RESPONSE_EXCEPTION operation="
                          << static_cast<int>(response.header.operation)
                          << " detail=unknown\n";
                break;
            }
            if (close_after) {
                parent_alive.store(false);
                break;
            }
        }
    }
    parent_alive.store(false);
    close_stdin();
    stdin_liveness.join();
#ifdef _WIN32
    WSACleanup();
#endif
    return 0;
}
