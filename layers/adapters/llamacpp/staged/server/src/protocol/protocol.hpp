#pragma once

#include <cstddef>
#include <cstdint>
#include <array>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

namespace staged::protocol {

inline constexpr std::uint16_t kProtocolRevision = 1;
inline constexpr std::size_t kHeaderBytes = 12;

enum class Operation : std::uint8_t {
    Hello = 1,
    Hop = 2,
    HopResult = 3,
    Cancel = 4,
    KvSave = 5,
    KvRestore = 6,
    KvDrop = 7,
    KvResult = 8,
    Unload = 9,
    Error = 10,
    KvPrepare = 11,
    KvCommit = 12,
    KvAbort = 13,
    KvReconcile = 14,
    KvReceipt = 15,
    LogicalBatch = 16,
    PhysicalBatch = 17,
    PhysicalResult = 18,
    Tokenize = 19,
    Tokenized = 20,
    PhysicalRelease = 21,
};

inline constexpr std::uint32_t kKvDirect = 0;
inline constexpr std::uint32_t kKvPersist = 1;
inline constexpr std::uint32_t kKvRestore = 2;
inline constexpr std::uint32_t kKvDiscard = 3;

enum class KvReceiptState : std::uint8_t {
    Absent = 0,
    Prepared = 1,
    Committed = 2,
    Aborted = 3,
    Inconsistent = 4,
    Committing = 5,
};

enum class WireType : std::uint8_t {
    F32 = 1,
    F16 = 2,
    Q8 = 3,
    Q4 = 4,
    Bytes = 255,
};

[[nodiscard]] WireType wire_type_from_byte(std::uint8_t value);

enum class ErrorCode {
    BadMagic,
    Truncated,
    FrameTooLarge,
    PayloadTooLarge,
    NameTooLong,
    UnsupportedRevision,
    UnknownOperation,
    UnknownWireType,
    ReservedFlags,
    InvalidDescriptor,
    InvalidSequence,
    TooManyDescriptors,
    PayloadLengthMismatch,
    LengthMismatch,
};

class ProtocolError final : public std::runtime_error {
public:
    ProtocolError(ErrorCode code, const std::string &message);

    [[nodiscard]] ErrorCode code() const noexcept { return code_; }

private:
    ErrorCode code_;
};

struct ProtocolLimits {
    // One F32 cut per token per sequence: a 5,000-token prompt at n_embd
    // 2,048 is 39 MiB for one sequence, so a ten-wide prefill window is
    // 391 MiB. At 128 MiB this refused every window past three sequences.
    // Two gibibytes stays inside the `uint32` wire length and matches the
    // Rust adapter and the outer P4 frame, so neither side refuses first.
    // Splitting a wide hop across several frames is the better answer and is
    // not built yet; this is the limit until it is.
    std::size_t max_frame_bytes = 2ULL * 1024 * 1024 * 1024;
    // A 5k-token prefill can produce one outbound cut descriptor per token.
    // Keep headroom for larger non-MTP context windows without changing the
    // fixed-width wire fields.
    std::size_t max_descriptors = 16384;
    std::size_t max_payload_bytes = 2ULL * 1024 * 1024 * 1024;
    std::size_t max_name_bytes = 4096;
};

struct FrameHeader {
    std::uint16_t revision = kProtocolRevision;
    Operation operation = Operation::Hello;
    std::uint32_t body_bytes = 0;
};

struct Frame {
    FrameHeader header;
    std::vector<std::uint8_t> body;

    static Frame make(Operation operation, std::vector<std::uint8_t> body);
    [[nodiscard]] std::vector<std::uint8_t> encode(const ProtocolLimits &limits) const;
    static Frame decode(const std::vector<std::uint8_t> &bytes,
                        const ProtocolLimits &limits);
};

struct Descriptor {
    WireType wire_type = WireType::Bytes;
    std::vector<std::uint64_t> dimensions;
    std::vector<std::uint64_t> strides;
    std::uint64_t nbytes = 0;
    std::uint64_t view_offset = 0;
    bool has_alias = false;
    std::uint32_t alias_of = 0;
    std::uint8_t flags = 0;
    std::string name;

    void validate(const ProtocolLimits &limits) const;
};

struct SequencePayload {
    std::string sequence_id;
    std::vector<Descriptor> descriptors;
    std::vector<std::optional<std::vector<std::uint8_t>>> payloads;
    // Explicit logical token count for stage cut-sets. This must not be
    // inferred from hidden tensor dimensions (rank-1 dims[0] is often n_embd).
    std::optional<std::uint32_t> n_tokens;
    std::optional<std::string> prompt;
    std::optional<std::vector<std::int32_t>> initial_tokens;
    // Optional P4 progress supplied by a new HMUX caller. Legacy sequence
    // bodies omit it and retain the old zero-based runtime fallback.
    std::optional<std::uint32_t> position;
    // Tail-only sampled result. It is deliberately optional so middle stages
    // and old payloads retain the tensor-only contract.
    struct OutcomeMetadata {
        std::optional<std::int32_t> token;
        std::string text;
        std::uint32_t position = 0;
        std::optional<std::string> stop;
    };
    std::optional<OutcomeMetadata> outcome;
    // Opaque request-level generation options. The staged runtime preserves
    // this value but does not interpret sampler JSON in this wire slice.
    std::string options;

    [[nodiscard]] std::vector<std::uint8_t> encode(const ProtocolLimits &limits) const;
    [[nodiscard]] std::vector<std::uint8_t> encode_v2(const ProtocolLimits &limits) const;
    static SequencePayload decode(const std::vector<std::uint8_t> &bytes,
                                  const ProtocolLimits &limits);
    static SequencePayload decode_v2(const std::vector<std::uint8_t> &bytes,
                                     const ProtocolLimits &limits);
};

enum class HopPhase : std::uint8_t {
    Prefill = 0,
    Decode = 1,
};

// A one-sequence HOP remains the legacy SequencePayload wire. Multi-sequence
// HOPs use this envelope and length-delimit each legacy SequencePayload.
struct HopPayload {
    static constexpr std::array<std::uint8_t, 4> kEnvelopeMagic{'H', 'M', 'U', 'X'};
    static constexpr std::uint8_t kEnvelopeVersion = 2;

    HopPhase phase = HopPhase::Decode;
    std::vector<SequencePayload> sequences;
    bool legacy = false;

    [[nodiscard]] std::vector<std::uint8_t> encode(const ProtocolLimits &limits) const;
    static HopPayload decode(const std::vector<std::uint8_t> &bytes,
                             const ProtocolLimits &limits,
                             bool *enveloped = nullptr);
};

struct KvPayload {
    std::string sequence_id;
    std::string cache_key;
    std::string model_identity;
    std::int32_t stage_begin = 0;
    std::int32_t stage_end = 0;
    std::uint32_t flags = 0;
    std::string expected_checksum;
    // Optional trailing field for legacy direct KV; required by transactions.
    std::string operation_id;

    // Process-local manifest fields. They are not serialized by encode/decode;
    // the loaded runtime fills them before durable state is touched.
    std::string build_identity;
    std::string runtime_identity;
    std::string context_identity;
    std::string kv_format;
    std::uint64_t token_position = 0;

    [[nodiscard]] std::vector<std::uint8_t> encode(const ProtocolLimits &limits) const;
    static KvPayload decode(const std::vector<std::uint8_t> &bytes,
                            const ProtocolLimits &limits);
};

struct KvResult {
    std::string sequence_id;
    std::string cache_key;
    std::uint64_t bytes = 0;
    std::string checksum;

    [[nodiscard]] std::vector<std::uint8_t> encode(const ProtocolLimits &limits) const;
    static KvResult decode(const std::vector<std::uint8_t> &bytes,
                           const ProtocolLimits &limits);
};

struct KvReceipt {
    std::string operation_id;
    std::string sequence_id;
    std::string cache_key;
    std::string model_identity;
    std::int32_t stage_begin = 0;
    std::int32_t stage_end = 0;
    std::uint32_t kind = kKvDirect;
    KvReceiptState state = KvReceiptState::Absent;
    std::uint64_t bytes = 0;
    std::string checksum;
    std::string detail;

    [[nodiscard]] std::vector<std::uint8_t> encode(const ProtocolLimits &limits) const;
    static KvReceipt decode(const std::vector<std::uint8_t> &bytes,
                            const ProtocolLimits &limits);
};

} // namespace staged::protocol
