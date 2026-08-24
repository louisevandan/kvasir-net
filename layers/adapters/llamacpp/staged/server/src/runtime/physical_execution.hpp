#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "llama.h"

namespace staged::llama_runtime {

struct LogicalRow final {
    llama_token token = 0;
    llama_pos position = 0;
    llama_seq_id sequence_id = 0;
    bool output = false;
};

struct PhysicalTensor final {
    llama_linkcpp_tensor_desc descriptor{};
    std::vector<std::uint8_t> data;
};

struct PhysicalExecution final {
    std::uint32_t flags = 0;
    std::uint32_t n_seq_tokens = 0;
    std::uint32_t n_seqs = 0;
    std::uint32_t n_seqs_unq = 0;
    std::uint32_t n_pos = 0;
    std::vector<llama_pos> positions;
    std::vector<std::int32_t> sequence_counts;
    std::vector<llama_seq_id> sequence_ids;
    std::vector<std::int8_t> output;
    std::vector<PhysicalTensor> tensors;
    bool terminal = false;
};

} // namespace staged::llama_runtime
