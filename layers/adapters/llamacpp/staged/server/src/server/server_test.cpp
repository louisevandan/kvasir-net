#include "server.hpp"
#include "transaction_store.hpp"

#include <cassert>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <memory>
#include <string>

using staged::protocol::Frame;
using staged::protocol::Operation;
using staged::protocol::Descriptor;
using staged::protocol::SequencePayload;
using staged::protocol::HopPayload;
using staged::protocol::KvPayload;
using staged::protocol::KvReceipt;
using staged::protocol::KvReceiptState;
using staged::protocol::WireType;
using staged::server::Session;
using staged::server::HopExecutor;

static void transaction_lease_fences_same_operation() {
    const auto root = std::filesystem::temp_directory_path() / "p4-staged-lease-test";
    std::error_code cleanup_error;
    std::filesystem::remove_all(root, cleanup_error);
    staged::runtime::TransactionStore store(root);
    std::string error;
    auto first = store.acquire("same-operation", &error);
    assert(first.held());
    auto second = store.acquire("same-operation", &error);
    assert(!second.held());
    first = {};
    auto after_release = store.acquire("same-operation", &error);
    assert(after_release.held());
    std::filesystem::remove_all(root, cleanup_error);
}

static void load_plan(Session &session) {
    const std::uint8_t plan[] = {4, 0, 0, 0, '{', '}', '\n', ' '};
    if (!session.feed_plan({plan, plan + sizeof(plan)})) {
        std::abort();
    }
}

static void hello_requires_plan() {
    Session session;
    bool close = false;
    const auto response = session.handle(Frame::make(Operation::Hello, {}), &close);
    assert(response.header.operation == Operation::Error);
    assert(!close);
}

static void unsupported_capabilities_are_explicit() {
    Session session;
    load_plan(session);
    const auto hello = session.handle(Frame::make(Operation::Hello, {}));
    assert(hello.header.operation == Operation::Hello);
    assert(hello.body.size() > 2);
    assert(hello.body[0] == (staged::protocol::kProtocolRevision & 0xffU));
    assert(hello.body[1] == (staged::protocol::kProtocolRevision >> 8U));
    const std::string capabilities(hello.body.begin() + 2, hello.body.end());
    for (const auto * required : {
             ";n_ctx=", ";n_batch=", ";n_ubatch=", ";n_seq_max=",
             ";physical_result_payload_bytes=", ";physical_result_tensor_count=", ";max_physical_result_bytes=",
             ";max_atomic_sequences=", ";upstream="}) {
        assert(capabilities.find(required) != std::string::npos);
    }
    const auto hop = session.handle(Frame::make(Operation::Hop, {}));
    assert(hop.header.operation == Operation::Error);
}

static void llama_capability_failure_is_explicit_for_a_valid_tensor_hop() {
    Session session({true, true, false});
    load_plan(session);
    const auto hello = session.handle(Frame::make(Operation::Hello, {}));
    assert(hello.header.operation == Operation::Hello);
    SequencePayload input{
        "seq-1",
        {Descriptor{WireType::F32, {1, 1}, {4, 4}, 4, 0, false, 0, 0, "hidden"}},
        {std::vector<std::uint8_t>{0, 0, 0, 0}},
    };
    const auto hop = session.handle(Frame::make(
        Operation::Hop, input.encode(staged::protocol::ProtocolLimits{})));
    assert(hop.header.operation == Operation::Error);
}

static void llama_free_build_rejects_kv_with_capability_error() {
    Session session({false, false, true});
    load_plan(session);
    (void)session.handle(Frame::make(Operation::Hello, {}));
    KvPayload payload{
        "seq-1", "deployment-1", "model-fingerprint", 0, 4, 0, {}};
    const auto response = session.handle(Frame::make(
        Operation::KvSave, payload.encode(staged::protocol::ProtocolLimits{})));
    assert(response.header.operation == Operation::Error);
    assert(session.state() == staged::runtime::State::Ready);
}

static void durable_transaction_prepare_reconcile_abort_round_trip() {
    const auto root = std::filesystem::temp_directory_path() / "p4-staged-transaction-test";
    std::error_code cleanup_error;
    std::filesystem::remove_all(root, cleanup_error);
    Session session(staged::server::Capabilities{false, false, true}
#ifdef P4_STAGED_WITH_LLAMA
                    , nullptr
#endif
                    , HopExecutor{}, root);
    load_plan(session);
    (void)session.handle(Frame::make(Operation::Hello, {}));
    KvPayload payload{
        "seq-transaction", "cache-transaction", "model-fingerprint", 0, 4,
        staged::protocol::kKvPersist, {}, "operation-transaction"};
    const auto encoded = payload.encode(staged::protocol::ProtocolLimits{});
    auto response = session.handle(Frame::make(Operation::KvPrepare, encoded));
    assert(response.header.operation == Operation::KvReceipt);
    auto receipt = KvReceipt::decode(response.body, staged::protocol::ProtocolLimits{});
    assert(receipt.state == KvReceiptState::Prepared);

    auto control = payload;
    control.flags = staged::protocol::kKvDirect;
    const auto control_encoded = control.encode(staged::protocol::ProtocolLimits{});
    response = session.handle(Frame::make(Operation::KvReconcile, control_encoded));
    receipt = KvReceipt::decode(response.body, staged::protocol::ProtocolLimits{});
    assert(receipt.state == KvReceiptState::Prepared);
    response = session.handle(Frame::make(Operation::KvAbort, control_encoded));
    receipt = KvReceipt::decode(response.body, staged::protocol::ProtocolLimits{});
    assert(receipt.state == KvReceiptState::Aborted);

    Session restarted(staged::server::Capabilities{false, false, true}
#ifdef P4_STAGED_WITH_LLAMA
                      , nullptr
#endif
                      , HopExecutor{}, root);
    load_plan(restarted);
    (void)restarted.handle(Frame::make(Operation::Hello, {}));
    response = restarted.handle(Frame::make(Operation::KvReconcile, control_encoded));
    receipt = KvReceipt::decode(response.body, staged::protocol::ProtocolLimits{});
    assert(receipt.state == KvReceiptState::Aborted);
    std::filesystem::remove_all(root, cleanup_error);
}

static void committed_receipt_without_runtime_is_not_reported_as_durable() {
    const auto root = std::filesystem::temp_directory_path()
        / "p4-staged-transaction-reconcile-fail-closed-test";
    std::error_code cleanup_error;
    std::filesystem::remove_all(root, cleanup_error);
    Session session(staged::server::Capabilities{false, false, true}
#ifdef P4_STAGED_WITH_LLAMA
                    , nullptr
#endif
                    , HopExecutor{}, root);
    load_plan(session);
    (void)session.handle(Frame::make(Operation::Hello, {}));
    KvPayload payload{
        "seq-reconcile", "cache-reconcile", "model-fingerprint", 0, 4,
        staged::protocol::kKvDirect, {}, "operation-reconcile"};
    KvReceipt committed{
        payload.operation_id, payload.sequence_id, payload.cache_key, payload.model_identity,
        payload.stage_begin, payload.stage_end, staged::protocol::kKvPersist,
        KvReceiptState::Committed, 12, std::string(64, '0'), "committed"};
    staged::runtime::TransactionStore store(root);
    std::string error;
    assert(store.write(committed, &error));
    auto response = session.handle(Frame::make(
        Operation::KvReconcile, payload.encode(staged::protocol::ProtocolLimits{})));
    assert(response.header.operation == Operation::KvReceipt);
    const auto receipt = KvReceipt::decode(response.body, staged::protocol::ProtocolLimits{});
    assert(receipt.state == KvReceiptState::Inconsistent);
    std::filesystem::remove_all(root, cleanup_error);
}

static SequencePayload tensor_sequence(const char *id, std::uint8_t value) {
    auto result = SequencePayload{
        id,
        {Descriptor{WireType::F32, {1, 1}, {4, 4}, 4, 0, false, 0, 0, "hidden"}},
        {std::vector<std::uint8_t>{value, value, value, value}},
    };
    result.options = R"({"temperature":0})";
    return result;
}

static std::unique_ptr<Session> make_hop_session(HopExecutor executor) {
#ifdef P4_STAGED_WITH_LLAMA
    return std::make_unique<Session>(staged::server::Capabilities{true, true, false},
                                     nullptr, std::move(executor));
#else
    return std::make_unique<Session>(staged::server::Capabilities{true, true, false},
                                     std::move(executor));
#endif
}

static void server_executes_and_groups_each_sequence_in_a_hop() {
    std::size_t calls = 0;
    auto session = make_hop_session(
        [&](const SequencePayload &input, staged::protocol::HopPhase,
            SequencePayload *output, std::string *error) {
            assert(error != nullptr);
            ++calls;
            *output = input;
            output->sequence_id += "-out";
            return true;
        });
    load_plan(*session);
    (void)session->handle(Frame::make(Operation::Hello, {}));

    const HopPayload request{staged::protocol::HopPhase::Decode,
                             {tensor_sequence("seq-a", 1), tensor_sequence("seq-b", 2)}, false};
    const auto response = session->handle(Frame::make(
        Operation::Hop, request.encode(staged::protocol::ProtocolLimits{})));
    assert(response.header.operation == Operation::HopResult);
    assert(calls == 2);
    bool enveloped = false;
    const auto result = HopPayload::decode(response.body,
                                           staged::protocol::ProtocolLimits{}, &enveloped);
    assert(enveloped);
    assert(result.sequences.size() == 2);
    assert(result.sequences[0].sequence_id == "seq-a-out");
    assert(result.sequences[1].sequence_id == "seq-b-out");
    assert(result.sequences[0].options == R"({"temperature":0})");
    assert(result.sequences[1].options == R"({"temperature":0})");
    assert(session->state() == staged::runtime::State::Ready);
}

static void server_keeps_legacy_single_sequence_response_wire() {
    auto session = make_hop_session(
        [](const SequencePayload &input, staged::protocol::HopPhase,
           SequencePayload *output, std::string *) {
            *output = input;
            return true;
        });
    load_plan(*session);
    (void)session->handle(Frame::make(Operation::Hello, {}));
    const auto input = tensor_sequence("seq-legacy", 3);
    const auto response = session->handle(Frame::make(
        Operation::Hop, input.encode(staged::protocol::ProtocolLimits{})));
    assert(response.header.operation == Operation::HopResult);
    bool enveloped = true;
    const auto result = HopPayload::decode(response.body,
                                           staged::protocol::ProtocolLimits{}, &enveloped);
    assert(!enveloped);
    assert(result.sequences.size() == 1);
    assert(result.sequences[0].sequence_id == "seq-legacy");
}

static void completed_hop_accepts_racing_empty_cancel() {
    auto session = make_hop_session(
        [](const SequencePayload &input, staged::protocol::HopPhase,
           SequencePayload *output, std::string *) {
            *output = input;
            return true;
        });
    load_plan(*session);
    (void)session->handle(Frame::make(Operation::Hello, {}));
    const auto input = tensor_sequence("seq-racing-cancel", 7);
    const auto hop = session->handle(Frame::make(
        Operation::Hop, input.encode(staged::protocol::ProtocolLimits{})));
    assert(hop.header.operation == Operation::HopResult);
    const auto cancel = session->handle(Frame::make(Operation::Cancel, {}));
    assert(cancel.header.operation == Operation::Cancel);
    assert(std::string(cancel.body.begin(), cancel.body.end())
           == "HOP_ALREADY_COMPLETE");
    assert(session->state() == staged::runtime::State::Ready);
}

static void physical_binding_is_explicit_and_blocks_legacy_mutation() {
    Session session;
    load_plan(session);
    const auto hello = session.handle(Frame::make(Operation::Hello, {}));
    const std::string capabilities(hello.body.begin() + 2, hello.body.end());
    assert(capabilities.find(";physical_identity_revision=1") != std::string::npos);
    const std::vector<std::uint8_t> generation{7, 0, 0, 0, 0, 0, 0, 0};
    for (unsigned attempt = 0; attempt < 2; ++attempt) {
        const auto bound = session.handle(Frame::make(Operation::BindLoad, generation));
        assert(bound.header.operation == Operation::BindLoad && bound.body == generation);
    }
    auto different = generation;
    different[0] = 8;
    assert(session.handle(Frame::make(Operation::BindLoad, different)).header.operation == Operation::Error);
    for (const auto operation : {Operation::Hop, Operation::Cancel, Operation::KvRestore, Operation::KvCommit}) {
        const auto result = session.handle(Frame::make(operation, {}));
        assert(result.header.operation == Operation::Error);
        assert(std::string(result.body.begin(), result.body.end()).find("rejects legacy mutation") != std::string::npos);
    }
}

int main() {
    physical_binding_is_explicit_and_blocks_legacy_mutation();
    transaction_lease_fences_same_operation();
    hello_requires_plan();
    unsupported_capabilities_are_explicit();
    llama_capability_failure_is_explicit_for_a_valid_tensor_hop();
    llama_free_build_rejects_kv_with_capability_error();
    durable_transaction_prepare_reconcile_abort_round_trip();
    committed_receipt_without_runtime_is_not_reported_as_durable();
    server_executes_and_groups_each_sequence_in_a_hop();
    server_keeps_legacy_single_sequence_response_wire();
    completed_hop_accepts_racing_empty_cancel();
    return 0;
}
