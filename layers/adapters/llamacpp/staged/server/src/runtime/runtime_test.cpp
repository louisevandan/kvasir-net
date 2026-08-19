#include "runtime.hpp"
#include <cassert>
#include <cstdint>
#include <string>
#include <vector>

using staged::runtime::ErrorCode;
using staged::runtime::Operation;
using staged::runtime::Runtime;
using staged::runtime::State;

namespace {

void feed_plan(Runtime &runtime, const std::string &plan) {
    const auto size = static_cast<std::uint32_t>(plan.size());
    const std::vector<std::uint8_t> header{
        static_cast<std::uint8_t>(size),
        static_cast<std::uint8_t>(size >> 8U),
        static_cast<std::uint8_t>(size >> 16U),
        static_cast<std::uint8_t>(size >> 24U)};
    const std::vector<std::uint8_t> body(plan.begin(), plan.end());
    assert(runtime.stdin_plan().feed(header.data(), header.size()).ok());
    assert(runtime.stdin_plan().feed(body.data(), body.size()).ok());
}

void test_plan_stays_live() {
    Runtime runtime;
    feed_plan(runtime, "model=small");
    assert(runtime.state() == State::AwaitingPlan);
    assert(runtime.hello().ok());
    assert(runtime.state() == State::Ready);
    assert(runtime.stdin_eof().error == ErrorCode::UnexpectedEof);
    assert(runtime.state() == State::Failed);
}

void test_operation_order() {
    Runtime runtime;
    feed_plan(runtime, "plan");
    assert(runtime.hello().ok());
    assert(runtime.begin_hop().ok());
    assert(runtime.begin_hop().error == ErrorCode::OperationInProgress);
    assert(runtime.begin_kv(Operation::KvSave).error
           == ErrorCode::OperationInProgress);
    assert(runtime.unload().error == ErrorCode::OperationInProgress);
    assert(runtime.cancel().ok());
    assert(runtime.begin_kv(Operation::KvRestore).ok());
    assert(runtime.finish_kv().ok());
    assert(runtime.unload().ok());
    assert(runtime.finish_unload().ok());
    assert(runtime.state() == State::Unloaded);
}

void test_invalid_order_is_terminal() {
    Runtime runtime;
    assert(runtime.begin_hop().error == ErrorCode::InvalidState);
    assert(runtime.state() == State::AwaitingPlan);
}

} // namespace

int main() {
    test_plan_stays_live();
    test_operation_order();
    test_invalid_order_is_terminal();
    return 0;
}
