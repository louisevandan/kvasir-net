// linkcpp-node: rank-local model window and adjacent-ring stage runner.
#include "llama.h"
#include "../linkcpp-ring-protocol.h"
#include "ggml-backend.h"
#include "chat.h"

#ifdef _WIN32
#  define NOMINMAX
#  include <winsock2.h>
#  include <ws2tcpip.h>
#  include <fcntl.h>
#  include <io.h>
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
#include <exception>
#include <string>
#include <thread>
#include <type_traits>
#include <utility>
#include <vector>

namespace {
constexpr uint8_t PROTOCOL_VERSION = LINKCPP_RING_WIRE_VERSION;
constexpr uint8_t FRAME_HELLO = 1;
constexpr uint8_t FRAME_PREFILL = 2;
constexpr uint8_t FRAME_DECODE = 3;
constexpr uint8_t FRAME_TOKEN = 4;
constexpr uint8_t FRAME_RESET = 5;
constexpr uint8_t FRAME_ACK = 6;
constexpr uint8_t FRAME_EMBED = 7;
constexpr uint8_t FRAME_TERMINALS = 8;
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
constexpr uint8_t CONTROL_FLAG_CHAT_MESSAGES = 1;
constexpr uint8_t CONTROL_FLAG_EMBEDDING = 2;
constexpr uint8_t CONTROL_FLAG_TOKEN_IDS = 4;
constexpr uint32_t MAX_FRAME_BYTES = 64u * 1024u * 1024u;
constexpr size_t STATE_CHUNK_BYTES = 4u * 1024u * 1024u;
constexpr uint32_t MAX_PROMPT_BYTES = 16u * 1024u * 1024u;
constexpr uint8_t BOUNDARY_VERSION = 3;
constexpr uint16_t MAX_BOUNDARY_TENSORS = 1024;

#ifdef _WIN32
using socket_handle = SOCKET;
constexpr socket_handle INVALID_SOCKET_HANDLE = INVALID_SOCKET;
void close_socket(socket_handle fd) { closesocket(fd); }
int fd_read(int fd, void * data, unsigned size) { return _read(fd, data, size); }
int fd_write(int fd, const void * data, unsigned size) { return _write(fd, data, size); }
#else
using socket_handle = int;
constexpr socket_handle INVALID_SOCKET_HANDLE = -1;
void close_socket(socket_handle fd) { close(fd); }
int fd_read(int fd, void * data, unsigned size) { return (int) read(fd, data, size); }
int fd_write(int fd, const void * data, unsigned size) { return (int) write(fd, data, size); }
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

struct control_request_header {
    char magic[4];
    uint8_t version;
    uint8_t flags;
    uint16_t reserved;
    uint64_t request_id;
    uint32_t prompt_bytes;
    uint32_t max_tokens;
    uint64_t reserved2;
};

struct control_response_header {
    char magic[4];
    uint8_t version;
    uint8_t status;
    uint16_t flags;
    uint64_t request_id;
    uint32_t text_bytes;
    uint32_t token_count;
    uint32_t elapsed_ms;
    uint32_t reserved;
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
#pragma pack(pop)

static_assert(sizeof(wire_header) == 32, "wire header must match linkcpp-stage-v1");
static_assert(sizeof(control_request_header) == 32, "control request header must be 32 bytes");
static_assert(sizeof(control_response_header) == 32, "control response header must be 32 bytes");
static_assert(sizeof(boundary_bundle_header) == 12, "boundary bundle header must be 12 bytes");
static_assert(sizeof(boundary_desc_wire) == 160, "boundary descriptor must be 160 bytes");
static_assert(sizeof(execute_bundle_header) == 40, "execute bundle header must be 40 bytes");
static_assert(sizeof(memory_op_wire) == 32, "memory operation header must be 32 bytes");
static_assert(sizeof(state_op_wire) == 40, "state operation header must be 40 bytes");

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

using boundary_bundle = std::vector<boundary_tensor>;

struct execute_bundle {
    execute_bundle_header header{};
    std::vector<llama_pos> positions;
    std::vector<int32_t> n_seq_id;
    std::vector<llama_seq_id> seq_ids;
    std::vector<int8_t> output;
    boundary_bundle tensors;
};

bool socket_write_all(socket_handle fd, const void * data, size_t size) {
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

bool socket_read_all(socket_handle fd, void * data, size_t size) {
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

bool fd_write_all(int fd, const void * data, size_t size) {
    const auto * p = static_cast<const uint8_t *>(data);
    while (size) {
        const int n = fd_write(fd, p, (unsigned) std::min<size_t>(size, 1u << 20));
        if (n <= 0) return false;
        p += n;
        size -= (size_t) n;
    }
    return true;
}

bool fd_read_all(int fd, void * data, size_t size) {
    auto * p = static_cast<uint8_t *>(data);
    while (size) {
        const int n = fd_read(fd, p, (unsigned) std::min<size_t>(size, 1u << 20));
        if (n <= 0) return false;
        p += n;
        size -= (size_t) n;
    }
    return true;
}

bool send_frame(socket_handle fd, uint8_t kind, uint64_t request_id, uint32_t position,
                const void * data = nullptr, uint32_t bytes = 0, uint32_t sequence_id = 0) {
    wire_header h = {{'L', 'K', 'S', '1'}, PROTOCOL_VERSION, kind, 0,
                     request_id, sequence_id, position, bytes, 0};
    return bytes <= MAX_FRAME_BYTES
        && socket_write_all(fd, &h, sizeof(h))
        && (bytes == 0 || socket_write_all(fd, data, bytes));
}

bool recv_frame(socket_handle fd, frame * result) {
    wire_header h{};
    if (!socket_read_all(fd, &h, sizeof(h)) || memcmp(h.magic, "LKS1", 4) != 0
        || h.version != PROTOCOL_VERSION || h.flags != 0 || h.reserved != 0
        || h.bytes > MAX_FRAME_BYTES) return false;
    result->kind = h.kind;
    result->request_id = h.request_id;
    result->sequence_id = h.sequence_id;
    result->position = h.position;
    result->payload.resize(h.bytes);
    return h.bytes == 0 || socket_read_all(fd, result->payload.data(), h.bytes);
}

bool encode_boundary_bundle(const boundary_bundle & tensors, std::vector<uint8_t> * payload) {
    if (tensors.empty() || tensors.size() > MAX_BOUNDARY_TENSORS) return false;
    size_t total = sizeof(boundary_bundle_header);
    for (const auto & tensor : tensors) {
        const bool alias = tensor.desc.alias_of >= 0;
        if ((!alias && tensor.data.size() != tensor.desc.nbytes)
            || (alias && !tensor.data.empty())
            || tensor.desc.n_dims < 1 || tensor.desc.n_dims > 4) {
            return false;
        }
        total += sizeof(boundary_desc_wire) + tensor.data.size();
        if (total > MAX_FRAME_BYTES) return false;
    }
    payload->resize(total);
    boundary_bundle_header header = {
        {'L', 'K', 'B', '3'}, BOUNDARY_VERSION, 0, (uint16_t) tensors.size(), 0,
    };
    memcpy(payload->data(), &header, sizeof(header));
    size_t offset = sizeof(header);
    for (const auto & tensor : tensors) {
        boundary_desc_wire desc{};
        desc.type = tensor.desc.type;
        desc.n_dims = tensor.desc.n_dims;
        memcpy(desc.ne, tensor.desc.ne, sizeof(desc.ne));
        memcpy(desc.nb, tensor.desc.nb, sizeof(desc.nb));
        desc.nbytes = tensor.desc.nbytes;
        desc.view_offset = tensor.desc.view_offset;
        desc.alias_of = tensor.desc.alias_of;
        desc.flags = tensor.desc.flags;
        snprintf(desc.name, sizeof(desc.name), "%s", tensor.desc.name);
        memcpy(payload->data() + offset, &desc, sizeof(desc));
        offset += sizeof(desc);
        memcpy(payload->data() + offset, tensor.data.data(), tensor.data.size());
        offset += tensor.data.size();
    }
    return true;
}

bool decode_boundary_bundle(const std::vector<uint8_t> & payload, boundary_bundle * tensors) {
    if (payload.size() < sizeof(boundary_bundle_header)) return false;
    boundary_bundle_header header{};
    memcpy(&header, payload.data(), sizeof(header));
    if (memcmp(header.magic, "LKB3", 4) != 0 || header.version != BOUNDARY_VERSION
        || header.reserved != 0 || header.reserved2 != 0
        || header.count == 0 || header.count > MAX_BOUNDARY_TENSORS) return false;
    size_t offset = sizeof(header);
    tensors->clear();
    tensors->reserve(header.count);
    for (uint16_t i = 0; i < header.count; ++i) {
        if (offset + sizeof(boundary_desc_wire) > payload.size()) return false;
        boundary_desc_wire wire{};
        memcpy(&wire, payload.data() + offset, sizeof(wire));
        offset += sizeof(wire);
        const bool alias = wire.alias_of >= 0;
        const uint64_t data_bytes = alias ? 0 : wire.nbytes;
        if (wire.n_dims < 1 || wire.n_dims > 4 || wire.nbytes > MAX_FRAME_BYTES
            || wire.flags != 0 || wire.alias_of >= (int32_t) i
            || offset + data_bytes > payload.size()) return false;
        boundary_tensor tensor;
        tensor.desc.type = wire.type;
        tensor.desc.n_dims = wire.n_dims;
        memcpy(tensor.desc.ne, wire.ne, sizeof(wire.ne));
        memcpy(tensor.desc.nb, wire.nb, sizeof(wire.nb));
        tensor.desc.nbytes = wire.nbytes;
        tensor.desc.view_offset = wire.view_offset;
        tensor.desc.alias_of = wire.alias_of;
        tensor.desc.flags = wire.flags;
        snprintf(tensor.desc.name, sizeof(tensor.desc.name), "%s", wire.name);
        tensor.data.assign(payload.begin() + offset, payload.begin() + offset + data_bytes);
        offset += data_bytes;
        tensors->push_back(std::move(tensor));
    }
    return offset == payload.size();
}

bool decode_execute_bundle(const std::vector<uint8_t> & payload, execute_bundle * result) {
    if (payload.size() < sizeof(execute_bundle_header)) return false;
    memcpy(&result->header, payload.data(), sizeof(result->header));
    const auto & h = result->header;
    if (h.version != 1 || h.reserved != 0 || h.n_tokens == 0
        || h.n_tokens > 65536 || h.n_pos == 0 || h.n_pos > 4
        || h.total_seq_ids < h.n_tokens
        || h.total_seq_ids > (uint64_t) h.n_tokens * llama_max_parallel_sequences()
        || h.boundary_bytes > MAX_FRAME_BYTES) return false;
    const uint64_t positions_bytes = (uint64_t) h.n_tokens * h.n_pos * sizeof(llama_pos);
    const uint64_t counts_bytes = (uint64_t) h.n_tokens * sizeof(int32_t);
    const uint64_t seq_ids_bytes = (uint64_t) h.total_seq_ids * sizeof(llama_seq_id);
    const uint64_t output_bytes = h.n_tokens;
    const uint64_t expected = sizeof(execute_bundle_header) + positions_bytes + counts_bytes
        + seq_ids_bytes + output_bytes + h.boundary_bytes;
    if (expected != payload.size()) return false;
    size_t offset = sizeof(execute_bundle_header);
    auto copy_vector = [&](auto * vector, size_t count) {
        using value_type = typename std::remove_reference_t<decltype(*vector)>::value_type;
        vector->resize(count);
        const size_t bytes = count * sizeof(value_type);
        memcpy(vector->data(), payload.data() + offset, bytes);
        offset += bytes;
    };
    copy_vector(&result->positions, (size_t) h.n_tokens * h.n_pos);
    copy_vector(&result->n_seq_id, h.n_tokens);
    copy_vector(&result->seq_ids, h.total_seq_ids);
    copy_vector(&result->output, h.n_tokens);
    uint64_t count_sum = 0;
    for (int32_t count : result->n_seq_id) {
        if (count <= 0 || (size_t) count > llama_max_parallel_sequences()) return false;
        count_sum += (uint32_t) count;
    }
    if (count_sum != h.total_seq_ids) return false;
    std::vector<uint8_t> boundary(payload.begin() + offset, payload.end());
    return decode_boundary_bundle(boundary, &result->tensors);
}

bool encode_execute_bundle(const execute_bundle & request, const boundary_bundle & tensors,
                           std::vector<uint8_t> * payload) {
    std::vector<uint8_t> boundary;
    if (!encode_boundary_bundle(tensors, &boundary)) return false;
    execute_bundle_header h = request.header;
    h.boundary_bytes = (uint32_t) boundary.size();
    const uint64_t total = sizeof(h)
        + (uint64_t) request.positions.size() * sizeof(llama_pos)
        + (uint64_t) request.n_seq_id.size() * sizeof(int32_t)
        + (uint64_t) request.seq_ids.size() * sizeof(llama_seq_id)
        + request.output.size() + boundary.size();
    if (total > MAX_FRAME_BYTES
        || request.positions.size() != (size_t) h.n_tokens * h.n_pos
        || request.n_seq_id.size() != h.n_tokens
        || request.seq_ids.size() != h.total_seq_ids
        || request.output.size() != h.n_tokens) return false;
    payload->resize((size_t) total);
    size_t offset = 0;
    auto append = [&](const void * data, size_t bytes) {
        memcpy(payload->data() + offset, data, bytes);
        offset += bytes;
    };
    append(&h, sizeof(h));
    append(request.positions.data(), request.positions.size() * sizeof(llama_pos));
    append(request.n_seq_id.data(), request.n_seq_id.size() * sizeof(int32_t));
    append(request.seq_ids.data(), request.seq_ids.size() * sizeof(llama_seq_id));
    append(request.output.data(), request.output.size());
    append(boundary.data(), boundary.size());
    return true;
}

socket_handle listen_on(int port) {
    const socket_handle fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd == INVALID_SOCKET_HANDLE) return INVALID_SOCKET_HANDLE;
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
    socket_handle fd = INVALID_SOCKET_HANDLE;
    for (int attempt = 0; attempt < 300 && fd == INVALID_SOCKET_HANDLE; ++attempt) {
        addrinfo hints{};
        hints.ai_family = AF_UNSPEC;
        hints.ai_socktype = SOCK_STREAM;
        addrinfo * result = nullptr;
        if (getaddrinfo(endpoint.substr(0, colon).c_str(), endpoint.substr(colon + 1).c_str(),
                        &hints, &result) == 0) {
            for (auto * item = result; item; item = item->ai_next) {
                fd = socket(item->ai_family, item->ai_socktype, item->ai_protocol);
                if (fd != INVALID_SOCKET_HANDLE
                    && connect(fd, item->ai_addr, (int) item->ai_addrlen) == 0) break;
                if (fd != INVALID_SOCKET_HANDLE) close_socket(fd);
                fd = INVALID_SOCKET_HANDLE;
            }
            freeaddrinfo(result);
        }
        if (fd == INVALID_SOCKET_HANDLE) std::this_thread::sleep_for(std::chrono::milliseconds(200));
    }
    return fd;
}

// Connection role preamble (NAT traversal). A ring edge's TCP connection can be
// opened by either endpoint — a node behind NAT can only dial out, so it dials
// both neighbours while public neighbours accept. The dialer announces who it is
// to the accepter so each side still assigns prev_fd/next_fd correctly,
// independent of who opened the socket:
//   'P' — "I am your predecessor" (I feed your input)  -> accepter's prev_fd
//   'N' — "I am your successor"   (I consume your output) -> accepter's next_fd
constexpr char RING_ROLE_PRED = 'P';
constexpr char RING_ROLE_SUCC = 'N';

bool send_role(socket_handle fd, char role) { return socket_write_all(fd, &role, 1); }
bool recv_role(socket_handle fd, char * role) { return socket_read_all(fd, role, 1); }

// The ring exchanges many small control/boundary frames with a send-then-block-
// recv pattern; Nagle + delayed-ACK stalls that badly, especially once relayed
// over a WebSocket. Disable Nagle on every ring socket.
void set_tcp_nodelay(socket_handle fd) {
    int one = 1;
    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, reinterpret_cast<const char *>(&one), sizeof(one));
}

struct stage_engine {
    llama_model * model = nullptr;
    llama_context * ctx = nullptr;
    const llama_vocab * vocab = nullptr;
    int n_embd = 0;
    int n_embd_out = 0;
    int n_vocab = 0;
    int context_size = 0;
    bool has_encoder = false;
    int first_layer = 0;
    int last_layer = 0;
    std::string architecture;
    common_chat_templates_ptr chat_templates;
    mutable bool boundary_schema_logged = false;
    mutable bool terminal_schema_logged = false;

    bool init(const char * path, int first, int last, int gpu_layers, int n_ctx,
              enum llama_pooling_type pooling, bool embeddings, int parallel,
              enum ggml_type type_k, enum ggml_type type_v,
              bool kv_offload = true, int req_n_batch = 0, int req_n_ubatch = 0) {
        auto mp = llama_model_default_params();
        mp.n_gpu_layers = gpu_layers;
        mp.linkcpp_layer_begin = first;
        mp.linkcpp_layer_end = last;
        model = llama_model_load_from_file(path, mp);
        if (!model) return false;
        std::vector<char> architecture_buf(128);
        int architecture_size = llama_model_meta_val_str(
            model, "general.architecture", architecture_buf.data(), architecture_buf.size());
        if (architecture_size <= 0) return false;
        architecture.assign(architecture_buf.data(), (size_t) architecture_size);
        try {
            chat_templates = common_chat_templates_init(model, "");
        } catch (const std::exception & exc) {
            fprintf(stderr, "chat template initialization failed: %s\n", exc.what());
            return false;
        }
        auto cp = llama_context_default_params();
        cp.n_ctx = n_ctx;
        // The coordinator drives the ring per-ubatch, so a stage never receives a
        // frame as large as the full context. The bare stage API does not inherit
        // llama-server's bounded CLI defaults; reserve at most a 512-token graph.
        cp.n_batch = req_n_batch > 0
            ? (uint32_t) req_n_batch
            : (uint32_t) std::min(n_ctx, 2048);
        cp.n_ubatch = req_n_ubatch > 0
            ? (uint32_t) req_n_ubatch
            : std::min(cp.n_batch, (uint32_t) 512);
        cp.n_seq_max = (uint32_t) std::max(1, parallel);
        cp.type_k = type_k;
        cp.type_v = type_v;
        cp.offload_kqv = kv_offload;
        has_encoder = llama_model_has_encoder(model);
        cp.embeddings = embeddings || has_encoder;
        cp.pooling_type = pooling;
        ctx = llama_init_from_model(model, cp);
        vocab = llama_model_get_vocab(model);
        n_embd = llama_model_n_embd(model);
        n_embd_out = llama_model_n_embd_out(model);
        n_vocab = llama_vocab_n_tokens(vocab);
        context_size = n_ctx;
        first_layer = first;
        last_layer = last;
        return ctx != nullptr;
    }

    void reset() {
        llama_memory_clear(llama_get_memory(ctx), true);
    }

    bool collect_boundary(boundary_bundle * tensors) const {
        const int count = llama_linkcpp_output_count(ctx);
        if (count < 0 || count > MAX_BOUNDARY_TENSORS) {
            fprintf(stderr, "invalid boundary output count: %d\n", count);
            return false;
        }
        tensors->clear();
        tensors->reserve((size_t) count);
        for (int i = 0; i < count; ++i) {
            boundary_tensor tensor;
            if (!llama_linkcpp_output_desc(ctx, i, &tensor.desc)
                || tensor.desc.nbytes > MAX_FRAME_BYTES) {
                fprintf(stderr, "invalid boundary output descriptor: index=%d\n", i);
                return false;
            }
            if (tensor.desc.alias_of < 0) {
                tensor.data.resize((size_t) tensor.desc.nbytes);
            }
            if (tensor.desc.alias_of < 0
                && !llama_linkcpp_output_get(ctx, i, tensor.data.data(), tensor.data.size())) {
                fprintf(stderr, "failed to read boundary output: index=%d name=%s bytes=%llu\n",
                        i, tensor.desc.name, (unsigned long long) tensor.desc.nbytes);
                return false;
            }
            tensors->push_back(std::move(tensor));
        }
        if (!boundary_schema_logged && !tensors->empty()) {
            uint64_t logical_bytes = 0;
            uint64_t payload_bytes = 0;
            size_t aliases = 0;
            for (const auto & tensor : *tensors) {
                logical_bytes += tensor.desc.nbytes;
                payload_bytes += tensor.data.size();
                aliases += tensor.desc.alias_of >= 0 ? 1 : 0;
            }
            fprintf(stderr,
                    "ring boundary schema: tensors=%zu aliases=%zu logical_bytes=%llu payload_bytes=%llu\n",
                    tensors->size(), aliases, (unsigned long long) logical_bytes,
                    (unsigned long long) payload_bytes);
            boundary_schema_logged = true;
        }
        if (getenv("LINKCPP_RING_TRACE")) {
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
                fprintf(stderr,
                        "ring boundary local: index=%zu name=%s type=%d bytes=%llu alias=%d sum=%.9f norm=%.9f\n",
                        i, desc.name, desc.type,
                        (unsigned long long) desc.nbytes, desc.alias_of, sum, sqrt(sumsq));
            }
        }
        return true;
    }

    bool collect_terminals(boundary_bundle * tensors) const {
        const int count = llama_linkcpp_terminal_count(ctx);
        if (count <= 0 || count > MAX_BOUNDARY_TENSORS) return false;
        tensors->clear();
        tensors->reserve((size_t) count);
        for (int i = 0; i < count; ++i) {
            boundary_tensor tensor;
            if (!llama_linkcpp_terminal_desc(ctx, i, &tensor.desc)
                || tensor.desc.nbytes > MAX_FRAME_BYTES) return false;
            if (tensor.desc.alias_of < 0) {
                tensor.data.resize((size_t) tensor.desc.nbytes);
                if (!llama_linkcpp_terminal_get(ctx, i, tensor.data.data(), tensor.data.size())) return false;
            }
            tensors->push_back(std::move(tensor));
        }
        if (!terminal_schema_logged) {
            for (size_t i = 0; i < tensors->size(); ++i) {
                const auto & desc = (*tensors)[i].desc;
                fprintf(stderr,
                        "ring terminal schema: index=%zu name=%s type=%d bytes=%llu alias=%d\n",
                        i, desc.name, desc.type, (unsigned long long) desc.nbytes, desc.alias_of);
            }
            terminal_schema_logged = true;
        }
        if (getenv("LINKCPP_RING_TRACE")) {
            for (size_t i = 0; i < tensors->size(); ++i) {
                const auto & tensor = (*tensors)[i];
                if (tensor.desc.type == GGML_TYPE_F32
                    && strcmp(tensor.desc.name, "result_output") == 0
                    && !tensor.data.empty()) {
                    const float * values = reinterpret_cast<const float *>(tensor.data.data());
                    const size_t count = tensor.data.size() / sizeof(float);
                    const size_t argmax = (size_t) (std::max_element(values, values + count) - values);
                    fprintf(stderr, "ring terminal logits: values=%zu argmax=%zu max=%.9f\n",
                            count, argmax, values[argmax]);
                }
            }
        }
        return true;
    }

    bool set_boundary(const boundary_bundle & tensors) {
        if (tensors.empty() || tensors.size() > MAX_BOUNDARY_TENSORS) return false;
        llama_linkcpp_input_clear(ctx);
        for (size_t i = 0; i < tensors.size(); ++i) {
            const int32_t alias = tensors[i].desc.alias_of;
            const auto & data = alias >= 0 ? tensors[(size_t) alias].data : tensors[i].data;
            if (!llama_linkcpp_input_set_tensor(ctx, &tensors[i].desc,
                                                data.data(), data.size())) return false;
        }
        return true;
    }

    bool decode_token_bundle(int token, uint32_t pos, boundary_bundle * output) {
        llama_batch batch = llama_batch_init(1, 0, 1);
        batch.n_tokens = 1;
        batch.token[0] = token;
        batch.pos[0] = pos;
        batch.n_seq_id[0] = 1;
        batch.seq_id[0][0] = 0;
        batch.logits[0] = 1;
        const int rc = llama_decode(ctx, batch);
        llama_batch_free(batch);
        if (rc != 0) {
            fprintf(stderr, "first-stage decode failed: rc=%d\n", rc);
            return false;
        }
        return collect_boundary(output)
            && (last_layer == llama_model_n_layer(model) || !output->empty());
    }

    bool encode_tokens_bundle(const std::vector<llama_token> & tokens, boundary_bundle * output) {
        if (tokens.empty() || tokens.size() > (size_t) context_size) return false;
        llama_batch batch = llama_batch_init((int32_t) tokens.size(), 0, 1);
        batch.n_tokens = (int32_t) tokens.size();
        for (int32_t i = 0; i < batch.n_tokens; ++i) {
            batch.token[i] = tokens[(size_t) i];
            batch.pos[i] = i;
            batch.n_seq_id[i] = 1;
            batch.seq_id[i][0] = 0;
            batch.logits[i] = 1;
        }
        const int rc = has_encoder ? llama_encode(ctx, batch) : llama_decode(ctx, batch);
        llama_batch_free(batch);
        return rc == 0 && collect_boundary(output)
            && (last_layer == llama_model_n_layer(model) || !output->empty());
    }

    bool decode_boundary(const boundary_bundle & input, uint32_t pos, boundary_bundle * output) {
        if (!set_boundary(input)) return false;
        llama_batch batch = llama_batch_init(1, n_embd, 1);
        batch.n_tokens = 1;
        memset(batch.embd, 0, sizeof(float) * n_embd);
        batch.pos[0] = pos;
        batch.n_seq_id[0] = 1;
        batch.seq_id[0][0] = 0;
        batch.logits[0] = 1;
        const int rc = llama_decode(ctx, batch);
        llama_batch_free(batch);
        if (rc != 0 || llama_linkcpp_input_count(ctx) != (int32_t) input.size()) return false;
        for (size_t i = 0; i < input.size(); ++i) {
            llama_linkcpp_tensor_desc expected{};
            if (!llama_linkcpp_input_desc(ctx, (int32_t) i, &expected)
                || expected.type != input[i].desc.type
                || expected.n_dims != input[i].desc.n_dims
                || expected.nbytes != input[i].desc.nbytes
                || expected.alias_of != input[i].desc.alias_of
                || memcmp(expected.ne, input[i].desc.ne, sizeof(expected.ne)) != 0
                || memcmp(expected.nb, input[i].desc.nb, sizeof(expected.nb)) != 0) return false;
        }
        return collect_boundary(output);
    }

    bool encode_boundary(const boundary_bundle & input, uint32_t n_tokens, boundary_bundle * output) {
        if (n_tokens == 0 || n_tokens > (uint32_t) context_size || !set_boundary(input)) return false;
        llama_batch batch = llama_batch_init((int32_t) n_tokens, n_embd, 1);
        batch.n_tokens = (int32_t) n_tokens;
        memset(batch.embd, 0, sizeof(float) * (size_t) n_embd * n_tokens);
        for (uint32_t i = 0; i < n_tokens; ++i) {
            batch.pos[i] = (llama_pos) i;
            batch.n_seq_id[i] = 1;
            batch.seq_id[i][0] = 0;
            batch.logits[i] = 1;
        }
        const int rc = has_encoder ? llama_encode(ctx, batch) : llama_decode(ctx, batch);
        llama_batch_free(batch);
        if (rc != 0 || llama_linkcpp_input_count(ctx) != (int32_t) input.size()) return false;
        return collect_boundary(output);
    }

    bool execute_boundary(const execute_bundle & request, boundary_bundle * output) {
        const auto & h = request.header;
        if (!set_boundary(request.tensors)) return false;
        std::vector<llama_token> tokens(h.n_tokens, 0);
        std::vector<llama_seq_id *> seq_ptrs(h.n_tokens);
        size_t seq_offset = 0;
        for (uint32_t i = 0; i < h.n_tokens; ++i) {
            seq_ptrs[i] = const_cast<llama_seq_id *>(request.seq_ids.data() + seq_offset);
            seq_offset += (size_t) request.n_seq_id[i];
        }
        llama_batch batch = {
            /*.n_tokens =*/ (int32_t) h.n_tokens,
            /*.token    =*/ tokens.data(),
            /*.embd     =*/ nullptr,
            /*.pos      =*/ const_cast<llama_pos *>(request.positions.data()),
            /*.n_seq_id =*/ const_cast<int32_t *>(request.n_seq_id.data()),
            /*.seq_id   =*/ seq_ptrs.data(),
            /*.logits   =*/ const_cast<int8_t *>(request.output.data()),
        };
        const bool encoder = (h.flags & LLAMA_LINKCPP_STAGE_FLAG_ENCODER) != 0;
        if ((h.flags & ~LLAMA_LINKCPP_STAGE_FLAG_ENCODER) != 0
            || (encoder && !has_encoder)) return false;
        const int rc = encoder ? llama_encode(ctx, batch) : llama_decode(ctx, batch);
        if (rc != 0 || llama_linkcpp_input_count(ctx) != (int32_t) request.tensors.size()) return false;
        return last_layer == llama_model_n_layer(model)
            ? collect_terminals(output)
            : collect_boundary(output);
    }

    bool apply_memory(const memory_op_wire & op) {
        if (op.version != 1 || op.reserved
            || op.op < LLAMA_LINKCPP_MEMORY_CLEAR
            || op.op > LLAMA_LINKCPP_MEMORY_SEQ_DIV) return false;
        if (getenv("LINKCPP_RING_TRACE")) {
            fprintf(stderr, "ring memory apply: op=%u src=%d dst=%d p=[%d,%d) value=%d\n",
                    op.op, op.seq_id_src, op.seq_id_dst, op.p0, op.p1, op.value);
        }
        llama_memory_t memory = llama_get_memory(ctx);
        switch (op.op) {
            case LLAMA_LINKCPP_MEMORY_CLEAR:
                if (op.value != 0 && op.value != 1) return false;
                llama_memory_clear(memory, op.value != 0);
                return true;
            case LLAMA_LINKCPP_MEMORY_SEQ_RM:
                return llama_memory_seq_rm(memory, op.seq_id_src, op.p0, op.p1);
            case LLAMA_LINKCPP_MEMORY_SEQ_CP:
                llama_memory_seq_cp(memory, op.seq_id_src, op.seq_id_dst, op.p0, op.p1);
                return true;
            case LLAMA_LINKCPP_MEMORY_SEQ_KEEP:
                llama_memory_seq_keep(memory, op.seq_id_src);
                return true;
            case LLAMA_LINKCPP_MEMORY_SEQ_ADD:
                llama_memory_seq_add(memory, op.seq_id_src, op.p0, op.p1, op.value);
                return true;
            case LLAMA_LINKCPP_MEMORY_SEQ_DIV:
                if (op.value <= 1) return false;
                llama_memory_seq_div(memory, op.seq_id_src, op.p0, op.p1, op.value);
                return true;
        }
        return false;
    }

    bool get_sequence_state(int32_t seq_id, uint32_t flags, std::vector<uint8_t> * data) {
        if (!data || (flags & LLAMA_STATE_SEQ_FLAGS_ON_DEVICE)) return false;
        const auto state_flags = (llama_state_seq_flags) flags;
        const size_t size = llama_state_seq_get_size_ext(ctx, seq_id, state_flags);
        if (size == 0) return false;
        data->resize(size);
        return llama_state_seq_get_data_ext(ctx, data->data(), data->size(), seq_id, state_flags) == size;
    }

    bool set_sequence_state(int32_t seq_id, uint32_t flags, const std::vector<uint8_t> & data) {
        if (data.empty() || (flags & LLAMA_STATE_SEQ_FLAGS_ON_DEVICE)) return false;
        return llama_state_seq_set_data_ext(
            ctx, data.data(), data.size(), seq_id, (llama_state_seq_flags) flags) == data.size();
    }

    bool decode_token(int token, uint32_t pos, std::vector<float> * hidden) {
        boundary_bundle output;
        if (!decode_token_bundle(token, pos, &output)) return false;
        auto it = std::find_if(output.begin(), output.end(), [&](const boundary_tensor & tensor) {
            return tensor.desc.type == GGML_TYPE_F32
                && tensor.desc.nbytes == (uint64_t) n_embd * sizeof(float);
        });
        if (it == output.end()) return false;
        const float * data = reinterpret_cast<const float *>(it->data.data());
        hidden->assign(data, data + n_embd);
        return true;
    }

    bool decode_hidden(const float * input, uint32_t pos, std::vector<float> * hidden) {
        boundary_tensor tensor;
        tensor.desc.type = GGML_TYPE_F32;
        tensor.desc.n_dims = 2;
        tensor.desc.ne[0] = n_embd;
        tensor.desc.ne[1] = 1;
        tensor.desc.ne[2] = tensor.desc.ne[3] = 1;
        tensor.desc.nb[0] = sizeof(float);
        tensor.desc.nb[1] = (uint64_t) n_embd * sizeof(float);
        tensor.desc.nb[2] = tensor.desc.nb[1];
        tensor.desc.nb[3] = tensor.desc.nb[2];
        tensor.desc.nbytes = (uint64_t) n_embd * sizeof(float);
        tensor.desc.alias_of = -1;
        tensor.data.assign(reinterpret_cast<const uint8_t *>(input),
                           reinterpret_cast<const uint8_t *>(input) + tensor.desc.nbytes);
        boundary_bundle output;
        if (!decode_boundary({std::move(tensor)}, pos, &output)) return false;
        if (last_layer == llama_model_n_layer(model)) return true;
        auto it = std::find_if(output.begin(), output.end(), [&](const boundary_tensor & item) {
            return item.desc.type == GGML_TYPE_F32
                && item.desc.nbytes == (uint64_t) n_embd * sizeof(float);
        });
        if (it == output.end()) return false;
        const float * data = reinterpret_cast<const float *>(it->data.data());
        hidden->assign(data, data + n_embd);
        return true;
    }

    int sample_argmax() const {
        float * logits = llama_get_logits_ith(ctx, 0);
        if (!logits) return -1;
        return (int) (std::max_element(logits, logits + n_vocab) - logits);
    }

    bool tokenize(const std::string & prompt, std::vector<llama_token> * tokens) const {
        const int count = -llama_tokenize(vocab, prompt.data(), prompt.size(), nullptr, 0, true, true);
        if (count <= 0) return false;
        tokens->resize(count);
        return llama_tokenize(vocab, prompt.data(), prompt.size(), tokens->data(), tokens->size(), true, true) == count;
    }

    bool token_piece(llama_token token, std::string * text) const {
        std::vector<char> buf(256);
        int size = llama_token_to_piece(vocab, token, buf.data(), (int32_t) buf.size(), 0, true);
        if (size < 0) {
            buf.resize((size_t) -size);
            size = llama_token_to_piece(vocab, token, buf.data(), (int32_t) buf.size(), 0, true);
        }
        if (size < 0) return false;
        text->append(buf.data(), (size_t) size);
        return true;
    }

    bool evaluate_embedding(const std::string & text, std::vector<float> * embedding) {
        std::vector<llama_token> tokens;
        if (!tokenize(text, &tokens) || tokens.size() > (size_t) context_size) return false;
        llama_batch batch = llama_batch_init((int32_t) tokens.size(), 0, 1);
        batch.n_tokens = (int32_t) tokens.size();
        for (int32_t i = 0; i < batch.n_tokens; ++i) {
            batch.token[i] = tokens[(size_t) i];
            batch.pos[i] = i;
            batch.n_seq_id[i] = 1;
            batch.seq_id[i][0] = 0;
            batch.logits[i] = 1;
        }
        const int rc = llama_model_has_encoder(model) ? llama_encode(ctx, batch) : llama_decode(ctx, batch);
        llama_batch_free(batch);
        if (rc != 0) return false;
        float * data = llama_get_embeddings_seq(ctx, 0);
        if (!data) data = llama_get_embeddings_ith(ctx, -1);
        if (!data) return false;
        const int n = llama_model_n_embd_out(model);
        embedding->assign(data, data + n);
        return true;
    }

    bool apply_chat_template(const std::vector<std::pair<std::string, std::string>> & source,
                             std::string * prompt) const {
        if (!chat_templates) return false;
        try {
            common_chat_templates_inputs inputs;
            inputs.add_generation_prompt = true;
            inputs.use_jinja = true;
            inputs.enable_thinking = false;
            for (const auto & item : source) {
                common_chat_msg message;
                message.role = item.first;
                message.content = item.second;
                inputs.messages.push_back(std::move(message));
            }
            auto params = common_chat_templates_apply(chat_templates.get(), inputs);
            if (params.prompt.empty() || params.prompt.size() > MAX_PROMPT_BYTES) return false;
            *prompt = std::move(params.prompt);
            return true;
        } catch (const std::exception & exc) {
            fprintf(stderr, "chat template application failed: %s\n", exc.what());
            return false;
        }
    }

    ~stage_engine() {
        if (ctx) llama_free(ctx);
        if (model) llama_model_free(model);
    }
};

bool exchange_hello(socket_handle prev_fd, socket_handle next_fd, const stage_engine & engine) {
    hello_payload sent = {(uint32_t) engine.n_embd, (uint32_t) engine.first_layer,
                          (uint32_t) engine.last_layer, LINKCPP_RING_ADAPTER_ABI,
                          LINKCPP_RING_BUILD_FINGERPRINT};
    if (!send_frame(next_fd, FRAME_HELLO, 0, 0, &sent, sizeof(sent))) return false;
    frame incoming;
    if (!recv_frame(prev_fd, &incoming) || incoming.kind != FRAME_HELLO
        || incoming.payload.size() != sizeof(hello_payload)) return false;
    hello_payload received{};
    memcpy(&received, incoming.payload.data(), sizeof(received));
    return received.n_embd == (uint32_t) engine.n_embd
        && received.adapter_abi == LINKCPP_RING_ADAPTER_ABI
        && received.build_fingerprint == LINKCPP_RING_BUILD_FINGERPRINT;
}

bool send_boundary(socket_handle fd, uint8_t kind, uint64_t request_id, uint32_t position,
                   const boundary_bundle & tensors, uint32_t sequence_id = 0) {
    std::vector<uint8_t> payload;
    return encode_boundary_bundle(tensors, &payload)
        && send_frame(fd, kind, request_id, position, payload.data(),
                      (uint32_t) payload.size(), sequence_id);
}

bool send_state_chunks(socket_handle fd, uint64_t request_id, const state_op_wire & base,
                       const std::vector<uint8_t> & data) {
    for (size_t offset = 0; offset < data.size();) {
        const size_t count = std::min(STATE_CHUNK_BYTES, data.size() - offset);
        state_op_wire wire = base;
        wire.total_size = data.size();
        wire.offset = offset;
        std::vector<uint8_t> payload(sizeof(wire) + count);
        memcpy(payload.data(), &wire, sizeof(wire));
        memcpy(payload.data() + sizeof(wire), data.data() + offset, count);
        if (!send_frame(fd, FRAME_STATE_CHUNK, request_id, 0,
                        payload.data(), (uint32_t) payload.size())) return false;
        offset += count;
    }
    return true;
}

bool handle_forward_stage(stage_engine & engine, socket_handle prev_fd, socket_handle next_fd, bool is_last) {
    frame incoming;
    boundary_bundle input;
    boundary_bundle output;
    state_op_wire pending_state{};
    std::vector<uint8_t> pending_state_data;
    bool pending_state_target = false;
    while (recv_frame(prev_fd, &incoming)) {
        if (incoming.kind == FRAME_RESET) {
            engine.reset();
            if (!send_frame(next_fd, is_last ? FRAME_ACK : FRAME_RESET,
                            incoming.request_id, incoming.position)) return false;
            continue;
        }
        if (incoming.kind == FRAME_MEMORY) {
            if (incoming.payload.size() != sizeof(memory_op_wire)) return false;
            memory_op_wire op{};
            memcpy(&op, incoming.payload.data(), sizeof(op));
            if (!engine.apply_memory(op)) return false;
            if (!send_frame(next_fd, is_last ? FRAME_ACK : FRAME_MEMORY,
                            incoming.request_id, incoming.position,
                            is_last ? nullptr : &op,
                            is_last ? 0u : (uint32_t) sizeof(op))) return false;
            continue;
        }
        if (incoming.kind == FRAME_STATE_GET) {
            if (incoming.payload.size() != sizeof(state_op_wire)) return false;
            state_op_wire wire{};
            memcpy(&wire, incoming.payload.data(), sizeof(wire));
            if (wire.version != 1 || wire.status || wire.total_size || wire.offset
                || wire.layer_begin || wire.layer_end) return false;
            std::vector<uint8_t> state;
            if (!engine.get_sequence_state(wire.seq_id, wire.flags, &state)) return false;
            if (getenv("LINKCPP_RING_TRACE")) {
                fprintf(stderr, "ring state capture: seq=%d flags=%u layers=[%d,%d) bytes=%zu\n",
                        wire.seq_id, wire.flags, engine.first_layer, engine.last_layer, state.size());
            }
            wire.layer_begin = engine.first_layer;
            wire.layer_end = engine.last_layer;
            wire.status = 1;
            if (!send_state_chunks(next_fd, incoming.request_id, wire, state)) return false;
            if (is_last) {
                if (!send_frame(next_fd, FRAME_STATE_DONE, incoming.request_id, 0,
                                &wire, sizeof(wire))) return false;
            } else if (!send_frame(next_fd, FRAME_STATE_GET, incoming.request_id, 0,
                                   incoming.payload.data(), (uint32_t) incoming.payload.size())) return false;
            continue;
        }
        if (incoming.kind == FRAME_STATE_CHUNK) {
            if (incoming.payload.size() < sizeof(state_op_wire)) return false;
            if (!send_frame(next_fd, FRAME_STATE_CHUNK, incoming.request_id, 0,
                            incoming.payload.data(), (uint32_t) incoming.payload.size())) return false;
            continue;
        }
        if (incoming.kind == FRAME_STATE_SET_BEGIN) {
            if (incoming.payload.size() != sizeof(state_op_wire)) return false;
            memcpy(&pending_state, incoming.payload.data(), sizeof(pending_state));
            if (pending_state.version != 1 || pending_state.offset
                || pending_state.total_size > SIZE_MAX) return false;
            pending_state_target = pending_state.layer_begin == engine.first_layer
                && pending_state.layer_end == engine.last_layer;
            pending_state_data.clear();
            if (pending_state_target) pending_state_data.reserve((size_t) pending_state.total_size);
            if (!is_last && !send_frame(next_fd, FRAME_STATE_SET_BEGIN, incoming.request_id, 0,
                                        &pending_state, sizeof(pending_state))) return false;
            continue;
        }
        if (incoming.kind == FRAME_STATE_SET_CHUNK) {
            if (incoming.payload.size() < sizeof(state_op_wire)) return false;
            state_op_wire wire{};
            memcpy(&wire, incoming.payload.data(), sizeof(wire));
            const size_t count = incoming.payload.size() - sizeof(wire);
            if (wire.version != 1 || wire.total_size != pending_state.total_size
                || wire.layer_begin != pending_state.layer_begin
                || wire.layer_end != pending_state.layer_end
                || wire.offset > wire.total_size || count > wire.total_size - wire.offset) return false;
            if (pending_state_target) {
                if (wire.offset != pending_state_data.size()) return false;
                pending_state_data.insert(pending_state_data.end(),
                    incoming.payload.begin() + sizeof(wire), incoming.payload.end());
            }
            if (!is_last && !send_frame(next_fd, FRAME_STATE_SET_CHUNK, incoming.request_id, 0,
                                        incoming.payload.data(), (uint32_t) incoming.payload.size())) return false;
            continue;
        }
        if (incoming.kind == FRAME_STATE_SET_COMMIT) {
            if (incoming.payload.size() != sizeof(state_op_wire)) return false;
            state_op_wire wire{};
            memcpy(&wire, incoming.payload.data(), sizeof(wire));
            if (wire.version != 1 || wire.layer_begin != pending_state.layer_begin
                || wire.layer_end != pending_state.layer_end
                || wire.total_size != pending_state.total_size) return false;
            if (pending_state_target) {
                wire.status = pending_state_data.size() == (size_t) wire.total_size
                    && engine.set_sequence_state(wire.seq_id, wire.flags, pending_state_data) ? 1u : 0u;
                wire.offset = 1; // target matched
                if (getenv("LINKCPP_RING_TRACE")) {
                    fprintf(stderr, "ring state restore: seq=%d flags=%u layers=[%d,%d) bytes=%zu status=%u\n",
                            wire.seq_id, wire.flags, engine.first_layer, engine.last_layer,
                            pending_state_data.size(), wire.status);
                }
            }
            pending_state_target = false;
            pending_state_data.clear();
            if (is_last) {
                if (!send_frame(next_fd, FRAME_STATE_ACK, incoming.request_id, 0,
                                &wire, sizeof(wire))) return false;
            } else if (!send_frame(next_fd, FRAME_STATE_SET_COMMIT, incoming.request_id, 0,
                                   &wire, sizeof(wire))) return false;
            continue;
        }
        if (incoming.kind == FRAME_EXECUTE) {
            execute_bundle request;
            if (!decode_execute_bundle(incoming.payload, &request)
                || !engine.execute_boundary(request, &output)) return false;
            if (is_last) {
                if (!send_boundary(next_fd, FRAME_EXECUTE_RESULT, incoming.request_id,
                                   incoming.position, output, incoming.sequence_id)) return false;
            } else {
                std::vector<uint8_t> payload;
                if (!encode_execute_bundle(request, output, &payload)
                    || !send_frame(next_fd, FRAME_EXECUTE, incoming.request_id,
                                   incoming.position, payload.data(), (uint32_t) payload.size(),
                                   incoming.sequence_id)) return false;
            }
            continue;
        }
        if (incoming.kind == FRAME_EMBED) {
            if (!decode_boundary_bundle(incoming.payload, &input)
                || !engine.encode_boundary(input, incoming.sequence_id, &output)) return false;
            if (!is_last) {
                if (!send_boundary(next_fd, FRAME_EMBED, incoming.request_id, 0,
                                   output, incoming.sequence_id)) return false;
            } else {
                boundary_bundle terminals;
                if (!engine.collect_terminals(&terminals)
                    || !send_boundary(next_fd, FRAME_TERMINALS, incoming.request_id, 0,
                                      terminals, incoming.sequence_id)) return false;
            }
            continue;
        }
        if (incoming.kind != FRAME_PREFILL && incoming.kind != FRAME_DECODE) return false;
        if (!decode_boundary_bundle(incoming.payload, &input)
            || !engine.decode_boundary(input, incoming.position, &output)) return false;
        if (!is_last) {
            if (!send_boundary(next_fd, incoming.kind, incoming.request_id, incoming.position, output)) return false;
        } else if (incoming.kind == FRAME_PREFILL) {
            if (!send_frame(next_fd, FRAME_ACK, incoming.request_id, incoming.position)) return false;
        } else {
            const int32_t token = engine.sample_argmax();
            if (token < 0 || !send_frame(next_fd, FRAME_TOKEN, incoming.request_id,
                                         incoming.position, &token, sizeof(token))) return false;
        }
    }
    return true;
}

bool receive_expected(socket_handle fd, uint8_t kind, uint64_t request_id, frame * incoming) {
    return recv_frame(fd, incoming) && incoming->kind == kind && incoming->request_id == request_id;
}

bool run_generation(stage_engine & engine, socket_handle prev_fd, socket_handle next_fd,
                    uint64_t request_id, const std::string & prompt, uint32_t max_tokens,
                    std::string * output, uint32_t * generated, std::string * error) {
    engine.reset();
    if (!send_frame(next_fd, FRAME_RESET, request_id, 0)) {
        *error = "failed to reset successor";
        return false;
    }
    frame incoming;
    if (!receive_expected(prev_fd, FRAME_ACK, request_id, &incoming)) {
        *error = "ring reset acknowledgement failed";
        return false;
    }
    std::vector<llama_token> tokens;
    if (!engine.tokenize(prompt, &tokens)) {
        *error = "prompt tokenization failed";
        return false;
    }
    boundary_bundle boundary;
    uint32_t position = 0;
    int32_t token = -1;
    for (size_t i = 0; i < tokens.size(); ++i, ++position) {
        if (!engine.decode_token_bundle(tokens[i], position, &boundary)) {
            *error = "first stage prompt decode failed";
            return false;
        }
        const bool final_prompt_token = i + 1 == tokens.size();
        if (!send_boundary(next_fd, final_prompt_token ? FRAME_DECODE : FRAME_PREFILL,
                           request_id, position, boundary)) {
            *error = "hidden-state send failed";
            return false;
        }
        if (final_prompt_token) {
            if (!receive_expected(prev_fd, FRAME_TOKEN, request_id, &incoming)
                || incoming.payload.size() != sizeof(token)) {
                *error = "token response failed";
                return false;
            }
            memcpy(&token, incoming.payload.data(), sizeof(token));
        } else if (!receive_expected(prev_fd, FRAME_ACK, request_id, &incoming)) {
            *error = "prefill acknowledgement failed";
            return false;
        }
    }
    *generated = 0;
    while (*generated < max_tokens) {
        if (llama_vocab_is_eog(engine.vocab, token)) break;
        if (!engine.token_piece(token, output)) {
            *error = "token decoding failed";
            return false;
        }
        ++*generated;
        if (*generated >= max_tokens) break;
        if (!engine.decode_token_bundle(token, position, &boundary)
            || !send_boundary(next_fd, FRAME_DECODE, request_id, position, boundary)) {
            *error = "decode hidden-state send failed";
            return false;
        }
        ++position;
        if (!receive_expected(prev_fd, FRAME_TOKEN, request_id, &incoming)
            || incoming.payload.size() != sizeof(token)) {
            *error = "decode token response failed";
            return false;
        }
        memcpy(&token, incoming.payload.data(), sizeof(token));
    }
    return true;
}

bool run_token_eval(stage_engine & engine, socket_handle prev_fd, socket_handle next_fd,
                    uint64_t request_id, const std::vector<llama_token> & tokens,
                    int32_t * sampled, std::string * error) {
    if (tokens.empty()) {
        *error = "token input is empty";
        return false;
    }
    engine.reset();
    if (!send_frame(next_fd, FRAME_RESET, request_id, 0)) {
        *error = "failed to reset successor";
        return false;
    }
    frame incoming;
    if (!receive_expected(prev_fd, FRAME_ACK, request_id, &incoming)) {
        *error = "ring reset acknowledgement failed";
        return false;
    }
    boundary_bundle boundary;
    for (size_t i = 0; i < tokens.size(); ++i) {
        if (!engine.decode_token_bundle(tokens[i], (uint32_t) i, &boundary)
            || !send_boundary(next_fd, i + 1 == tokens.size() ? FRAME_DECODE : FRAME_PREFILL,
                              request_id, (uint32_t) i, boundary)) {
            *error = "token-id hidden-state send failed";
            return false;
        }
        if (i + 1 == tokens.size()) {
            if (!receive_expected(prev_fd, FRAME_TOKEN, request_id, &incoming)
                || incoming.payload.size() != sizeof(*sampled)) {
                *error = "token-id result failed";
                return false;
            }
            memcpy(sampled, incoming.payload.data(), sizeof(*sampled));
        } else if (!receive_expected(prev_fd, FRAME_ACK, request_id, &incoming)) {
            *error = "token-id prefill acknowledgement failed";
            return false;
        }
    }
    return *sampled >= 0;
}

bool run_embedding(stage_engine & engine, socket_handle prev_fd, socket_handle next_fd,
                   uint64_t request_id, const std::string & text,
                   std::vector<float> * embedding, std::string * error) {
    engine.reset();
    if (!send_frame(next_fd, FRAME_RESET, request_id, 0)) {
        *error = "failed to reset successor";
        return false;
    }
    frame incoming;
    if (!receive_expected(prev_fd, FRAME_ACK, request_id, &incoming)) {
        *error = "ring reset acknowledgement failed";
        return false;
    }
    std::vector<llama_token> tokens;
    boundary_bundle boundary;
    if (!engine.tokenize(text, &tokens)
        || !engine.encode_tokens_bundle(tokens, &boundary)
        || !send_boundary(next_fd, FRAME_EMBED, request_id, 0, boundary, (uint32_t) tokens.size())) {
        *error = "first stage embedding encode failed";
        return false;
    }
    if (!receive_expected(prev_fd, FRAME_TERMINALS, request_id, &incoming)) {
        *error = "embedding terminal response failed";
        return false;
    }
    boundary_bundle terminals;
    if (!decode_boundary_bundle(incoming.payload, &terminals)) {
        *error = "invalid embedding terminal bundle";
        return false;
    }
    const uint64_t expected_bytes = (uint64_t) engine.n_embd_out * sizeof(float);
    auto matches_embedding = [&](const boundary_tensor & tensor) {
        return tensor.desc.alias_of < 0
            && tensor.desc.type == GGML_TYPE_F32
            && tensor.desc.nbytes == expected_bytes;
    };
    auto it = std::find_if(terminals.begin(), terminals.end(), [&](const boundary_tensor & tensor) {
        return matches_embedding(tensor)
            && std::string(tensor.desc.name).find("pooled") != std::string::npos;
    });
    if (it == terminals.end()) {
        it = std::find_if(terminals.begin(), terminals.end(), matches_embedding);
    }
    if (it == terminals.end()) {
        *error = "pooled embedding terminal not found";
        return false;
    }
    const float * values = reinterpret_cast<const float *>(it->data.data());
    embedding->assign(values, values + engine.n_embd_out);
    return true;
}

bool write_control_response(uint64_t request_id, uint8_t status, const std::string & text,
                            uint32_t tokens, uint32_t elapsed_ms) {
    control_response_header response = {
        {'L', 'K', 'R', '1'}, PROTOCOL_VERSION, status, 0, request_id,
        (uint32_t) text.size(), tokens, elapsed_ms, 0,
    };
    return fd_write_all(1, &response, sizeof(response))
        && (text.empty() || fd_write_all(1, text.data(), text.size()));
}

bool write_control_response_bytes(uint64_t request_id, uint8_t status, uint16_t flags,
                                  const void * data, uint32_t bytes, uint32_t values,
                                  uint32_t elapsed_ms) {
    control_response_header response = {
        {'L', 'K', 'R', '1'}, PROTOCOL_VERSION, status, flags, request_id,
        bytes, values, elapsed_ms, 0,
    };
    return fd_write_all(1, &response, sizeof(response))
        && (bytes == 0 || fd_write_all(1, data, bytes));
}

bool parse_chat_messages(const std::string & payload,
                         std::vector<std::pair<std::string, std::string>> * messages) {
    const auto * data = reinterpret_cast<const uint8_t *>(payload.data());
    size_t offset = 0;
    auto read_u32 = [&](uint32_t * value) {
        if (offset + sizeof(uint32_t) > payload.size()) return false;
        memcpy(value, data + offset, sizeof(uint32_t));
        offset += sizeof(uint32_t);
        return true;
    };
    uint32_t count = 0;
    if (!read_u32(&count) || count == 0 || count > 1024) return false;
    messages->clear();
    messages->reserve(count);
    for (uint32_t i = 0; i < count; ++i) {
        uint32_t role_size = 0;
        uint32_t content_size = 0;
        if (!read_u32(&role_size) || !read_u32(&content_size)
            || role_size == 0 || role_size > 64
            || offset + role_size + content_size > payload.size()) return false;
        std::string role(reinterpret_cast<const char *>(data + offset), role_size);
        offset += role_size;
        std::string content(reinterpret_cast<const char *>(data + offset), content_size);
        offset += content_size;
        messages->emplace_back(std::move(role), std::move(content));
    }
    return offset == payload.size();
}

bool serve_control_requests(stage_engine & engine, socket_handle prev_fd, socket_handle next_fd) {
#ifdef _WIN32
    _setmode(0, _O_BINARY);
    _setmode(1, _O_BINARY);
#endif
    control_request_header request{};
    while (fd_read_all(0, &request, sizeof(request))) {
        if (memcmp(request.magic, "LKC1", 4) != 0 || request.version != PROTOCOL_VERSION
            || (request.flags & ~(CONTROL_FLAG_CHAT_MESSAGES | CONTROL_FLAG_EMBEDDING
                                  | CONTROL_FLAG_TOKEN_IDS)) != 0
            || ((request.flags & CONTROL_FLAG_CHAT_MESSAGES) != 0)
                + ((request.flags & CONTROL_FLAG_EMBEDDING) != 0)
                + ((request.flags & CONTROL_FLAG_TOKEN_IDS) != 0) > 1
            || request.reserved != 0 || request.reserved2 != 0
            || request.request_id == 0 || request.prompt_bytes == 0
            || request.prompt_bytes > MAX_PROMPT_BYTES
            || request.max_tokens == 0 || request.max_tokens > 4096) {
            return false;
        }
        std::string payload(request.prompt_bytes, '\0');
        if (!payload.empty() && !fd_read_all(0, payload.data(), payload.size())) return false;
        std::string prompt = payload;
        if (request.flags & CONTROL_FLAG_CHAT_MESSAGES) {
            std::vector<std::pair<std::string, std::string>> messages;
            if (!parse_chat_messages(payload, &messages) || !engine.apply_chat_template(messages, &prompt)) {
                if (!write_control_response(request.request_id, 1, "chat template application failed", 0, 0)) {
                    return false;
                }
                continue;
            }
        }
        const auto started = std::chrono::steady_clock::now();
        if (request.flags & CONTROL_FLAG_TOKEN_IDS) {
            if (payload.size() % sizeof(int32_t) != 0
                || payload.size() / sizeof(int32_t) > (size_t) engine.context_size) {
                if (!write_control_response(request.request_id, 1, "invalid token-id payload", 0, 0)) {
                    return false;
                }
                continue;
            }
            std::vector<llama_token> tokens(payload.size() / sizeof(int32_t));
            memcpy(tokens.data(), payload.data(), payload.size());
            int32_t sampled = -1;
            std::string error;
            const bool ok = run_token_eval(
                engine, prev_fd, next_fd, request.request_id, tokens, &sampled, &error);
            const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                std::chrono::steady_clock::now() - started).count();
            if (ok) {
                if (!write_control_response_bytes(
                        request.request_id, 0, CONTROL_FLAG_TOKEN_IDS, &sampled, sizeof(sampled), 1,
                        (uint32_t) std::min<int64_t>(elapsed, UINT32_MAX))) return false;
            } else if (!write_control_response(
                           request.request_id, 1, error, 0,
                           (uint32_t) std::min<int64_t>(elapsed, UINT32_MAX))) {
                return false;
            }
            if (!ok) return false;
            continue;
        }
        if (request.flags & CONTROL_FLAG_EMBEDDING) {
            std::vector<float> embedding;
            std::string error;
            const bool ok = run_embedding(
                engine, prev_fd, next_fd, request.request_id, payload, &embedding, &error);
            const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                std::chrono::steady_clock::now() - started).count();
            if (ok) {
                if (!write_control_response_bytes(
                        request.request_id, 0, CONTROL_FLAG_EMBEDDING,
                        embedding.data(), (uint32_t) (embedding.size() * sizeof(float)),
                        (uint32_t) embedding.size(),
                        (uint32_t) std::min<int64_t>(elapsed, UINT32_MAX))) return false;
            } else if (!write_control_response(request.request_id, 1, error, 0,
                                               (uint32_t) std::min<int64_t>(elapsed, UINT32_MAX))) {
                return false;
            }
            fprintf(stderr,
                    "ring embedding id=%llu status=%s input_bytes=%u values=%zu elapsed_ms=%lld\n",
                    (unsigned long long) request.request_id, ok ? "ok" : "error",
                    request.prompt_bytes, embedding.size(), (long long) elapsed);
            if (!ok) return false;
            continue;
        }
        std::string output;
        std::string error;
        uint32_t generated = 0;
        const bool ok = run_generation(engine, prev_fd, next_fd, request.request_id, prompt,
                                       std::max<uint32_t>(1, request.max_tokens),
                                       &output, &generated, &error);
        const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
            std::chrono::steady_clock::now() - started).count();
        const std::string & body = ok ? output : error;
        if (!write_control_response(request.request_id, ok ? 0 : 1, body, generated,
                                    (uint32_t) std::min<int64_t>(elapsed, UINT32_MAX))) return false;
        fprintf(stderr, "ring request id=%llu status=%s prompt_bytes=%u output_bytes=%zu tokens=%u elapsed_ms=%lld\n",
                (unsigned long long) request.request_id, ok ? "ok" : "error",
                request.prompt_bytes, output.size(), generated, (long long) elapsed);
        if (!ok) return false;
    }
    return true;
}

int run_ring(const char * model, int first_layer, int last_layer, const std::string & role,
             int listen_port, const std::string & next, int gpu_layers, int n_ctx,
             enum llama_pooling_type pooling, bool embeddings, int parallel,
             enum ggml_type type_k, enum ggml_type type_v,
             bool kv_offload, const std::string & dial_prev, bool accept_next,
             int n_batch = 0, int n_ubatch = 0) {
    // NAT traversal: a stage behind NAT dials both neighbours (dial_prev set,
    // and next dialed as usual); a public neighbour of a NAT'd stage accepts the
    // edge it would normally dial (accept_next). Default (dial_prev empty,
    // accept_next false) is the original wiring: dial next, accept prev.
    const bool do_dial_next = !accept_next;
    const bool do_dial_prev = !dial_prev.empty();
    const int n_accepts = (do_dial_next ? 0 : 1) + (do_dial_prev ? 0 : 1);

    socket_handle listener = INVALID_SOCKET_HANDLE;
    if (n_accepts > 0) {
        listener = listen_on(listen_port);
        if (listener == INVALID_SOCKET_HANDLE) {
            fprintf(stderr, "cannot listen on %d\n", listen_port);
            return 1;
        }
        fprintf(stderr, "ring listener bound: role=%s port=%d accepts=%d\n", role.c_str(), listen_port, n_accepts);
    }
    stage_engine engine;
    if (!engine.init(model, first_layer, last_layer, gpu_layers, n_ctx,
                     pooling, embeddings, parallel, type_k, type_v,
                     kv_offload, n_batch, n_ubatch)) {
        fprintf(stderr, "stage model/context initialization failed\n");
        if (listener != INVALID_SOCKET_HANDLE) close_socket(listener);
        return 1;
    }
    fprintf(stderr, "ring model ready: role=%s arch=%s layers=[%d,%d) n_embd=%d\n",
            role.c_str(), engine.architecture.c_str(), first_layer, last_layer, engine.n_embd);

    socket_handle next_fd = INVALID_SOCKET_HANDLE;
    socket_handle prev_fd = INVALID_SOCKET_HANDLE;
    auto fail_wiring = [&](const char * msg) -> int {
        fprintf(stderr, "%s\n", msg);
        if (listener != INVALID_SOCKET_HANDLE) close_socket(listener);
        if (next_fd != INVALID_SOCKET_HANDLE) close_socket(next_fd);
        if (prev_fd != INVALID_SOCKET_HANDLE) close_socket(prev_fd);
        return 1;
    };
    // Outbound dials first (connect_to retries until the peer listens), each
    // announcing our role to the accepter.
    if (do_dial_next) {
        next_fd = connect_to(next);
        if (next_fd != INVALID_SOCKET_HANDLE) set_tcp_nodelay(next_fd);
        if (next_fd == INVALID_SOCKET_HANDLE || !send_role(next_fd, RING_ROLE_PRED))
            return fail_wiring(("cannot connect to successor " + next).c_str());
    }
    if (do_dial_prev) {
        prev_fd = connect_to(dial_prev);
        if (prev_fd != INVALID_SOCKET_HANDLE) set_tcp_nodelay(prev_fd);
        if (prev_fd == INVALID_SOCKET_HANDLE || !send_role(prev_fd, RING_ROLE_SUCC))
            return fail_wiring(("cannot connect to predecessor " + dial_prev).c_str());
    }
    // Then accept the remaining edges, disambiguating by the peer's role byte.
    for (int i = 0; i < n_accepts; ++i) {
        socket_handle fd = accept(listener, nullptr, nullptr);
        if (fd != INVALID_SOCKET_HANDLE) set_tcp_nodelay(fd);
        char r = 0;
        if (fd == INVALID_SOCKET_HANDLE || !recv_role(fd, &r))
            return fail_wiring("ring accept/role failed");
        if (r == RING_ROLE_PRED) prev_fd = fd; else next_fd = fd;
    }
    if (listener != INVALID_SOCKET_HANDLE) close_socket(listener);
    listener = INVALID_SOCKET_HANDLE;
    if (next_fd == INVALID_SOCKET_HANDLE || prev_fd == INVALID_SOCKET_HANDLE)
        return fail_wiring("incomplete ring wiring");

    if (!exchange_hello(prev_fd, next_fd, engine)) {
        fprintf(stderr, "ring hello failed\n");
        close_socket(prev_fd);
        close_socket(next_fd);
        return 1;
    }
    fprintf(stderr, "ring stage ready: role=%s layers=[%d,%d) prev<- next->%s\n",
            role.c_str(), first_layer, last_layer, next.c_str());
    const bool ok = role == "first"
        ? serve_control_requests(engine, prev_fd, next_fd)
        : handle_forward_stage(engine, prev_fd, next_fd, role == "last");
    close_socket(prev_fd);
    close_socket(next_fd);
    return ok ? 0 : 1;
}

enum ggml_type parse_stage_cache_type(const char * value, bool * ok) {
    *ok = true;
    if (value == nullptr || !*value)      return GGML_TYPE_F16;
    const std::string v = value;
    if (v == "f16")    return GGML_TYPE_F16;
    if (v == "bf16")   return GGML_TYPE_BF16;
    if (v == "q8_0")   return GGML_TYPE_Q8_0;
    if (v == "q5_1")   return GGML_TYPE_Q5_1;
    if (v == "q5_0")   return GGML_TYPE_Q5_0;
    if (v == "q4_1")   return GGML_TYPE_Q4_1;
    if (v == "q4_0")   return GGML_TYPE_Q4_0;
    if (v == "iq4_nl") return GGML_TYPE_IQ4_NL;
    *ok = false;
    return GGML_TYPE_F16;
}

enum llama_pooling_type parse_stage_pooling(const char * value, bool * ok) {
    *ok = true;
    if (value == nullptr || !*value) return LLAMA_POOLING_TYPE_UNSPECIFIED;
    const std::string v = value;
    if (v == "none") return LLAMA_POOLING_TYPE_NONE;
    if (v == "mean") return LLAMA_POOLING_TYPE_MEAN;
    if (v == "cls")  return LLAMA_POOLING_TYPE_CLS;
    if (v == "last") return LLAMA_POOLING_TYPE_LAST;
    if (v == "rank") return LLAMA_POOLING_TYPE_RANK;
    *ok = false;
    return LLAMA_POOLING_TYPE_UNSPECIFIED;
}
}

#include "stage_api.h"

extern "C" int linkcpp_stage_run(const linkcpp_stage_params * params) {
    if (params == nullptr || params->model_path == nullptr
        || params->layer_begin < 0 || params->layer_end <= params->layer_begin) return 2;
    const std::string role = params->role ? params->role : "";
    if (role != "first" && role != "middle" && role != "last") return 2;
    bool ok_k = false, ok_v = false, ok_pool = false;
    const enum ggml_type type_k = parse_stage_cache_type(params->cache_type_k, &ok_k);
    const enum ggml_type type_v = parse_stage_cache_type(params->cache_type_v, &ok_v);
    const enum llama_pooling_type pooling = parse_stage_pooling(params->pooling, &ok_pool);
    if (!ok_k || !ok_v || !ok_pool) return 2;
#ifdef _WIN32
    WSADATA wsa{};
    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) return 1;
#endif
    llama_backend_init();
    const int rc = run_ring(params->model_path, params->layer_begin, params->layer_end, role,
                            params->listen_port, params->next_endpoint ? params->next_endpoint : "",
                            params->gpu_layers, params->ctx > 0 ? params->ctx : 4096,
                            pooling, params->embeddings,
                            params->parallel > 0 ? params->parallel : 1, type_k, type_v,
                            params->kv_offload,
                            params->dial_prev_endpoint ? params->dial_prev_endpoint : "",
                            params->accept_next);
    llama_backend_free();
#ifdef _WIN32
    WSACleanup();
#endif
    return rc;
}

extern "C" const char * linkcpp_stage_runtime_info_json(void) {
    static char buffer[256];
    snprintf(buffer, sizeof(buffer),
             "{\"protocol\":\"%s\",\"adapter_abi\":%u,"
             "\"build_id\":\"%s\",\"state_snapshot\":true,"
             "\"chunked_state\":true}",
             LINKCPP_RING_PROTOCOL_NAME, LINKCPP_RING_ADAPTER_ABI,
             LINKCPP_RING_BUILD_ID_STRING);
    return buffer;
}

#ifndef LINKCPP_STAGE_NO_MAIN
int main(int argc, char ** argv) {
    if (argc == 2 && !strcmp(argv[1], "--runtime-info")) {
        printf("{\"protocol\":\"%s\",\"adapter_abi\":%u,"
               "\"build_id\":\"%s\",\"state_snapshot\":true,"
               "\"chunked_state\":true}\n",
               LINKCPP_RING_PROTOCOL_NAME, LINKCPP_RING_ADAPTER_ABI,
               LINKCPP_RING_BUILD_ID_STRING);
        return 0;
    }
    const char * model_path = nullptr;
    int layer_begin = -1;
    int layer_end = -1;
    int eval_token = -1;
    int listen_port = 0;
    int gpu_layers = -1;
    int n_ctx = 4096;
    int parallel = 1;
    int n_batch = 0;    // 0 => use n_ctx (legacy); the coordinator's ubatch bounds ring frames
    int n_ubatch = 0;   // 0 => use n_ctx (legacy); set to cap the compute-graph reserve for large ctx
    enum llama_pooling_type pooling = LLAMA_POOLING_TYPE_UNSPECIFIED;
    enum ggml_type type_k = GGML_TYPE_F16;
    enum ggml_type type_v = GGML_TYPE_F16;
    bool eval_zero_embd = false;
    bool embeddings = false;
    std::string eval_prompt;
    std::string eval_embedding;
    std::string role;
    std::string next;
    std::string dial_prev;      // NAT: dial the predecessor instead of accepting it
    bool accept_next = false;   // NAT-neighbour: accept the successor instead of dialing it
    bool kv_offload = true;
    for (int i = 1; i < argc; ++i) {
        if (!strcmp(argv[i], "--model") && i + 1 < argc) model_path = argv[++i];
        else if (!strcmp(argv[i], "--layers") && i + 1 < argc
                 && sscanf(argv[++i], "%d:%d", &layer_begin, &layer_end) == 2
                 && layer_begin >= 0 && layer_end > layer_begin) {}
        else if (!strcmp(argv[i], "--eval-token") && i + 1 < argc) eval_token = atoi(argv[++i]);
        else if (!strcmp(argv[i], "--eval-zero-embd")) eval_zero_embd = true;
        else if (!strcmp(argv[i], "--embeddings")) embeddings = true;
        else if (!strcmp(argv[i], "--eval-prompt") && i + 1 < argc) eval_prompt = argv[++i];
        else if (!strcmp(argv[i], "--eval-embedding") && i + 1 < argc) eval_embedding = argv[++i];
        else if (!strcmp(argv[i], "--role") && i + 1 < argc) role = argv[++i];
        else if (!strcmp(argv[i], "--listen") && i + 1 < argc) listen_port = atoi(argv[++i]);
        else if (!strcmp(argv[i], "--next") && i + 1 < argc) next = argv[++i];
        else if (!strcmp(argv[i], "--dial-prev") && i + 1 < argc) dial_prev = argv[++i];
        else if (!strcmp(argv[i], "--accept-next")) accept_next = true;
        else if (!strcmp(argv[i], "--no-kv-offload")) kv_offload = false;
        else if (!strcmp(argv[i], "--gpu-layers") && i + 1 < argc) gpu_layers = atoi(argv[++i]);
        else if (!strcmp(argv[i], "--ctx") && i + 1 < argc) n_ctx = atoi(argv[++i]);
        else if (!strcmp(argv[i], "--batch") && i + 1 < argc) n_batch = atoi(argv[++i]);
        else if (!strcmp(argv[i], "--ubatch") && i + 1 < argc) n_ubatch = atoi(argv[++i]);
        else if (!strcmp(argv[i], "--parallel") && i + 1 < argc) parallel = atoi(argv[++i]);
        else if ((!strcmp(argv[i], "--cache-type-k") || !strcmp(argv[i], "--cache-type-v"))
                 && i + 1 < argc) {
            enum ggml_type parsed = GGML_TYPE_COUNT;
            const std::string value = argv[++i];
            if      (value == "f16")    parsed = GGML_TYPE_F16;
            else if (value == "bf16")   parsed = GGML_TYPE_BF16;
            else if (value == "q8_0")   parsed = GGML_TYPE_Q8_0;
            else if (value == "q5_1")   parsed = GGML_TYPE_Q5_1;
            else if (value == "q5_0")   parsed = GGML_TYPE_Q5_0;
            else if (value == "q4_1")   parsed = GGML_TYPE_Q4_1;
            else if (value == "q4_0")   parsed = GGML_TYPE_Q4_0;
            else if (value == "iq4_nl") parsed = GGML_TYPE_IQ4_NL;
            else return 2;
            if (!strcmp(argv[i - 1], "--cache-type-k")) type_k = parsed;
            else type_v = parsed;
        }
        else if (!strcmp(argv[i], "--pooling") && i + 1 < argc) {
            const std::string value = argv[++i];
            if      (value == "none") pooling = LLAMA_POOLING_TYPE_NONE;
            else if (value == "mean") pooling = LLAMA_POOLING_TYPE_MEAN;
            else if (value == "cls")  pooling = LLAMA_POOLING_TYPE_CLS;
            else if (value == "last") pooling = LLAMA_POOLING_TYPE_LAST;
            else if (value == "rank") pooling = LLAMA_POOLING_TYPE_RANK;
            else return 2;
        }
        else {
            fprintf(stderr, "usage: %s --model FILE --layers BEGIN:END [--gpu-layers N] [--ctx N] [--eval-token ID|--eval-zero-embd|--eval-prompt TEXT|--eval-embedding TEXT|--role first|middle|last --listen PORT --next HOST:PORT]\n", argv[0]);
            return 2;
        }
    }
    if (!model_path || layer_begin < 0 || layer_end <= layer_begin || n_ctx <= 0
        || parallel <= 0
        || (eval_token >= 0 && eval_zero_embd)) return 2;
#ifdef _WIN32
    WSADATA wsa{};
    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) return 1;
#endif
    llama_backend_init();
    int rc = 0;
    if (!role.empty()) {
        if (role != "first" && role != "middle" && role != "last") rc = 2;
        else rc = run_ring(model_path, layer_begin, layer_end, role, listen_port, next,
                           gpu_layers, n_ctx, pooling, embeddings, parallel, type_k, type_v,
                           kv_offload, dial_prev, accept_next, n_batch, n_ubatch);
    } else {
        stage_engine engine;
        if (!engine.init(model_path, layer_begin, layer_end, gpu_layers, n_ctx,
                         pooling, embeddings || !eval_embedding.empty(), parallel,
                         type_k, type_v, kv_offload)) rc = 1;
        else if (!eval_embedding.empty()) {
            std::vector<float> embedding;
            const bool ok = engine.evaluate_embedding(eval_embedding, &embedding);
            boundary_bundle terminals;
            const bool terminals_ok = ok && engine.collect_terminals(&terminals);
            double sum = 0.0;
            double sumsq = 0.0;
            for (float value : embedding) {
                sum += value;
                sumsq += (double) value * value;
            }
            printf("stage embedding eval: values=%zu terminals=%zu sum=%.9f norm=%.9f\n",
                   embedding.size(), terminals.size(), sum, sqrt(sumsq));
            rc = ok && terminals_ok ? 0 : 1;
        } else if (!eval_prompt.empty()) {
            std::vector<llama_token> tokens;
            boundary_bundle output;
            engine.reset();
            bool ok = engine.tokenize(eval_prompt, &tokens);
            for (size_t i = 0; ok && i < tokens.size(); ++i) {
                ok = engine.decode_token_bundle(tokens[i], (uint32_t) i, &output);
            }
            boundary_bundle terminals;
            ok = ok && engine.collect_terminals(&terminals);
            std::string piece;
            const int token = ok ? engine.sample_argmax() : -1;
            ok = token >= 0 && engine.token_piece(token, &piece);
            printf("stage prompt eval: token=%d terminals=%zu piece=%s\n",
                   token, terminals.size(), ok ? piece.c_str() : "<missing>");
            rc = ok ? 0 : 1;
        } else if (eval_token >= 0 || eval_zero_embd) {
            std::vector<float> hidden;
            const bool ok = eval_zero_embd
                ? engine.decode_hidden(std::vector<float>(engine.n_embd, 0).data(), 0, &hidden)
                : engine.decode_token(eval_token, 0, &hidden);
            printf("stage eval %s: hidden=%s\n",
                   eval_zero_embd ? "embedding-input" : "token-input", ok ? "available" : "missing");
            rc = ok ? 0 : 1;
        }
    }
    llama_backend_free();
#ifdef _WIN32
    WSACleanup();
#endif
    return rc;
}
#endif  // LINKCPP_STAGE_NO_MAIN
