#include "physical_authority.hpp"

#include <cassert>
#include <cstdint>
#include <string>
#include <vector>

using namespace staged::runtime;
using Decision = PhysicalAuthority::ControlDecision;

namespace {
PhysicalIdentity identity(std::uint64_t incarnation = 1, std::uint32_t slot = 0) {
    return {7, incarnation, slot, "pipeline", std::string("pipeline\0same-key", 17)};
}
void number(std::vector<std::uint8_t> & out, std::uint64_t value, unsigned count) {
    for (unsigned i = 0; i < count; ++i) out.push_back(std::uint8_t(value >> (i * 8U)));
}
std::vector<std::uint8_t> body(PhysicalIdentity id, std::uint64_t op) {
    std::vector<std::uint8_t> out{'P', '4', 'I', 'D', 1, 0, 0, 0};
    number(out, id.load_generation, 8);
    number(out, id.incarnation, 8);
    number(out, op, 8);
    number(out, id.slot, 4);
    for (const auto & text : {id.session, id.key}) {
        number(out, text.size(), 4);
        out.insert(out.end(), text.begin(), text.end());
    }
    return out;
}
PhysicalControlIdentity control(const std::vector<std::uint8_t> & bytes) {
    PhysicalControlIdentity value;
    std::string error;
    assert(decode_physical_control_identity(bytes, &value, &error));
    return value;
}
void acquire(PhysicalAuthority & state, PhysicalIdentity id) {
    std::string error;
    PhysicalAuthority::RowsPlan plan;
    assert(state.prepare_rows({{id, true}}, &plan, &error));
    state.commit_rows(plan);
}
void binding_and_rejected_rows_do_not_grant_authority() {
    PhysicalAuthority state;
    PhysicalAuthority::RowsPlan plan;
    std::string error;
    assert(!state.prepare_rows({{identity(), true}}, &plan, &error));
    assert(!state.bind(0, 4, &error));
    assert(state.bind(7, 4, &error));
    assert(state.bind(7, 4, &error));
    assert(!state.bind(8, 4, &error));
    assert(!state.prepare_rows({{identity(), false}}, &plan, &error));
    auto alien = identity();
    alien.key = std::string("pipeline\0another-key", 20);
    assert(!state.prepare_rows({{identity(), true}, {alien, true}}, &plan, &error));
    std::vector<std::uint8_t> cached;
    const auto request = body(identity(), 1);
    assert(state.prepare_control(control(request), true, request, &cached, &error) == Decision::Rejected);
    acquire(state, identity());
    assert(!state.prepare_rows({{identity(2), true}}, &plan, &error));
}
void duplicate_control_returns_receipt_without_native_effect() {
    PhysicalAuthority state;
    std::string error;
    assert(state.bind(7, 4, &error));
    acquire(state, identity());
    auto request = body(identity(), 5);
    request.push_back(8); // Opaque exact control payload for guard-only testing.
    const auto command = control(request);
    std::vector<std::uint8_t> cached;
    unsigned calls = 0;
    auto invoke = [&] {
        const auto decision = state.prepare_control(command, false, request, &cached, &error);
        if (decision == Decision::Execute) {
            ++calls;
            state.commit_control(command, false, request, {42});
        }
        return decision;
    };
    assert(invoke() == Decision::Execute);
    assert(invoke() == Decision::Replay);
    assert(calls == 1 && cached == std::vector<std::uint8_t>{42});
    request.back() ^= 1;
    assert(invoke() == Decision::Rejected);
    assert(calls == 1);
    request.back() ^= 1;
    assert(state.prepare_control(command, true, request, &cached, &error) == Decision::Rejected);
    const auto older = body(identity(), 4);
    assert(state.prepare_control(control(older), false, older, &cached, &error) == Decision::Rejected);
    assert(state.bind(7, 4, &error));
    assert(invoke() == Decision::Replay); // HELLO/reconnect/bind cannot clear receipts.
}
void late_release_cannot_destroy_a_reused_identical_key_and_slot() {
    PhysicalAuthority state;
    std::string error;
    assert(state.bind(7, 4, &error));
    acquire(state, identity());
    const auto old = body(identity(), 1);
    std::vector<std::uint8_t> cached;
    assert(state.prepare_control(control(old), true, old, &cached, &error) == Decision::Execute);
    state.commit_control(control(old), true, old, old);
    assert(state.prepare_control(control(old), true, old, &cached, &error) == Decision::Replay);
    PhysicalAuthority::RowsPlan plan;
    assert(!state.prepare_rows({{identity(), true}}, &plan, &error));
    acquire(state, identity(2));
    assert(state.prepare_control(control(old), true, old, &cached, &error) == Decision::Rejected);
    assert(state.prepare_control(control(old), false, old, &cached, &error) == Decision::Rejected);
    assert(!state.prepare_rows({{identity(), true}}, &plan, &error));
    assert(state.prepare_rows({{identity(2), false}}, &plan, &error));
    auto wrong_session = body({7, 2, 0, "other", std::string("other\0same-key", 14)}, 2);
    assert(state.prepare_control(control(wrong_session), true, wrong_session, &cached, &error) == Decision::Rejected);
}
void fences_and_receipt_budgets_refuse_before_native_work() {
    PhysicalAuthority state;
    std::string error;
    assert(state.bind(7, 4, &error));
    acquire(state, identity());
    const auto request = body(identity(), 1);
    auto oversized = request;
    oversized.resize(PhysicalAuthority::max_control_bytes + 1);
    std::vector<std::uint8_t> cached;
    assert(state.prepare_control(control(request), true, oversized, &cached, &error) == Decision::Rejected);
    state.fence();
    PhysicalAuthority::RowsPlan plan;
    assert(!state.prepare_rows({{identity(), false}}, &plan, &error));
    assert(state.prepare_control(control(request), true, request, &cached, &error) == Decision::Rejected);
    assert(state.bind(7, 4, &error) && state.fenced());
}
void identity_codec_refuses_old_zero_truncated_and_conflicting_headers() {
    const auto valid = body(identity(), 1);
    const auto value = control(valid);
    assert(value.identity == identity() && value.operation_id == 1);
    assert(value.prefix_bytes == valid.size());
    assert(valid[32] == 0 && valid[36] == 8); // agreed slot and u32 string offsets.
    std::string error;
    for (std::size_t length = 0; length < valid.size(); ++length) {
        PhysicalControlIdentity decoded;
        assert(!decode_physical_control_identity({valid.begin(), valid.begin() + length}, &decoded, &error));
    }
    for (const auto offset : {4U, 6U, 8U, 16U, 24U}) {
        auto broken = valid;
        broken[offset] = offset == 6 ? 1 : 0;
        PhysicalControlIdentity decoded;
        assert(!decode_physical_control_identity(broken, &decoded, &error));
    }
    auto zero_incarnation = body(identity(0), 1);
    PhysicalControlIdentity decoded;
    assert(!decode_physical_control_identity(zero_incarnation, &decoded, &error));
}
void malformed_request_names_never_acquire_native_authority() {
    PhysicalAuthority state;
    std::string error;
    assert(state.bind(7, 4, &error));
    std::vector<PhysicalIdentity> invalid;
    auto changed = identity();
    changed.key = std::string("different\0same-key", 18);
    invalid.push_back(changed);
    changed = identity();
    changed.key = std::string("pipeline\0", 9); // empty request
    invalid.push_back(changed);
    changed = identity();
    changed.key += std::string("\0suffix", 7);
    invalid.push_back(changed);
    changed = identity();
    changed.session = std::string("pipe\0line", 9);
    changed.key = changed.session + std::string("\0same-key", 9);
    invalid.push_back(changed);
    for (const std::string corrupt : {std::string("\xC0\x80"), std::string("\xED\xA0\x80"),
            std::string("\xF4\x90\x80\x80"), std::string("\x80"), std::string("\xE3\x81")}) {
        changed = identity();
        changed.key = std::string("pipeline\0", 9) + corrupt;
        invalid.push_back(changed);
        changed.session = corrupt;
        changed.key = corrupt + std::string("\0same-key", 9);
        invalid.push_back(changed);
    }
    for (const auto & id : invalid) {
        PhysicalAuthority::RowsPlan plan;
        assert(!state.prepare_rows({{id, true}}, &plan, &error));
        PhysicalControlIdentity parsed;
        assert(!decode_physical_control_identity(body(id, 1), &parsed, &error));
    }
    // Rejection did not reserve the slot. Valid Unicode bytes remain distinct
    // and accepted; there is no Unicode normalization of request identities.
    changed = identity();
    changed.session = "\xED\x95\x9C";
    changed.key = changed.session + std::string("\0", 1) + "\xF0\x9F\x98\x80";
    acquire(state, changed);
}
} // namespace

int main() {
    binding_and_rejected_rows_do_not_grant_authority();
    duplicate_control_returns_receipt_without_native_effect();
    late_release_cannot_destroy_a_reused_identical_key_and_slot();
    fences_and_receipt_budgets_refuse_before_native_work();
    identity_codec_refuses_old_zero_truncated_and_conflicting_headers();
    malformed_request_names_never_acquire_native_authority();
}
