#include "ring-executor.h"
#include "../linkcpp-ring-protocol.h"

#ifdef _WIN32
#  define NOMINMAX
#  include <winsock2.h>
#  include <ws2tcpip.h>
#else
#  include <arpa/inet.h>
#  include <netdb.h>
#  include <netinet/in.h>
#  include <netinet/tcp.h>
#  include <sys/socket.h>
#  include <unistd.h>
#endif

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

namespace {
constexpr uint8_t PROTOCOL_VERSION = LINKCPP_RING_WIRE_VERSION;
constexpr uint8_t FRAME_HELLO = 1;
constexpr uint8_t FRAME_ACK = 6;
constexpr uint8_t FRAME_EXECUTE = 9;
constexpr uint8_t FRAME_EXECUTE_RESULT = 10;
constexpr uint8_t FRAME_MEMORY = 11;
constexpr uint8_t FRAME_STATE_GET = 12;
constexpr uint8_t FRAME_STATE_CHUNK = 13;
constexpr uint8_t FRAME_STATE_DONE = 14;
constexpr uint8_t FRAME_STATE_SET_BEGIN = 15;
constexpr uint8_t FRAME_STATE_SET_CHUNK = 16;
constexpr uint8_t FRAME_STATE_SET_COMMIT = 17;
constexpr uint8_t FRAME_STATE_ACK = 18;
constexpr uint8_t BOUNDARY_VERSION = 3;
constexpr uint32_t MAX_FRAME_BYTES = 64u * 1024u * 1024u;
constexpr size_t STATE_CHUNK_BYTES = 4u * 1024u * 1024u;
constexpr uint16_t MAX_BOUNDARY_TENSORS = 1024;

#ifdef _WIN32
using socket_handle = SOCKET;
constexpr socket_handle INVALID_SOCKET_HANDLE = INVALID_SOCKET;
void close_socket(socket_handle fd) { closesocket(fd); }
#else
using socket_handle = int;
constexpr socket_handle INVALID_SOCKET_HANDLE = -1;
void close_socket(socket_handle fd) { close(fd); }
#endif

#pragma pack(push, 1)
struct wire_header {
    char magic[4];
    uint8_t version;
    uint8_t kind;
    uint16_t flags;
    uint64_t request_id;
    uint32_t sequence_id;
    uint32_t position;
    uint32_t bytes;
    uint32_t reserved;
};
struct hello_payload {
    uint32_t n_embd;
    uint32_t layer_begin;
    uint32_t layer_end;
    uint32_t adapter_abi;
    uint64_t build_fingerprint;
};
struct boundary_bundle_header {
    char magic[4];
    uint8_t version;
    uint8_t reserved;
    uint16_t count;
    uint32_t reserved2;
};
struct boundary_desc_wire {
    int32_t type;
    int32_t n_dims;
    int64_t ne[4];
    uint64_t nb[4];
    uint64_t nbytes;
    uint64_t view_offset;
    int32_t alias_of;
    uint32_t flags;
    char name[64];
};
struct execute_bundle_header {
    uint32_t version;
    uint32_t flags;
    uint32_t n_tokens;
    uint32_t n_seq_tokens;
    uint32_t n_seqs;
    uint32_t n_seqs_unq;
    uint32_t n_pos;
    uint32_t total_seq_ids;
    uint32_t boundary_bytes;
    uint32_t reserved;
};
struct memory_op_wire {
    uint32_t version;
    uint32_t op;
    int32_t seq_id_src;
    int32_t seq_id_dst;
    int32_t p0;
    int32_t p1;
    int32_t value;
    uint32_t reserved;
};
struct state_op_wire {
    uint32_t version;
    int32_t seq_id;
    uint32_t flags;
    int32_t layer_begin;
    int32_t layer_end;
    uint32_t status;
    uint64_t total_size;
    uint64_t offset;
};
struct state_bundle_header {
    uint32_t version;
    uint32_t count;
    uint64_t reserved;
};
struct state_bundle_entry {
    int32_t layer_begin;
    int32_t layer_end;
    uint64_t size;
};
#pragma pack(pop)

static_assert(sizeof(wire_header) == 32, "wire header ABI");
static_assert(sizeof(boundary_desc_wire) == 160, "boundary descriptor ABI");
static_assert(sizeof(execute_bundle_header) == 40, "execute bundle ABI");
static_assert(sizeof(memory_op_wire) == 32, "memory operation ABI");
static_assert(sizeof(state_op_wire) == 40, "state operation ABI");
static_assert(sizeof(state_bundle_header) == 16, "state bundle header ABI");
static_assert(sizeof(state_bundle_entry) == 16, "state bundle entry ABI");

struct frame {
    uint8_t kind = 0;
    uint64_t request_id = 0;
    uint32_t sequence_id = 0;
    uint32_t position = 0;
    std::vector<uint8_t> payload;
};
struct boundary_tensor {
    llama_linkcpp_tensor_desc desc{};
    std::vector<uint8_t> data;
};

struct remote_state_entry {
    int32_t layer_begin = -1;
    int32_t layer_end = -1;
    uint64_t total_size = 0;
    std::vector<uint8_t> data;
};
using boundary_bundle = std::vector<boundary_tensor>;

bool write_all(socket_handle fd, const void * data, size_t size) {
    const auto * p = static_cast<const uint8_t *>(data);
    while (size) {
#ifdef _WIN32
        const int n = send(fd, reinterpret_cast<const char *>(p), (int) size, 0);
#else
        const ssize_t n = send(fd, p, size, MSG_NOSIGNAL);
#endif
        if (n <= 0) return false;
        p += n;
        size -= (size_t) n;
    }
    return true;
}

bool read_all(socket_handle fd, void * data, size_t size) {
    auto * p = static_cast<uint8_t *>(data);
    while (size) {
#ifdef _WIN32
        const int n = recv(fd, reinterpret_cast<char *>(p), (int) size, 0);
#else
        const ssize_t n = recv(fd, p, size, 0);
#endif
        if (n <= 0) return false;
        p += n;
        size -= (size_t) n;
    }
    return true;
}

bool send_frame(socket_handle fd, uint8_t kind, uint64_t request_id,
                const void * data, uint32_t bytes, uint32_t sequence_id = 0) {
    wire_header h = {{'L','K','S','1'}, PROTOCOL_VERSION, kind, 0, request_id,
                     sequence_id, 0, bytes, 0};
    return write_all(fd, &h, sizeof(h)) && (!bytes || write_all(fd, data, bytes));
}

bool recv_frame(socket_handle fd, frame * result) {
    wire_header h{};
    if (!read_all(fd, &h, sizeof(h)) || memcmp(h.magic, "LKS1", 4) != 0
        || h.version != PROTOCOL_VERSION || h.flags || h.reserved
        || h.bytes > MAX_FRAME_BYTES) return false;
    result->kind = h.kind;
    result->request_id = h.request_id;
    result->sequence_id = h.sequence_id;
    result->position = h.position;
    result->payload.resize(h.bytes);
    return !h.bytes || read_all(fd, result->payload.data(), h.bytes);
}

socket_handle listen_on(int port) {
    const socket_handle fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd == INVALID_SOCKET_HANDLE) return fd;
    int yes = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, reinterpret_cast<const char *>(&yes), sizeof(yes));
    sockaddr_in addr{};
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_ANY);
    addr.sin_port = htons((uint16_t) port);
    if (bind(fd, reinterpret_cast<sockaddr *>(&addr), sizeof(addr)) || listen(fd, 1)) {
        close_socket(fd);
        return INVALID_SOCKET_HANDLE;
    }
    return fd;
}

socket_handle connect_to(const std::string & endpoint) {
    const size_t colon = endpoint.rfind(':');
    if (colon == std::string::npos) return INVALID_SOCKET_HANDLE;
    for (int attempt = 0; attempt < 300; ++attempt) {
        addrinfo hints{};
        hints.ai_family = AF_UNSPEC;
        hints.ai_socktype = SOCK_STREAM;
        addrinfo * result = nullptr;
        if (getaddrinfo(endpoint.substr(0, colon).c_str(), endpoint.substr(colon + 1).c_str(),
                        &hints, &result) == 0) {
            for (auto * item = result; item; item = item->ai_next) {
                socket_handle fd = socket(item->ai_family, item->ai_socktype, item->ai_protocol);
                if (fd != INVALID_SOCKET_HANDLE
                    && connect(fd, item->ai_addr, (int) item->ai_addrlen) == 0) {
                    freeaddrinfo(result);
                    return fd;
                }
                if (fd != INVALID_SOCKET_HANDLE) close_socket(fd);
            }
            freeaddrinfo(result);
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
    }
    return INVALID_SOCKET_HANDLE;
}

// Connection role preamble — see linkcpp-node send_role/recv_role. 'P' = the
// dialer is the accepter's predecessor (-> prev_fd); 'N' = successor (-> next_fd).
constexpr char RING_ROLE_PRED = 'P';
constexpr char RING_ROLE_SUCC = 'N';
bool send_role(socket_handle fd, char role) { return write_all(fd, &role, 1); }
bool recv_role(socket_handle fd, char * role) { return read_all(fd, role, 1); }

// The ring's small send-then-block-recv frames stall under Nagle + delayed-ACK,
// badly so once relayed over a WebSocket; disable Nagle on every ring socket.
void set_tcp_nodelay(socket_handle fd) {
    int one = 1;
    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, reinterpret_cast<const char *>(&one), sizeof(one));
}

bool collect_boundary(llama_context * ctx, boundary_bundle * tensors) {
    const int count = llama_linkcpp_output_count(ctx);
    if (count <= 0 || count > MAX_BOUNDARY_TENSORS) return false;
    tensors->clear();
    tensors->reserve((size_t) count);
    for (int i = 0; i < count; ++i) {
        boundary_tensor tensor;
        if (!llama_linkcpp_output_desc(ctx, i, &tensor.desc)) return false;
        if (tensor.desc.alias_of < 0) {
            tensor.data.resize((size_t) tensor.desc.nbytes);
            if (!llama_linkcpp_output_get(ctx, i, tensor.data.data(), tensor.data.size())) return false;
        }
        tensors->push_back(std::move(tensor));
    }
    if (std::getenv("LINKCPP_RING_TRACE")) {
        for (size_t i = 0; i < tensors->size(); ++i) {
            const auto & desc = (*tensors)[i].desc;
            double sum = 0.0;
            double sumsq = 0.0;
            if (desc.type == GGML_TYPE_F32 && !(*tensors)[i].data.empty()) {
                const float * values = reinterpret_cast<const float *>((*tensors)[i].data.data());
                const size_t count = (*tensors)[i].data.size() / sizeof(float);
                for (size_t j = 0; j < count; ++j) {
                    sum += values[j];
                    sumsq += (double) values[j] * values[j];
                }
            }
            std::fprintf(stderr,
                         "linkcpp boundary send: index=%zu name=%s type=%d bytes=%llu alias=%d sum=%.9f norm=%.9f\n",
                         i, desc.name, desc.type,
                         (unsigned long long) desc.nbytes, desc.alias_of, sum, std::sqrt(sumsq));
        }
    }
    return true;
}

bool encode_state_bundle(const std::vector<remote_state_entry> & entries, std::vector<uint8_t> * data) {
    if (!data || entries.size() > UINT32_MAX) return false;
    uint64_t total = sizeof(state_bundle_header);
    for (const auto & entry : entries) {
        if (entry.layer_begin < 0 || entry.layer_end <= entry.layer_begin
            || entry.data.size() != entry.total_size
            || total > SIZE_MAX - sizeof(state_bundle_entry) - entry.data.size()) return false;
        total += sizeof(state_bundle_entry) + entry.data.size();
    }
    data->resize((size_t) total);
    const state_bundle_header header = {1, (uint32_t) entries.size(), 0};
    memcpy(data->data(), &header, sizeof(header));
    size_t offset = sizeof(header);
    for (const auto & entry : entries) {
        const state_bundle_entry wire = {entry.layer_begin, entry.layer_end, entry.total_size};
        memcpy(data->data() + offset, &wire, sizeof(wire));
        offset += sizeof(wire);
        memcpy(data->data() + offset, entry.data.data(), entry.data.size());
        offset += entry.data.size();
    }
    return true;
}

bool decode_state_bundle(const uint8_t * data, size_t size, std::vector<remote_state_entry> * entries) {
    if (!data || !entries || size < sizeof(state_bundle_header)) return false;
    state_bundle_header header{};
    memcpy(&header, data, sizeof(header));
    if (header.version != 1 || header.reserved) return false;
    entries->clear();
    entries->reserve(header.count);
    size_t offset = sizeof(header);
    for (uint32_t i = 0; i < header.count; ++i) {
        if (offset > size || sizeof(state_bundle_entry) > size - offset) return false;
        state_bundle_entry wire{};
        memcpy(&wire, data + offset, sizeof(wire));
        offset += sizeof(wire);
        if (wire.layer_begin < 0 || wire.layer_end <= wire.layer_begin
            || wire.size > SIZE_MAX || wire.size > size - offset) return false;
        remote_state_entry entry;
        entry.layer_begin = wire.layer_begin;
        entry.layer_end = wire.layer_end;
        entry.total_size = wire.size;
        entry.data.assign(data + offset, data + offset + (size_t) wire.size);
        offset += (size_t) wire.size;
        entries->push_back(std::move(entry));
    }
    return offset == size;
}

bool encode_boundary(const boundary_bundle & tensors, std::vector<uint8_t> * payload) {
    if (tensors.empty() || tensors.size() > MAX_BOUNDARY_TENSORS) return false;
    uint64_t total = sizeof(boundary_bundle_header);
    for (const auto & tensor : tensors) total += sizeof(boundary_desc_wire) + tensor.data.size();
    if (total > MAX_FRAME_BYTES) return false;
    payload->resize((size_t) total);
    boundary_bundle_header header = {{'L','K','B','3'}, BOUNDARY_VERSION, 0,
                                     (uint16_t) tensors.size(), 0};
    memcpy(payload->data(), &header, sizeof(header));
    size_t offset = sizeof(header);
    for (const auto & tensor : tensors) {
        boundary_desc_wire wire{};
        wire.type = tensor.desc.type;
        wire.n_dims = tensor.desc.n_dims;
        memcpy(wire.ne, tensor.desc.ne, sizeof(wire.ne));
        memcpy(wire.nb, tensor.desc.nb, sizeof(wire.nb));
        wire.nbytes = tensor.desc.nbytes;
        wire.view_offset = tensor.desc.view_offset;
        wire.alias_of = tensor.desc.alias_of;
        wire.flags = tensor.desc.flags;
        snprintf(wire.name, sizeof(wire.name), "%s", tensor.desc.name);
        memcpy(payload->data() + offset, &wire, sizeof(wire));
        offset += sizeof(wire);
        if (!tensor.data.empty()) {
            memcpy(payload->data() + offset, tensor.data.data(), tensor.data.size());
            offset += tensor.data.size();
        }
    }
    return true;
}

bool decode_boundary(const std::vector<uint8_t> & payload, boundary_bundle * tensors) {
    if (payload.size() < sizeof(boundary_bundle_header)) return false;
    boundary_bundle_header header{};
    memcpy(&header, payload.data(), sizeof(header));
    if (memcmp(header.magic, "LKB3", 4) != 0 || header.version != BOUNDARY_VERSION
        || header.reserved || header.reserved2 || !header.count
        || header.count > MAX_BOUNDARY_TENSORS) return false;
    size_t offset = sizeof(header);
    tensors->clear();
    for (uint16_t i = 0; i < header.count; ++i) {
        if (offset + sizeof(boundary_desc_wire) > payload.size()) return false;
        boundary_desc_wire wire{};
        memcpy(&wire, payload.data() + offset, sizeof(wire));
        offset += sizeof(wire);
        const uint64_t bytes = wire.alias_of >= 0 ? 0 : wire.nbytes;
        if (wire.alias_of >= (int32_t) i || wire.n_dims < 1 || wire.n_dims > 4
            || wire.flags || offset + bytes > payload.size()) return false;
        boundary_tensor tensor;
        tensor.desc.type = wire.type;
        tensor.desc.n_dims = wire.n_dims;
        memcpy(tensor.desc.ne, wire.ne, sizeof(wire.ne));
        memcpy(tensor.desc.nb, wire.nb, sizeof(wire.nb));
        tensor.desc.nbytes = wire.nbytes;
        tensor.desc.view_offset = wire.view_offset;
        tensor.desc.alias_of = wire.alias_of;
        snprintf(tensor.desc.name, sizeof(tensor.desc.name), "%s", wire.name);
        tensor.data.assign(payload.begin() + offset, payload.begin() + offset + bytes);
        offset += bytes;
        tensors->push_back(std::move(tensor));
    }
    return offset == payload.size();
}
}

struct linkcpp_ring_executor::impl {
    explicit impl(linkcpp_ring_options options) : options(std::move(options)) {
#ifdef _WIN32
        WSADATA wsa{};
        sockets_ready = WSAStartup(MAKEWORD(2, 2), &wsa) == 0;
#else
        sockets_ready = true;
#endif
        if (sockets_ready) listener = listen_on(this->options.listen_port);
    }

    ~impl() {
        if (prev_fd != INVALID_SOCKET_HANDLE) close_socket(prev_fd);
        if (next_fd != INVALID_SOCKET_HANDLE) close_socket(next_fd);
        if (listener != INVALID_SOCKET_HANDLE) close_socket(listener);
#ifdef _WIN32
        if (sockets_ready) WSACleanup();
#endif
    }

    static bool callback(llama_context * ctx,
                         const llama_linkcpp_stage_invocation * invocation,
                         void * user_data) {
        return static_cast<impl *>(user_data)->execute(ctx, invocation);
    }

    static bool memory_callback(
            const llama_linkcpp_memory_invocation * invocation,
            void * user_data) {
        return static_cast<impl *>(user_data)->memory(invocation);
    }

    static bool state_callback(
            llama_linkcpp_state_invocation * invocation,
            void * user_data) {
        return static_cast<impl *>(user_data)->state(invocation);
    }

    bool connect_ring(llama_context * ctx) {
        if (prev_fd != INVALID_SOCKET_HANDLE && next_fd != INVALID_SOCKET_HANDLE) return true;
        const bool do_dial_next = !options.accept_next;
        const bool do_dial_prev = !options.dial_prev_endpoint.empty();
        const int n_accepts = (do_dial_next ? 0 : 1) + (do_dial_prev ? 0 : 1);
        if (n_accepts > 0 && listener == INVALID_SOCKET_HANDLE) return false;
        // Outbound dials first, each announcing our role to the accepter.
        if (do_dial_next) {
            next_fd = connect_to(options.next_endpoint);
            if (next_fd != INVALID_SOCKET_HANDLE) set_tcp_nodelay(next_fd);
            if (next_fd == INVALID_SOCKET_HANDLE || !send_role(next_fd, RING_ROLE_PRED)) return false;
        }
        if (do_dial_prev) {
            prev_fd = connect_to(options.dial_prev_endpoint);
            if (prev_fd != INVALID_SOCKET_HANDLE) set_tcp_nodelay(prev_fd);
            if (prev_fd == INVALID_SOCKET_HANDLE || !send_role(prev_fd, RING_ROLE_SUCC)) return false;
        }
        for (int i = 0; i < n_accepts; ++i) {
            socket_handle fd = accept(listener, nullptr, nullptr);
            if (fd != INVALID_SOCKET_HANDLE) set_tcp_nodelay(fd);
            char r = 0;
            if (fd == INVALID_SOCKET_HANDLE || !recv_role(fd, &r)) return false;
            if (r == RING_ROLE_PRED) prev_fd = fd; else next_fd = fd;
        }
        if (listener != INVALID_SOCKET_HANDLE) { close_socket(listener); listener = INVALID_SOCKET_HANDLE; }
        if (prev_fd == INVALID_SOCKET_HANDLE || next_fd == INVALID_SOCKET_HANDLE) return false;
        const auto * model = llama_get_model(ctx);
        hello_payload sent = {(uint32_t) llama_model_n_embd(model),
                              (uint32_t) options.layer_begin, (uint32_t) options.layer_end,
                              LINKCPP_RING_ADAPTER_ABI, LINKCPP_RING_BUILD_FINGERPRINT};
        if (!send_frame(next_fd, FRAME_HELLO, 0, &sent, sizeof(sent))) return false;
        frame incoming;
        if (!recv_frame(prev_fd, &incoming) || incoming.kind != FRAME_HELLO
            || incoming.payload.size() != sizeof(hello_payload)) return false;
        hello_payload received{};
        memcpy(&received, incoming.payload.data(), sizeof(received));
        return received.n_embd == sent.n_embd
            && received.adapter_abi == LINKCPP_RING_ADAPTER_ABI
            && received.build_fingerprint == LINKCPP_RING_BUILD_FINGERPRINT;
    }

    bool execute(llama_context * ctx, const llama_linkcpp_stage_invocation * invocation) {
        std::lock_guard<std::mutex> lock(mutex);
        auto fail = [](const char * stage) {
            std::fprintf(stderr, "linkcpp ring executor failed: stage=%s\n", stage);
            return false;
        };
        if (!invocation || invocation->version != 1) return fail("invocation");
        if (!connect_ring(ctx)) return fail("connect");
        boundary_bundle boundary;
        std::vector<uint8_t> boundary_bytes;
        if (!collect_boundary(ctx, &boundary)) return fail("collect_boundary");
        if (!encode_boundary(boundary, &boundary_bytes)) return fail("encode_boundary");
        uint64_t total_seq_ids = 0;
        for (uint32_t i = 0; i < invocation->n_tokens; ++i) {
            if (invocation->n_seq_id[i] <= 0) return fail("sequence_count");
            total_seq_ids += (uint32_t) invocation->n_seq_id[i];
        }
        execute_bundle_header h = {1, invocation->flags, invocation->n_tokens,
            invocation->n_seq_tokens, invocation->n_seqs, invocation->n_seqs_unq,
            invocation->n_pos, (uint32_t) total_seq_ids, (uint32_t) boundary_bytes.size(), 0};
        const uint64_t total = sizeof(h)
            + (uint64_t) h.n_tokens * h.n_pos * sizeof(llama_pos)
            + (uint64_t) h.n_tokens * sizeof(int32_t)
            + total_seq_ids * sizeof(llama_seq_id) + h.n_tokens + boundary_bytes.size();
        if (total > MAX_FRAME_BYTES) return fail("frame_size");
        std::vector<uint8_t> payload((size_t) total);
        size_t offset = 0;
        auto append = [&](const void * data, size_t bytes) {
            memcpy(payload.data() + offset, data, bytes);
            offset += bytes;
        };
        append(&h, sizeof(h));
        append(invocation->pos, (size_t) h.n_tokens * h.n_pos * sizeof(llama_pos));
        append(invocation->n_seq_id, (size_t) h.n_tokens * sizeof(int32_t));
        for (uint32_t i = 0; i < h.n_tokens; ++i) {
            append(invocation->seq_id[i], (size_t) invocation->n_seq_id[i] * sizeof(llama_seq_id));
        }
        append(invocation->output, h.n_tokens);
        append(boundary_bytes.data(), boundary_bytes.size());
        const uint64_t request_id = next_request_id++;
        if (!send_frame(next_fd, FRAME_EXECUTE, request_id, payload.data(), (uint32_t) payload.size())) return fail("send");
        frame result;
        if (!recv_frame(prev_fd, &result) || result.kind != FRAME_EXECUTE_RESULT
            || result.request_id != request_id) return fail("receive");
        boundary_bundle terminals;
        if (!decode_boundary(result.payload, &terminals)
            || llama_linkcpp_terminal_count(ctx) != (int32_t) terminals.size()) return fail("terminal_bundle");
        for (size_t i = 0; i < terminals.size(); ++i) {
            llama_linkcpp_tensor_desc expected{};
            if (!llama_linkcpp_terminal_desc(ctx, (int32_t) i, &expected)
                || expected.type != terminals[i].desc.type
                || expected.nbytes != terminals[i].desc.nbytes
                || memcmp(expected.ne, terminals[i].desc.ne, sizeof(expected.ne)) != 0) return fail("terminal_schema");
            const int32_t alias = terminals[i].desc.alias_of;
            const auto & data = alias >= 0 ? terminals[(size_t) alias].data : terminals[i].data;
            if (!llama_linkcpp_terminal_set(ctx, (int32_t) i, data.data(), data.size())) return fail("terminal_set");
        }
        return true;
    }

    bool memory(const llama_linkcpp_memory_invocation * invocation) {
        std::lock_guard<std::mutex> lock(mutex);
        if (!invocation || invocation->version != 1 || invocation->reserved
            || invocation->op < LLAMA_LINKCPP_MEMORY_CLEAR
            || invocation->op > LLAMA_LINKCPP_MEMORY_SEQ_DIV) return false;
        // Before the first distributed graph execution the successor has not
        // observed any state, so there is nothing to mirror yet.
        if (next_fd == INVALID_SOCKET_HANDLE || prev_fd == INVALID_SOCKET_HANDLE) return true;
        const memory_op_wire wire = {
            invocation->version, invocation->op,
            invocation->seq_id_src, invocation->seq_id_dst,
            invocation->p0, invocation->p1, invocation->value, 0,
        };
        const uint64_t request_id = next_request_id++;
        if (std::getenv("LINKCPP_RING_TRACE")) {
            std::fprintf(stderr, "linkcpp memory send: op=%u src=%d dst=%d p=[%d,%d) value=%d\n",
                         wire.op, wire.seq_id_src, wire.seq_id_dst, wire.p0, wire.p1, wire.value);
        }
        if (!send_frame(next_fd, FRAME_MEMORY, request_id, &wire, sizeof(wire))) return false;
        frame result;
        return recv_frame(prev_fd, &result)
            && result.kind == FRAME_ACK
            && result.request_id == request_id
            && result.payload.empty();
    }

    bool collect_remote_state(llama_context * ctx, int32_t seq_id, uint32_t flags) {
        if (!connect_ring(ctx) || (flags & LLAMA_STATE_SEQ_FLAGS_ON_DEVICE)) return false;
        const state_op_wire request = {1, seq_id, flags, 0, 0, 0, 0, 0};
        const uint64_t request_id = next_request_id++;
        if (!send_frame(next_fd, FRAME_STATE_GET, request_id, &request, sizeof(request))) return false;
        std::vector<remote_state_entry> entries;
        for (;;) {
            frame incoming;
            if (!recv_frame(prev_fd, &incoming) || incoming.request_id != request_id) return false;
            if (incoming.kind == FRAME_STATE_DONE) {
                if (incoming.payload.size() != sizeof(state_op_wire) || entries.empty()) return false;
                state_op_wire done{};
                memcpy(&done, incoming.payload.data(), sizeof(done));
                if (done.version != 1 || !done.status
                    || done.layer_begin != entries.back().layer_begin
                    || done.layer_end != entries.back().layer_end) return false;
                break;
            }
            if (incoming.kind != FRAME_STATE_CHUNK
                || incoming.payload.size() < sizeof(state_op_wire)) return false;
            state_op_wire wire{};
            memcpy(&wire, incoming.payload.data(), sizeof(wire));
            const size_t count = incoming.payload.size() - sizeof(wire);
            if (wire.version != 1 || !wire.status || wire.layer_begin < 0
                || wire.layer_end <= wire.layer_begin || wire.total_size > SIZE_MAX
                || wire.offset > wire.total_size || count > wire.total_size - wire.offset) return false;
            if (wire.offset == 0) {
                if ((!entries.empty() && entries.back().layer_end != wire.layer_begin)
                    || (entries.empty() && wire.layer_begin != options.layer_end)) return false;
                entries.push_back({wire.layer_begin, wire.layer_end, wire.total_size, {}});
                entries.back().data.reserve((size_t) wire.total_size);
            }
            if (entries.empty()) return false;
            auto & entry = entries.back();
            if (entry.layer_begin != wire.layer_begin || entry.layer_end != wire.layer_end
                || entry.total_size != wire.total_size || entry.data.size() != wire.offset) return false;
            entry.data.insert(entry.data.end(), incoming.payload.begin() + sizeof(wire), incoming.payload.end());
        }
        if (entries.back().data.size() != entries.back().total_size) return false;
        for (const auto & entry : entries) {
            if (entry.data.size() != entry.total_size) return false;
        }
        if (!encode_state_bundle(entries, &cached_state)) return false;
        if (std::getenv("LINKCPP_RING_TRACE")) {
            std::fprintf(stderr, "linkcpp state captured: seq=%d flags=%u stages=%zu bytes=%zu\n",
                         seq_id, flags, entries.size(), cached_state.size());
        }
        cached_state_seq_id = seq_id;
        cached_state_flags = flags;
        cached_state_valid = true;
        return true;
    }

    bool restore_remote_state(const uint8_t * data, size_t size, int32_t seq_id, uint32_t flags) {
        std::vector<remote_state_entry> entries;
        if (!decode_state_bundle(data, size, &entries) || entries.empty()
            || entries.front().layer_begin != options.layer_end) return false;
        for (size_t i = 1; i < entries.size(); ++i) {
            if (entries[i - 1].layer_end != entries[i].layer_begin) return false;
        }
        for (const auto & entry : entries) {
            const uint64_t request_id = next_request_id++;
            state_op_wire wire = {
                1, seq_id, flags, entry.layer_begin, entry.layer_end, 0,
                entry.total_size, 0,
            };
            if (!send_frame(next_fd, FRAME_STATE_SET_BEGIN, request_id, &wire, sizeof(wire))) return false;
            for (size_t offset = 0; offset < entry.data.size();) {
                const size_t count = std::min(STATE_CHUNK_BYTES, entry.data.size() - offset);
                wire.offset = offset;
                std::vector<uint8_t> payload(sizeof(wire) + count);
                memcpy(payload.data(), &wire, sizeof(wire));
                memcpy(payload.data() + sizeof(wire), entry.data.data() + offset, count);
                if (!send_frame(next_fd, FRAME_STATE_SET_CHUNK, request_id,
                                payload.data(), (uint32_t) payload.size())) return false;
                offset += count;
            }
            wire.offset = 0;
            if (!send_frame(next_fd, FRAME_STATE_SET_COMMIT, request_id, &wire, sizeof(wire))) return false;
            frame result;
            if (!recv_frame(prev_fd, &result) || result.kind != FRAME_STATE_ACK
                || result.request_id != request_id || result.payload.size() != sizeof(state_op_wire)) return false;
            state_op_wire ack{};
            memcpy(&ack, result.payload.data(), sizeof(ack));
            if (ack.version != 1 || !ack.status || ack.offset != 1
                || ack.layer_begin != entry.layer_begin || ack.layer_end != entry.layer_end
                || ack.total_size != entry.total_size) return false;
            if (std::getenv("LINKCPP_RING_TRACE")) {
                std::fprintf(stderr, "linkcpp state restored: seq=%d flags=%u layers=[%d,%d) bytes=%llu\n",
                             seq_id, flags, entry.layer_begin, entry.layer_end,
                             (unsigned long long) entry.total_size);
            }
        }
        cached_state_valid = false;
        cached_state.clear();
        return true;
    }

    bool state(llama_linkcpp_state_invocation * invocation) {
        std::lock_guard<std::mutex> lock(mutex);
        if (!invocation || invocation->version != 1 || invocation->reserved
            || invocation->op < LLAMA_LINKCPP_STATE_SEQ_GET_SIZE
            || invocation->op > LLAMA_LINKCPP_STATE_SEQ_SET_DATA
            || (invocation->flags & LLAMA_STATE_SEQ_FLAGS_ON_DEVICE)) return false;
        const bool cache_matches = cached_state_valid
            && cached_state_seq_id == invocation->seq_id
            && cached_state_flags == invocation->flags;
        if (invocation->op == LLAMA_LINKCPP_STATE_SEQ_GET_SIZE) {
            if (!collect_remote_state(invocation->ctx, invocation->seq_id, invocation->flags)) return false;
            invocation->result_size = cached_state.size();
            return true;
        }
        if (invocation->op == LLAMA_LINKCPP_STATE_SEQ_GET_DATA) {
            if (!cache_matches
                && !collect_remote_state(invocation->ctx, invocation->seq_id, invocation->flags)) return false;
            if (!invocation->output || invocation->output_capacity < cached_state.size()) return false;
            memcpy(invocation->output, cached_state.data(), cached_state.size());
            invocation->result_size = cached_state.size();
            return true;
        }
        if (!invocation->input || invocation->input_size == 0
            || !connect_ring(invocation->ctx)) return false;
        if (!restore_remote_state(invocation->input, invocation->input_size,
                                  invocation->seq_id, invocation->flags)) return false;
        invocation->result_size = invocation->input_size;
        return true;
    }

    linkcpp_ring_options options;
    bool sockets_ready = false;
    socket_handle listener = INVALID_SOCKET_HANDLE;
    socket_handle prev_fd = INVALID_SOCKET_HANDLE;
    socket_handle next_fd = INVALID_SOCKET_HANDLE;
    uint64_t next_request_id = 1;
    std::mutex mutex;
    std::vector<uint8_t> cached_state;
    int32_t cached_state_seq_id = -1;
    uint32_t cached_state_flags = 0;
    bool cached_state_valid = false;
};

linkcpp_ring_executor::linkcpp_ring_executor(linkcpp_ring_options options)
    : pimpl(new impl(std::move(options))) {}
linkcpp_ring_executor::~linkcpp_ring_executor() = default;

bool linkcpp_ring_executor::configure(const std::string & model_path) {
    if (pimpl->listener == INVALID_SOCKET_HANDLE) return false;
    llama_linkcpp_runtime_params params = {
        model_path.c_str(), pimpl->options.layer_begin, pimpl->options.layer_end,
        &impl::callback, pimpl.get(), &impl::memory_callback, pimpl.get(),
        &impl::state_callback, pimpl.get(),
    };
    return llama_linkcpp_runtime_configure(&params);
}
