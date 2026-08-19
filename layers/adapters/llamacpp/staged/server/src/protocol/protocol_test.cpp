#include "protocol.hpp"

#include <cassert>
#include <cstdint>
#include <vector>

using staged::protocol::ErrorCode;
using staged::protocol::Frame;
using staged::protocol::Operation;
using staged::protocol::ProtocolError;
using staged::protocol::ProtocolLimits;
using staged::protocol::WireType;
using staged::protocol::wire_type_from_byte;
using staged::protocol::Descriptor;
using staged::protocol::SequencePayload;
using staged::protocol::HopPayload;
using staged::protocol::KvPayload;
using staged::protocol::KvResult;
using staged::protocol::KvReceipt;
using staged::protocol::KvReceiptState;
using staged::protocol::kKvPersist;

namespace {

template <typename Function>
void expects(Function &&function, ErrorCode code) {
    try {
        function();
        assert(false);
    } catch (const ProtocolError &error) {
        assert(error.code() == code);
    }
}

void round_trip_uses_little_endian_header() {
    const auto frame = Frame::make(Operation::Hop, {1, 2, 3, 4});
    const auto encoded = frame.encode(ProtocolLimits{});
    assert(encoded.size() == 16);
    assert(encoded[0] == 'L' && encoded[1] == 'C' && encoded[2] == 'P' && encoded[3] == '4');
    assert(encoded[4] == 1 && encoded[5] == 0);
    assert(encoded[6] == static_cast<std::uint8_t>(Operation::Hop));
    assert(encoded[8] == 4 && encoded[9] == 0 && encoded[10] == 0 && encoded[11] == 0);
    assert(Frame::decode(encoded, ProtocolLimits{}).body == frame.body);
}

void rejects_malformed_headers() {
    auto unknown = Frame::make(Operation::Hop, {}).encode(ProtocolLimits{});
    unknown[6] = 99;
    expects([&] { Frame::decode(unknown, ProtocolLimits{}); }, ErrorCode::UnknownOperation);

    auto bad_length = Frame::make(Operation::Hop, {1, 2}).encode(ProtocolLimits{});
    bad_length[8] = 3;
    expects([&] { Frame::decode(bad_length, ProtocolLimits{}); }, ErrorCode::LengthMismatch);

    auto bad_flags = Frame::make(Operation::Hop, {}).encode(ProtocolLimits{});
    bad_flags[7] = 1;
    expects([&] { Frame::decode(bad_flags, ProtocolLimits{}); }, ErrorCode::ReservedFlags);
}

void enforces_limits() {
    const auto frame = Frame::make(Operation::Hop, std::vector<std::uint8_t>(16));
    ProtocolLimits limits;
    limits.max_frame_bytes = 20;
    expects([&] { frame.encode(limits); }, ErrorCode::FrameTooLarge);
}

void validates_wire_types() {
    assert(wire_type_from_byte(1) == WireType::F32);
    assert(wire_type_from_byte(255) == WireType::Bytes);
    expects([&] { wire_type_from_byte(99); }, ErrorCode::UnknownWireType);
}

Descriptor descriptor(const char *name, std::uint64_t nbytes, bool alias = false,
                      std::uint32_t alias_of = 0) {
    return Descriptor{WireType::F32, {nbytes}, {1}, nbytes, 0, alias, alias_of, 0, name};
}

void sequence_payload_matches_rust_wire() {
    SequencePayload payload{"seq-42", {descriptor("base", 4), descriptor("view", 4, true, 0)},
                            {std::vector<std::uint8_t>{1, 2, 3, 4}, std::nullopt}};
    const auto encoded = payload.encode(ProtocolLimits{});
    assert(encoded[0] == 6 && encoded[1] == 0 && encoded[2] == 0 && encoded[3] == 0);
    assert(encoded[4] == 's' && encoded[9] == '2');
    const auto decoded = SequencePayload::decode(encoded, ProtocolLimits{});
    assert(decoded.sequence_id == payload.sequence_id);
    assert(decoded.descriptors.size() == 2);
    assert(decoded.payloads[0] == payload.payloads[0]);
    assert(!decoded.payloads[1].has_value());
    const SequencePayload non_alias{
        "seq-42",
        {descriptor("base", 4), descriptor("view", 4)},
        {std::vector<std::uint8_t>{1, 2, 3, 4},
         std::vector<std::uint8_t>{1, 2, 3, 4}}};
    const auto non_alias_encoded = non_alias.encode(ProtocolLimits{});
    assert(encoded.size() < non_alias_encoded.size());
}

void rejects_invalid_sequence_payloads() {
    SequencePayload missing{"id", {descriptor("x", 2)}, {std::nullopt}};
    expects([&] { missing.encode(ProtocolLimits{}); }, ErrorCode::InvalidSequence);
    SequencePayload alias_payload{"id", {descriptor("x", 2), descriptor("y", 2, true, 0)}, {std::vector<std::uint8_t>{1, 2}, std::vector<std::uint8_t>{3}}};
    expects([&] { alias_payload.encode(ProtocolLimits{}); }, ErrorCode::InvalidSequence);
    SequencePayload bad_target{"id", {descriptor("x", 2, true, 4)}, {std::nullopt}};
    expects([&] { bad_target.encode(ProtocolLimits{}); }, ErrorCode::InvalidDescriptor);
}

void hop_payload_preserves_legacy_and_supports_multiple_sequences() {
    const auto first = SequencePayload{
        "seq-a", {descriptor("hidden-a", 4)},
        {std::vector<std::uint8_t>{1, 2, 3, 4}}};
    const auto second = SequencePayload{
        "seq-b", {descriptor("hidden-b", 4)},
        {std::vector<std::uint8_t>{5, 6, 7, 8}}};

    bool enveloped = true;
    const auto legacy = first.encode(ProtocolLimits{});
    const auto legacy_decoded = HopPayload::decode(legacy, ProtocolLimits{}, &enveloped);
    assert(!enveloped);
    assert(legacy_decoded.sequences.size() == 1);
    assert(legacy_decoded.sequences[0].sequence_id == "seq-a");

    const HopPayload multi{staged::protocol::HopPhase::Decode, {first, second}, false};
    const auto encoded = multi.encode(ProtocolLimits{});
    assert(encoded.size() >= HopPayload::kEnvelopeMagic.size());
    const auto decoded = HopPayload::decode(encoded, ProtocolLimits{}, &enveloped);
    assert(enveloped);
    assert(decoded.sequences.size() == 2);
    assert(decoded.sequences[0].sequence_id == "seq-a");
    assert(decoded.sequences[1].sequence_id == "seq-b");
    assert(decoded.sequences[1].payloads[0].value()[3] == 8);
}

void hop_payload_rejects_empty_and_trailing_envelopes() {
    expects([&] { (void)HopPayload{}.encode(ProtocolLimits{}); }, ErrorCode::InvalidSequence);
    const HopPayload multi{staged::protocol::HopPhase::Decode,
                           {SequencePayload{"seq", {}, {}, std::nullopt, std::nullopt}}, false};
    auto encoded = multi.encode(ProtocolLimits{});
    encoded.push_back(0xff);
    expects([&] { (void)HopPayload::decode(encoded, ProtocolLimits{}); },
            ErrorCode::InvalidSequence);
}

void hop_metadata_and_token_count_round_trip() {
    SequencePayload payload{
        "tail", {}, {}, 1u, std::nullopt, std::nullopt, 3u,
        SequencePayload::OutcomeMetadata{42, "hello", 4u, std::string("eos")}};
    payload.options = R"({"temperature":0,"grammar":"root ::= \"A\""})";
    const HopPayload hop{staged::protocol::HopPhase::Decode, {payload}, false};
    const auto encoded = hop.encode(ProtocolLimits{});
    const auto decoded = HopPayload::decode(encoded, ProtocolLimits{});
    assert(decoded.sequences[0].n_tokens == 1u);
    assert(decoded.sequences[0].position == 3u);
    assert(decoded.sequences[0].outcome.has_value());
    assert(decoded.sequences[0].outcome->token == 42);
    assert(decoded.sequences[0].outcome->text == "hello");
    assert(decoded.sequences[0].outcome->position == 4u);
    assert(decoded.sequences[0].outcome->stop.has_value());
    assert(*decoded.sequences[0].outcome->stop == "eos");
    assert(decoded.sequences[0].options == payload.options);

    const auto cut_set = payload.encode(ProtocolLimits{});
    const auto cut_decoded = SequencePayload::decode(cut_set, ProtocolLimits{});
    assert(cut_decoded.n_tokens == 1u);
}

void kv_payload_round_trips_and_rejects_unsafe_keys() {
    KvPayload payload{
        "sequence-1", "deployment-1", "model-fingerprint", 2, 8, 0,
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "operation-1"};
    const auto encoded = payload.encode(ProtocolLimits{});
    const auto decoded = KvPayload::decode(encoded, ProtocolLimits{});
    assert(decoded.sequence_id == payload.sequence_id);
    assert(decoded.cache_key == payload.cache_key);
    assert(decoded.expected_checksum == payload.expected_checksum);
    assert(decoded.operation_id == payload.operation_id);
    auto manifest = payload;
    manifest.build_identity = "build-a";
    manifest.runtime_identity = "runtime-a";
    manifest.context_identity = "context-a";
    manifest.kv_format = "K=1;V=1";
    manifest.token_position = 17;
    assert(manifest.encode(ProtocolLimits{}) == encoded);
    const KvResult result{
        payload.sequence_id, payload.cache_key, 42, payload.expected_checksum};
    assert(KvResult::decode(result.encode(ProtocolLimits{}), ProtocolLimits{}).bytes == 42);
    auto unsafe = payload;
    unsafe.cache_key = "../escape";
    expects([&] { (void)unsafe.encode(ProtocolLimits{}); }, ErrorCode::InvalidSequence);
}

void kv_receipt_round_trips() {
    const KvReceipt receipt{
        "operation-1", "sequence-1", "deployment-1", "model-fingerprint", 2, 8,
        kKvPersist, KvReceiptState::Prepared, 42,
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "prepared"};
    const auto decoded = KvReceipt::decode(receipt.encode(ProtocolLimits{}), ProtocolLimits{});
    assert(decoded.operation_id == receipt.operation_id);
    assert(decoded.state == KvReceiptState::Prepared);
    assert(decoded.bytes == 42);
}

} // namespace

int main() {
    round_trip_uses_little_endian_header();
    rejects_malformed_headers();
    enforces_limits();
    validates_wire_types();
    sequence_payload_matches_rust_wire();
    rejects_invalid_sequence_payloads();
    hop_payload_preserves_legacy_and_supports_multiple_sequences();
    hop_payload_rejects_empty_and_trailing_envelopes();
    kv_receipt_round_trips();
    hop_metadata_and_token_count_round_trip();
    kv_payload_round_trips_and_rejects_unsafe_keys();
    return 0;
}
