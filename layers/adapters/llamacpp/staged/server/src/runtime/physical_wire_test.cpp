#include "physical_wire.hpp"

#include <cassert>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <vector>

namespace {

void u16(std::vector<std::uint8_t> & bytes, std::uint16_t value) {
    bytes.push_back(static_cast<std::uint8_t>(value));
    bytes.push_back(static_cast<std::uint8_t>(value >> 8U));
}
void u32(std::vector<std::uint8_t> & bytes, std::uint32_t value) {
    for (std::uint32_t index = 0; index < 4; ++index) {
        bytes.push_back(static_cast<std::uint8_t>(value >> (index * 8U)));
    }
}
void u64(std::vector<std::uint8_t> & bytes, std::uint64_t value) {
    for (std::uint32_t index = 0; index < 8; ++index) {
        bytes.push_back(static_cast<std::uint8_t>(value >> (index * 8U)));
    }
}
void string(std::vector<std::uint8_t> & bytes, const std::string & value) {
    const auto size = value.size();
    u16(bytes, static_cast<std::uint16_t>(size));
    bytes.insert(bytes.end(), value.begin(), value.end());
}

std::vector<std::uint8_t> rust_logical_fixture(
        const std::string & reply = "reply-1", const std::string & options = "{}") {
    std::vector<std::uint8_t> bytes{'P', '4', 'L', 'B'};
    u16(bytes, 4);
    u16(bytes, 0);
    u32(bytes, 1);
    u64(bytes, 1);
    u64(bytes, 1); // request incarnation follows load generation
    string(bytes, "request-1");
    string(bytes, std::string("pipeline-a\0request-1", 20));
    string(bytes, "pipeline-a");
    string(bytes, reply);
    u32(bytes, 3);
    bytes.push_back(1); // Decode
    bytes.push_back(1); // output
    u16(bytes, 0);
    u32(bytes, 9);
    u32(bytes, 500);
    u32(bytes, 4);
    u32(bytes, 42); // token i32 has the same little-endian representation
    u64(bytes, 0);
    u32(bytes, 0);
    u32(bytes, 0);
    string(bytes, options);
    return bytes;
}

// The fixture writers above are independent of the production encoder and
// deliberately permit 4097 bytes. The rejected bytes must reach the decoder;
// using encode_physical_set to construct this input would reject it first.
std::vector<std::uint8_t> literal_physical_fixture(
        const std::string & reply, const std::string & options) {
    std::vector<std::uint8_t> bytes{'P', '4', 'P', 'B'};
    u16(bytes, 4);
    u16(bytes, 0);
    u32(bytes, 1); // one capsule
    u64(bytes, 7); // execution ID
    u32(bytes, 1); // terminal
    u32(bytes, 0); // execution flags
    u32(bytes, 1); // tokens per sequence
    u32(bytes, 1); // sequences
    u32(bytes, 1); // unique sequences
    u32(bytes, 1); // position dimensions
    u32(bytes, 1); // rows
    u32(bytes, 1); // sequence IDs
    u32(bytes, 0); // tensors
    u32(bytes, 1); // outcomes
    u32(bytes, 9); // row position
    u32(bytes, 1); // row sequence count
    u32(bytes, 3); // row sequence ID
    bytes.push_back(1); // output
    // LB and PB carry the same owner fields, in the same order. Strip only
    // the literal LB header, not any bytes produced by a native encoder.
    const auto logical = rust_logical_fixture(reply, options);
    bytes.insert(bytes.end(), logical.begin() + 12, logical.end());
    u32(bytes, 0); // outcome owner index
    u32(bytes, 1); // generated tokens
    u32(bytes, 0); // proposal tokens
    u32(bytes, 0); // replay tokens
    u32(bytes, 0xffffffffU); // retain_from = -1
    u32(bytes, 0); // replay position
    u32(bytes, 99); // sampled token
    u32(bytes, 10); // sampled position
    string(bytes, "ok");
    string(bytes, "eos");
    return bytes;
}

std::string json_bytes(std::size_t size, bool multibyte) {
    // Both inputs are complete JSON strings. This tests wire byte limits,
    // not the model-dependent request-options grammar. The byte escapes also
    // keep this source independent of MSVC's active code page.
    std::string value = "{\"text\":\"";
    const std::string suffix = "\"}";
    assert(size >= value.size() + suffix.size());
    auto remaining = size - value.size() - suffix.size();
    if (multibyte) {
        while (remaining >= 3) {
            value += "\xEA\xB0\x80"; // U+AC00: exactly three UTF-8 bytes
            remaining -= 3;
        }
    }
    value.append(remaining, 'a');
    value += suffix;
    assert(value.size() == size);
    return value;
}

void row_string_byte_boundaries() {
    using namespace staged::llama_runtime;
    std::size_t checked = 0;
    for (const auto multibyte : {false, true}) {
        for (const auto reply_field : {false, true}) {
            for (const std::size_t size : {4095U, 4096U, 4097U}) {
                const auto value = json_bytes(size, multibyte);
                const auto reply = reply_field ? value : std::string("reply-1");
                const auto options = reply_field ? std::string("{}") : value;
                const bool accepted = size <= 4096;
                std::string error;
                std::vector<LogicalExecutionRow> logical;
                assert(decode_logical_batch(rust_logical_fixture(reply, options),
                    &logical, &error) == accepted);
                if (accepted) {
                    assert(logical.size() == 1);
                    assert(logical[0].owner.reply == reply);
                    assert(logical[0].owner.options == options);
                }
                std::vector<RoutedPhysicalExecution> physical;
                const auto literal = literal_physical_fixture(reply, options);
                assert(decode_physical_set(literal, &physical, &error) == accepted);
                if (accepted) {
                    assert(physical.size() == 1);
                    assert(physical[0].owners[0].reply == reply);
                    assert(physical[0].owners[0].options == options);
                    std::vector<std::uint8_t> encoded;
                    assert(encode_physical_set(physical, &encoded, &error));
                    assert(encoded == literal);
                } else {
                    // Exercise the encoder independently of its decoder's
                    // rejection, using an otherwise valid decoded capsule.
                    assert(decode_physical_set(literal_physical_fixture("reply-1", "{}"),
                        &physical, &error));
                    physical[0].owners[0].reply = reply;
                    physical[0].owners[0].options = options;
                    std::vector<std::uint8_t> encoded;
                    assert(!encode_physical_set(physical, &encoded, &error));
                }
                ++checked;
            }
        }
    }
    // Defaults remain allowed: early Rust admission must not turn the size
    // guard into a new requirement for non-empty options.
    std::string error;
    std::vector<LogicalExecutionRow> logical;
    std::vector<RoutedPhysicalExecution> physical;
    assert(decode_logical_batch(rust_logical_fixture("reply-1", ""), &logical, &error));
    assert(logical[0].owner.options.empty());
    assert(decode_physical_set(literal_physical_fixture("reply-1", ""), &physical, &error));
    assert(physical[0].owners[0].options.empty());
    assert(checked == 12);
    std::cout << "row-string boundary cases=" << checked
              << "; LB/PB decode and PB encode; empty options retained\n";
}

staged::llama_runtime::RoutedPhysicalExecution capsule() {
    using namespace staged::llama_runtime;
    RoutedPhysicalExecution result;
    result.execution_id = 7;
    result.execution.n_seq_tokens = 1;
    result.execution.n_seqs = 1;
    result.execution.n_seqs_unq = 1;
    result.execution.n_pos = 1;
    result.execution.positions = {9};
    result.execution.sequence_counts = {1};
    result.execution.sequence_ids = {3};
    result.execution.output = {1};
    PhysicalTensor tensor;
    tensor.descriptor.type = 0;
    tensor.descriptor.n_dims = 2;
    tensor.descriptor.ne[0] = 2;
    tensor.descriptor.ne[1] = 1;
    tensor.descriptor.nb[0] = 4;
    tensor.descriptor.nb[1] = 8;
    tensor.descriptor.nbytes = 8;
    tensor.descriptor.alias_of = -1;
    std::strcpy(tensor.descriptor.name, "cut.0");
    tensor.data.assign(8, 1);
    result.execution.tensors.push_back(std::move(tensor));
    PhysicalOwner owner;
    owner.load_generation = 1;
    owner.incarnation = 1;
    owner.request_id = "request-1";
    owner.sequence_key = std::string("pipeline-a\0request-1", 20);
    owner.session_id = "pipeline-a";
    owner.reply = "reply-1";
    owner.sequence_id = 3;
    owner.phase = PhysicalPhase::Decode;
    owner.position = 9;
    owner.max_tokens = 500;
    owner.generated_tokens = 4;
    owner.output = true;
    owner.input_token = 42;
    owner.options = "{}";
    result.owners.push_back(std::move(owner));
    return result;
}

} // namespace

int main() {
    using namespace staged::llama_runtime;
    row_string_byte_boundaries();
    std::string error;
    std::vector<LogicalExecutionRow> logical;
    assert(decode_logical_batch(rust_logical_fixture(), &logical, &error));
    assert(logical.size() == 1);
    assert(logical[0].token == 42);
    assert(logical[0].owner.options == "{}");
    assert(logical[0].owner.phase == PhysicalPhase::Decode);
    assert(logical[0].owner.incarnation == 1);
    auto legacy = rust_logical_fixture();
    legacy[4] = 3;
    assert(!decode_logical_batch(legacy, &logical, &error));
    auto zero_incarnation = rust_logical_fixture();
    zero_incarnation[20] = 0;
    assert(!decode_logical_batch(zero_incarnation, &logical, &error));
    auto wrong_request = rust_logical_fixture();
    wrong_request[30] = 'x'; // request_id no longer equals the canonical key suffix.
    assert(!decode_logical_batch(wrong_request, &logical, &error));
    auto wrong_prefix = rust_logical_fixture();
    wrong_prefix[41] = 'x'; // first key byte, after request's u16 length + nine bytes.
    assert(!decode_logical_batch(wrong_prefix, &logical, &error));
    wrong_prefix[41] = 0xff;
    assert(!decode_logical_batch(wrong_prefix, &logical, &error));

    std::vector<std::uint8_t> encoded;
    assert(encode_physical_set({capsule()}, &encoded, &error));
    std::vector<RoutedPhysicalExecution> decoded;
    assert(decode_physical_set(encoded, &decoded, &error));
    assert(decoded.size() == 1);
    assert(decoded[0].execution.positions == std::vector<llama_pos>{9});
    assert(decoded[0].execution.tensors[0].data == std::vector<std::uint8_t>(8, 1));
    assert(decoded[0].owners[0].sequence_key == std::string("pipeline-a\0request-1", 20));
    assert(decoded[0].owners[0].incarnation == 1);
    auto old_physical = encoded;
    old_physical[4] = 3;
    assert(!decode_physical_set(old_physical, &decoded, &error));
    auto invalid_owner = capsule();
    invalid_owner.owners[0].incarnation = 0;
    assert(!encode_physical_set({invalid_owner}, &encoded, &error));
    invalid_owner = capsule();
    invalid_owner.owners[0].request_id = "request-2";
    assert(!encode_physical_set({invalid_owner}, &encoded, &error));

    auto terminal = capsule();
    terminal.execution.terminal = true;
    terminal.execution.tensors.clear();
    PhysicalOutcome outcome;
    outcome.owner_index = 0;
    outcome.generated.push_back({99, "ok", 10, "eos"});
    terminal.outcomes.push_back(std::move(outcome));
    assert(encode_physical_set({terminal}, &encoded, &error));
    assert(decode_physical_set(encoded, &decoded, &error));
    assert(decoded[0].outcomes[0].generated[0].token == 99);
    assert(decoded[0].outcomes[0].generated[0].stop == "eos");

    auto replay = capsule();
    replay.execution.terminal = true;
    replay.execution.tensors.clear();
    replay.owners[0].phase = PhysicalPhase::Verify;
    replay.owners[0].speculative_id = 17;
    replay.owners[0].speculative_count = 1;
    PhysicalOutcome replay_outcome;
    replay_outcome.owner_index = 0;
    replay_outcome.retain_from = 11;
    replay_outcome.replay_position = 9;
    replay_outcome.replay_tokens = {42, 99};
    replay.outcomes.push_back(std::move(replay_outcome));
    assert(encode_physical_set({replay}, &encoded, &error));
    assert(decode_physical_set(encoded, &decoded, &error));
    assert(decoded[0].outcomes[0].generated.empty());
    assert(decoded[0].outcomes[0].proposal.empty());
    assert(decoded[0].outcomes[0].replay_tokens
        == std::vector<llama_token>({42, 99}));
    return 0;
}
