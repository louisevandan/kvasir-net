#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace staged::runtime {

enum class State {
    AwaitingPlan,
    PlanLoaded,
    Ready,
    HopActive,
    KvActive,
    Unloading,
    Unloaded,
    Failed,
};

enum class Operation {
    Hello,
    Hop,
    Cancel,
    KvSave,
    KvRestore,
    KvDrop,
    KvPrepare,
    KvCommit,
    KvAbort,
    KvReconcile,
    Unload,
};

enum class ErrorCode {
    None,
    PlanTooLarge,
    InvalidPlanLength,
    PlanIncomplete,
    UnexpectedEof,
    InvalidState,
    OperationInProgress,
    NoOperationInProgress,
    WrongOperation,
};

struct Result {
    State state;
    ErrorCode error = ErrorCode::None;

    [[nodiscard]] bool ok() const noexcept { return error == ErrorCode::None; }
};

class PlanReader final {
public:
    explicit PlanReader(std::size_t max_plan_bytes = 1024U * 1024U);

    // Feed arbitrary chunks. The pipe remains open after a complete plan.
    [[nodiscard]] Result feed(const std::uint8_t *bytes, std::size_t size);
    [[nodiscard]] Result eof();

    [[nodiscard]] bool complete() const noexcept { return complete_; }
    [[nodiscard]] bool failed() const noexcept { return failed_; }
    [[nodiscard]] const std::vector<std::uint8_t> &plan() const noexcept {
        return plan_;
    }

private:
    std::size_t max_plan_bytes_;
    std::vector<std::uint8_t> prefix_;
    std::vector<std::uint8_t> plan_;
    std::size_t expected_bytes_ = 0;
    bool length_known_ = false;
    bool complete_ = false;
    bool failed_ = false;
};

class Runtime final {
public:
    explicit Runtime(std::size_t max_plan_bytes = 1024U * 1024U);

    [[nodiscard]] PlanReader &stdin_plan() noexcept { return plan_reader_; }
    [[nodiscard]] const PlanReader &stdin_plan() const noexcept {
        return plan_reader_;
    }

    [[nodiscard]] Result hello();
    [[nodiscard]] Result begin_hop();
    [[nodiscard]] Result finish_hop();
    [[nodiscard]] Result cancel();
    [[nodiscard]] Result begin_kv(Operation operation);
    [[nodiscard]] Result finish_kv();
    [[nodiscard]] Result unload();
    [[nodiscard]] Result finish_unload();
    [[nodiscard]] Result stdin_eof();

    [[nodiscard]] State state() const noexcept { return state_; }
    [[nodiscard]] Operation active_operation() const noexcept {
        return active_operation_;
    }

private:
    [[nodiscard]] Result fail(ErrorCode error);
    [[nodiscard]] Result state_result() const noexcept { return {state_}; }

    PlanReader plan_reader_;
    State state_ = State::AwaitingPlan;
    Operation active_operation_ = Operation::Hello;
    bool has_active_operation_ = false;
};

} // namespace staged::runtime
