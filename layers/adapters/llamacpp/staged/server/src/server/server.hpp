#pragma once

#include <cstdint>
#include <filesystem>
#include <functional>
#include <string>
#include <vector>

#include "protocol.hpp"
#include "kv_bridge.hpp"
#include "runtime.hpp"
#include "transaction_store.hpp"

#ifdef P4_STAGED_WITH_LLAMA
#include "llama_stage_runtime.hpp"
#include "plan.hpp"
#endif

namespace staged::server {

struct Capabilities {
    bool llama_runtime = false;
    bool hop = false;
    bool kv = false;
#ifdef P4_STAGED_WITH_LLAMA
    CapabilityReport llama_options{};
#endif
};

using HopExecutor = std::function<bool(const protocol::SequencePayload &,
                                       protocol::HopPhase,
                                       protocol::SequencePayload *,
                                       std::string *)>;

class Session final {
public:
    explicit Session(
            Capabilities capabilities = {}
#ifdef P4_STAGED_WITH_LLAMA
            , llama_runtime::StageRuntime * llama_runtime = nullptr
#endif
            , HopExecutor hop_executor = {}
            , std::filesystem::path transaction_root = {}
            );

    [[nodiscard]] bool feed_plan(const std::vector<std::uint8_t> &bytes,
                                 std::string *error = nullptr);
    [[nodiscard]] bool plan_complete() const noexcept {
        return runtime_.stdin_plan().complete();
    }
    [[nodiscard]] protocol::Frame handle(const protocol::Frame &request,
                                         bool *close_after = nullptr);
    [[nodiscard]] runtime::State state() const noexcept { return runtime_.state(); }

private:
    [[nodiscard]] protocol::Frame handle_hello();
    [[nodiscard]] protocol::Frame handle_hop(const protocol::Frame &request);
    [[nodiscard]] protocol::Frame handle_logical_batch(const protocol::Frame &request);
    [[nodiscard]] protocol::Frame handle_physical_batch(const protocol::Frame &request);
    [[nodiscard]] protocol::Frame handle_physical_settle(const protocol::Frame &request);
    [[nodiscard]] protocol::Frame handle_physical_release(const protocol::Frame &request);
    [[nodiscard]] protocol::Frame handle_tokenize(const protocol::Frame &request);
    [[nodiscard]] bool execute_hop(const protocol::SequencePayload &input,
                                   protocol::HopPhase phase,
                                   protocol::SequencePayload *output,
                                   std::string *error) const;
    [[nodiscard]] protocol::Frame error(const std::string &message) const;
    [[nodiscard]] protocol::Frame status(protocol::Operation operation,
                                          const std::string &message) const;

    runtime::Runtime runtime_;
    runtime::TransactionStore transaction_store_;
    Capabilities capabilities_;
#ifdef P4_STAGED_WITH_LLAMA
    llama_runtime::StageRuntime * llama_runtime_ = nullptr;
#endif
    HopExecutor hop_executor_;
    std::uint64_t next_physical_execution_id_ = 1;
};

} // namespace staged::server
