#include "physical_wire.hpp"

#include <cassert>
#include <cstdint>
#include <cstring>
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
void string(std::vector<std::uint8_t> & bytes, const char * value) {
    const auto size = std::strlen(value);
    u16(bytes, static_cast<std::uint16_t>(size));
    bytes.insert(bytes.end(), value, value + size);
}

std::vector<std::uint8_t> rust_logical_fixture() {
    std::vector<std::uint8_t> bytes{'P', '4', 'L', 'B'};
    u16(bytes, 2);
    u16(bytes, 0);
    u32(bytes, 1);
    string(bytes, "request-1");
    string(bytes, "sequence-1");
    string(bytes, "pipeline-a");
    string(bytes, "reply-1");
    u32(bytes, 3);
    bytes.push_back(1); // Decode
    bytes.push_back(1); // output
    u16(bytes, 0);
    u32(bytes, 9);
    u32(bytes, 500);
    u32(bytes, 4);
    u32(bytes, 42); // token i32 has the same little-endian representation
    string(bytes, "{}");
    return bytes;
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
    result.owners.push_back({"request-1", "sequence-1", "pipeline-a", "reply-1", 3,
                             PhysicalPhase::Decode, 9, 500, 4, true, "{}"});
    return result;
}

} // namespace

int main() {
    using namespace staged::llama_runtime;
    std::string error;
    std::vector<LogicalExecutionRow> logical;
    assert(decode_logical_batch(rust_logical_fixture(), &logical, &error));
    assert(logical.size() == 1);
    assert(logical[0].token == 42);
    assert(logical[0].owner.options == "{}");
    assert(logical[0].owner.phase == PhysicalPhase::Decode);

    std::vector<std::uint8_t> encoded;
    assert(encode_physical_set({capsule()}, &encoded, &error));
    std::vector<RoutedPhysicalExecution> decoded;
    assert(decode_physical_set(encoded, &decoded, &error));
    assert(decoded.size() == 1);
    assert(decoded[0].execution.positions == std::vector<llama_pos>{9});
    assert(decoded[0].execution.tensors[0].data == std::vector<std::uint8_t>(8, 1));
    assert(decoded[0].owners[0].sequence_key == "sequence-1");

    auto terminal = capsule();
    terminal.execution.terminal = true;
    terminal.execution.tensors.clear();
    terminal.outcomes.push_back({0, 99, "ok", 10, "eos"});
    assert(encode_physical_set({terminal}, &encoded, &error));
    assert(decode_physical_set(encoded, &decoded, &error));
    assert(decoded[0].outcomes[0].token == 99);
    assert(decoded[0].outcomes[0].stop == "eos");
    return 0;
}
