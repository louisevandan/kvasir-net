#include "runtime.hpp"

#include <algorithm>

namespace staged::runtime {

namespace {

Result result(State state, ErrorCode error = ErrorCode::None) {
    return {state, error};
}

std::uint32_t read_u32_le(const std::vector<std::uint8_t> &bytes) {
    return static_cast<std::uint32_t>(bytes[0])
        | (static_cast<std::uint32_t>(bytes[1]) << 8U)
        | (static_cast<std::uint32_t>(bytes[2]) << 16U)
        | (static_cast<std::uint32_t>(bytes[3]) << 24U);
}

} // namespace

PlanReader::PlanReader(std::size_t max_plan_bytes)
    : max_plan_bytes_(max_plan_bytes) {}

Result PlanReader::feed(const std::uint8_t *bytes, std::size_t size) {
    if (failed_) {
        return result(State::AwaitingPlan, ErrorCode::InvalidState);
    }
    if (complete_) {
        return result(State::PlanLoaded);
    }

    std::size_t offset = 0;
    if (!length_known_) {
        const auto prefix_needed = 4U - prefix_.size();
        const auto prefix_taken = std::min(prefix_needed, size);
        prefix_.insert(prefix_.end(), bytes, bytes + prefix_taken);
        offset = prefix_taken;
        if (prefix_.size() < 4U) {
            return result(State::AwaitingPlan);
        }

        expected_bytes_ = read_u32_le(prefix_);
        length_known_ = true;
        if (expected_bytes_ > max_plan_bytes_) {
            failed_ = true;
            return result(State::AwaitingPlan, ErrorCode::PlanTooLarge);
        }
        plan_.reserve(expected_bytes_);
    }

    const auto remaining = expected_bytes_ - plan_.size();
    const auto taken = std::min(remaining, size - offset);
    plan_.insert(plan_.end(), bytes + offset, bytes + offset + taken);
    if (plan_.size() == expected_bytes_) {
        complete_ = true;
        return result(State::PlanLoaded);
    }
    return result(State::AwaitingPlan);
}

Result PlanReader::eof() {
    if (complete_) {
        return result(State::PlanLoaded, ErrorCode::UnexpectedEof);
    }
    failed_ = true;
    return result(State::AwaitingPlan, ErrorCode::PlanIncomplete);
}

Runtime::Runtime(std::size_t max_plan_bytes)
    : plan_reader_(max_plan_bytes) {}

Result Runtime::fail(ErrorCode error) {
    state_ = State::Failed;
    has_active_operation_ = false;
    return result(state_, error);
}

Result Runtime::hello() {
    if (state_ != State::AwaitingPlan || !plan_reader_.complete()) {
        return result(state_, ErrorCode::InvalidState);
    }
    state_ = State::Ready;
    return state_result();
}

Result Runtime::begin_hop() {
    if (has_active_operation_) {
        return result(state_, ErrorCode::OperationInProgress);
    }
    if (state_ != State::Ready) {
        return result(state_, ErrorCode::InvalidState);
    }
    state_ = State::HopActive;
    active_operation_ = Operation::Hop;
    has_active_operation_ = true;
    return state_result();
}

Result Runtime::finish_hop() {
    if (!has_active_operation_) {
        return result(state_, ErrorCode::NoOperationInProgress);
    }
    if (active_operation_ != Operation::Hop) {
        return result(state_, ErrorCode::WrongOperation);
    }
    has_active_operation_ = false;
    state_ = State::Ready;
    return state_result();
}

Result Runtime::cancel() {
    if (!has_active_operation_ || active_operation_ != Operation::Hop) {
        return result(state_, ErrorCode::InvalidState);
    }
    has_active_operation_ = false;
    state_ = State::Ready;
    return state_result();
}

Result Runtime::begin_kv(Operation operation) {
    if (operation != Operation::KvSave && operation != Operation::KvRestore
        && operation != Operation::KvDrop && operation != Operation::KvPrepare
        && operation != Operation::KvCommit && operation != Operation::KvAbort
        && operation != Operation::KvReconcile) {
        return result(state_, ErrorCode::WrongOperation);
    }
    if (has_active_operation_) {
        return result(state_, ErrorCode::OperationInProgress);
    }
    if (state_ != State::Ready) {
        return result(state_, ErrorCode::InvalidState);
    }
    active_operation_ = operation;
    has_active_operation_ = true;
    state_ = State::KvActive;
    return state_result();
}

Result Runtime::finish_kv() {
    if (!has_active_operation_) {
        return result(state_, ErrorCode::NoOperationInProgress);
    }
    if (active_operation_ != Operation::KvSave
        && active_operation_ != Operation::KvRestore
        && active_operation_ != Operation::KvDrop
        && active_operation_ != Operation::KvPrepare
        && active_operation_ != Operation::KvCommit
        && active_operation_ != Operation::KvAbort
        && active_operation_ != Operation::KvReconcile) {
        return result(state_, ErrorCode::WrongOperation);
    }
    has_active_operation_ = false;
    state_ = State::Ready;
    return state_result();
}

Result Runtime::unload() {
    if (has_active_operation_) {
        return result(state_, ErrorCode::OperationInProgress);
    }
    if (state_ != State::Ready) {
        return result(state_, ErrorCode::InvalidState);
    }
    active_operation_ = Operation::Unload;
    has_active_operation_ = true;
    state_ = State::Unloading;
    return state_result();
}

Result Runtime::finish_unload() {
    if (!has_active_operation_ || active_operation_ != Operation::Unload) {
        return result(state_, ErrorCode::WrongOperation);
    }
    has_active_operation_ = false;
    state_ = State::Unloaded;
    return state_result();
}

Result Runtime::stdin_eof() {
    if (state_ == State::Unloaded) {
        return state_result();
    }
    const auto reader_result = plan_reader_.eof();
    (void)reader_result;
    return fail(ErrorCode::UnexpectedEof);
}

} // namespace staged::runtime
