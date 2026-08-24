#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "physical_execution.hpp"

namespace staged::llama_runtime {

enum class PhysicalPhase : std::uint8_t { Prefill = 0, Decode = 1 };

struct PhysicalOwner final {
    std::string request_id;
    std::string sequence_key;
    std::string session_id;
    std::string reply;
    std::uint32_t sequence_id = 0;
    PhysicalPhase phase = PhysicalPhase::Prefill;
    std::uint32_t position = 0;
    std::uint32_t max_tokens = 0;
    std::uint32_t generated_tokens = 0;
    bool output = false;
    std::string options;
};

struct PhysicalOutcome final {
    std::uint32_t owner_index = 0;
    llama_token token = 0;
    std::string text;
    std::uint32_t position = 0;
    std::string stop;
};

struct LogicalExecutionRow final {
    PhysicalOwner owner;
    llama_token token = 0;
};

struct RoutedPhysicalExecution final {
    std::uint64_t execution_id = 0;
    PhysicalExecution execution;
    std::vector<PhysicalOwner> owners;
    std::vector<PhysicalOutcome> outcomes;
};

[[nodiscard]] bool decode_logical_batch(
    const std::vector<std::uint8_t> &, std::vector<LogicalExecutionRow> *,
    std::string * error = nullptr);
[[nodiscard]] bool decode_physical_set(
    const std::vector<std::uint8_t> &, std::vector<RoutedPhysicalExecution> *,
    std::string * error = nullptr);
[[nodiscard]] bool encode_physical_set(
    const std::vector<RoutedPhysicalExecution> &, std::vector<std::uint8_t> *,
    std::string * error = nullptr);

} // namespace staged::llama_runtime
