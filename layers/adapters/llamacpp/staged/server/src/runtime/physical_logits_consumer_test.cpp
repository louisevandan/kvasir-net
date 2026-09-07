// Compile the production physical consumer body below, not a copied batch
// builder. Only the model/context and native-call boundary are substituted.
// This is a model-free construction/capture test, NOT llama execution, MTP
// acceptance, checkpoint restoration or backend/logits availability evidence.
#include "llama_stage_runtime.hpp"
#include "physical_wire.hpp"

#include <algorithm>
#include <cassert>
#include <cstring>
#include <iostream>
#include <utility>

namespace staged::llama_runtime { class PhysicalConsumerProbe; }

// Complete the public ABI's opaque handles only in this test translation unit.
struct llama_model { bool encoder = false; };
struct llama_context {
    staged::llama_runtime::PhysicalConsumerProbe * runtime = nullptr;
    std::vector<std::int8_t> observed_logits;
    std::vector<llama_token> observed_tokens;
    std::vector<llama_pos> observed_positions;
    int decode_calls = 0;
    int encode_calls = 0;
    int input_count = 0;
};

namespace staged::llama_runtime {

// Methods defined by the included production .cpp retain their exact bodies.
// MTP preparation/atomic validation and loaded-model establishment are fixture
// boundaries; they do not compute an alternative mask or modify any row.
class PhysicalConsumerProbe {
public:
    struct Config { int layer_begin = 0; } config_;
    llama_model * model_ = nullptr;
    llama_context * ctx_ = nullptr;
    bool tail_stage_ = false;
    bool hop_memory_dirty_ = false;
    std::string capture_error_;
    std::vector<PhysicalExecution> captured_executions_;

    bool loaded() const { return model_ && ctx_; }
    bool requires_equal_sequence_ubatch() const { return false; }
    bool prepare_physical_owners(const std::vector<PhysicalOwner> &, std::string *) { return true; }
    // Mixed phases below exercise mask construction, not a legal recurrent
    // ubatch or native splitting shape. Atomic/shape validation is not tested.
    bool validate_physical_atomic_round(const std::vector<PhysicalOwner> &, std::string *) { return true; }
    bool process_physical_mtp(const llama_batch &, const std::vector<PhysicalOwner> &, std::string *) { return true; }
    bool collect_physical_tensors(bool, std::vector<PhysicalTensor> *, std::string *);
    bool capture_execution(llama_context *, const llama_linkcpp_stage_invocation *);
    bool execute_first_batch(const std::vector<LogicalRow> &, const std::vector<PhysicalOwner> &,
                             std::vector<PhysicalExecution> *, std::string *);
    bool execute_physical(const PhysicalExecution &, const std::vector<PhysicalOwner> &,
                          PhysicalExecution *, std::string *);
    bool prepare_physical_execution(const PhysicalExecution &, const std::vector<PhysicalOwner> &,
                                    std::string *);
};

namespace native_probe {
std::uint32_t n_batch(const llama_context *) { return 32; }
std::uint32_t n_ubatch(const llama_context *) { return 32; }
std::uint32_t n_seq_max(const llama_context *) { return 8; }
bool has_encoder(const llama_model * model) { return model->encoder; }
int output_count(const llama_context *) { return 1; }
bool describe(const llama_context *, int index, llama_linkcpp_tensor_desc * desc) {
    if (index != 0) return false;
    *desc = {};
    desc->n_dims = 1;
    desc->ne[0] = 1;
    desc->nbytes = 4;
    desc->alias_of = -1;
    return true;
}
bool get(const llama_context *, int index, void * data, std::size_t size) {
    if (index != 0 || size != 4) return false;
    std::memset(data, 17, size);
    return true;
}
bool synchronize(const llama_context *) { return true; }
void input_clear(llama_context * ctx) { ctx->input_count = 0; }
bool input_set(llama_context * ctx, const llama_linkcpp_tensor_desc *, const void *, std::size_t size) {
    if (size != 4) return false;
    ++ctx->input_count;
    return true;
}
int input_count(const llama_context * ctx) { return ctx->input_count; }

llama_batch batch_init(int count, int, int) {
    llama_batch batch{};
    batch.token = new llama_token[count];
    batch.pos = new llama_pos[count];
    batch.n_seq_id = new int[count];
    batch.seq_id = new llama_seq_id *[count];
    for (int i = 0; i < count; ++i) batch.seq_id[i] = new llama_seq_id[1];
    batch.logits = new std::int8_t[count];
    batch.n_tokens = count;
    return batch;
}
void batch_free(llama_batch batch) {
    for (int i = 0; i < batch.n_tokens; ++i) delete[] batch.seq_id[i];
    delete[] batch.seq_id;
    delete[] batch.n_seq_id;
    delete[] batch.token;
    delete[] batch.pos;
    delete[] batch.logits;
}

int submit(llama_context * ctx, const llama_batch & batch) {
    // Observe the arguments at the actual production llama_decode/encode call.
    // This function does not derive expected logits from phase or owner data.
    ctx->observed_logits.assign(batch.logits, batch.logits + batch.n_tokens);
    ctx->observed_tokens.assign(batch.token, batch.token + batch.n_tokens);
    ctx->observed_positions.assign(batch.pos, batch.pos + batch.n_tokens);
    if (ctx->runtime->config_.layer_begin == 0) {
        llama_linkcpp_stage_invocation invocation{};
        invocation.version = 1;
        invocation.n_tokens = batch.n_tokens;
        invocation.n_pos = 1;
        invocation.n_seqs = 4;
        invocation.n_seqs_unq = 4;
        invocation.n_seq_tokens = batch.n_tokens;
        invocation.pos = batch.pos;
        invocation.n_seq_id = batch.n_seq_id;
        invocation.seq_id = batch.seq_id;
        invocation.output = batch.logits;
        if (!ctx->runtime->capture_execution(ctx, &invocation)) return -1;
    }
    return 0;
}
int decode(llama_context * ctx, llama_batch batch) { ++ctx->decode_calls; return submit(ctx, batch); }
int encode(llama_context * ctx, llama_batch batch) { ++ctx->encode_calls; return submit(ctx, batch); }
} // namespace native_probe
} // namespace staged::llama_runtime

// The public headers were read above, so these substitutions affect only the
// real consumer definitions and their native calls in this one test TU.
#define StageRuntime PhysicalConsumerProbe
#define llama_n_batch native_probe::n_batch
#define llama_n_ubatch native_probe::n_ubatch
#define llama_n_seq_max native_probe::n_seq_max
#define llama_model_has_encoder native_probe::has_encoder
#define llama_linkcpp_terminal_count native_probe::output_count
#define llama_linkcpp_output_count native_probe::output_count
#define llama_linkcpp_terminal_desc native_probe::describe
#define llama_linkcpp_output_desc native_probe::describe
#define llama_linkcpp_terminal_get native_probe::get
#define llama_linkcpp_output_get native_probe::get
#define llama_linkcpp_output_synchronize native_probe::synchronize
#define llama_linkcpp_input_clear native_probe::input_clear
#define llama_linkcpp_input_set_tensor native_probe::input_set
#define llama_linkcpp_input_count native_probe::input_count
#define llama_batch_init native_probe::batch_init
#define llama_batch_free native_probe::batch_free
#define llama_decode native_probe::decode
#define llama_encode native_probe::encode
#include "llama_stage_runtime_physical.cpp"
#undef StageRuntime

namespace staged::llama_runtime {
namespace {
std::vector<PhysicalOwner> mixed_owners() {
    std::vector<PhysicalOwner> owners(7);
    const PhysicalPhase phases[] = {PhysicalPhase::Prefill, PhysicalPhase::Prefill,
        PhysicalPhase::Decode, PhysicalPhase::Verify, PhysicalPhase::Verify,
        PhysicalPhase::Replay, PhysicalPhase::Replay};
    const bool wire[] = {false, true, true, true, true, false, false};
    const std::uint32_t slots[] = {0, 0, 1, 2, 2, 3, 3};
    const std::uint32_t positions[] = {0, 1, 5, 8, 9, 12, 13};
    for (std::size_t i = 0; i < owners.size(); ++i) {
        auto & owner = owners[i];
        owner.load_generation = 7;
        owner.incarnation = 11;
        owner.session_id = "session";
        owner.request_id = "r" + std::to_string(slots[i]);
        owner.sequence_key = owner.session_id + '\0' + owner.request_id;
        owner.sequence_id = slots[i];
        owner.position = positions[i];
        owner.phase = phases[i];
        owner.output = wire[i];
        owner.input_token = 100 + static_cast<llama_token>(i);
        owner.max_tokens = 32;
        owner.generated_tokens = slots[i] == 0 ? 0 : 2;
        if (i >= 3) {
            owner.speculative_id = slots[i];
            owner.speculative_index = i < 5 ? i - 3 : i - 5;
            owner.speculative_count = 2;
        }
    }
    return owners;
}

std::vector<PhysicalOwner> owners_for(bool replay_only) {
    auto owners = mixed_owners();
    return replay_only ? std::vector<PhysicalOwner>(owners.begin() + 5, owners.end()) : owners;
}

void check_observation(const llama_context & ctx, const std::vector<PhysicalOwner> & owners,
                       bool replay_only) {
    // In the all-Replay case the old code requests ZERO outputs. The mixed
    // case separately catches enabling only the last row or all ordinary rows.
    const std::vector<std::int8_t> expected = replay_only
        ? std::vector<std::int8_t>{1, 1} : std::vector<std::int8_t>{0, 1, 1, 1, 1, 1, 1};
    assert(ctx.observed_logits == expected && "native submission must request EVERY Replay row logits");
    for (std::size_t i = 0; i < owners.size(); ++i) {
        assert(ctx.observed_tokens[i] == owners[i].input_token);
        assert(ctx.observed_positions[i] == static_cast<llama_pos>(owners[i].position));
    }
    if (replay_only) assert(!owners[0].output && !owners[1].output);
    else assert(!owners[0].output && !owners[5].output && !owners[6].output);
}

void first_stage(bool encoder, bool replay_only) {
    llama_model model{encoder};
    llama_context ctx;
    PhysicalConsumerProbe runtime;
    runtime.model_ = &model;
    runtime.ctx_ = &ctx;
    ctx.runtime = &runtime;
    auto owners = owners_for(replay_only);
    std::vector<LogicalRow> rows;
    for (const auto & owner : owners) rows.push_back({owner.input_token,
        static_cast<llama_pos>(owner.position), static_cast<llama_seq_id>(owner.sequence_id), owner.output});
    std::vector<PhysicalExecution> captured;
    std::string error;
    assert(runtime.execute_first_batch(rows, owners, &captured, &error));
    assert(ctx.decode_calls == (encoder ? 0 : 1));
    assert(ctx.encode_calls == (encoder ? 1 : 0));
    check_observation(ctx, owners, replay_only);
    assert(captured.size() == 1 && captured[0].positions == ctx.observed_positions);
    assert(captured[0].sequence_counts == std::vector<std::int32_t>(owners.size(), 1));
    for (std::size_t i = 0; i < rows.size(); ++i) assert(rows[i].output == owners[i].output);
    // This is the internal llama capture mask. server_physical's owner-based
    // restoration to wire output=false is a DIFFERENT consumer, outside this
    // test. Do not mistake internal capture output=true for a wire change.
    assert(captured[0].output == ctx.observed_logits);
}

void downstream(bool terminal, bool encoder, bool replay_only) {
    llama_model model{encoder};
    llama_context ctx;
    PhysicalConsumerProbe runtime;
    runtime.model_ = &model;
    runtime.ctx_ = &ctx;
    runtime.config_.layer_begin = 2;
    runtime.tail_stage_ = terminal;
    ctx.runtime = &runtime;
    auto owners = owners_for(replay_only);
    PhysicalExecution input;
    input.n_pos = 1;
    input.n_seq_tokens = owners.size();
    input.n_seqs = input.n_seqs_unq = 4;
    input.flags = encoder ? LLAMA_LINKCPP_STAGE_FLAG_ENCODER : 0;
    for (const auto & owner : owners) {
        input.positions.push_back(owner.position);
        input.sequence_counts.push_back(1);
        input.sequence_ids.push_back(owner.sequence_id);
        input.output.push_back(owner.output ? 1 : 0);
    }
    PhysicalTensor tensor;
    native_probe::describe(&ctx, 0, &tensor.descriptor);
    tensor.data = {1, 2, 3, 4};
    input.tensors.push_back(tensor);
    const auto original_output = input.output;
    PhysicalExecution output;
    std::string error;
    assert(runtime.execute_physical(input, owners, &output, &error));
    assert(ctx.decode_calls == (encoder ? 0 : 1));
    assert(ctx.encode_calls == (encoder ? 1 : 0));
    check_observation(ctx, owners, replay_only);
    assert(input.output == original_output && output.output == original_output);
    assert(input.tensors[0].data == tensor.data);
    assert(output.terminal == terminal);
    assert(output.tensors.empty() == terminal);
}
} // namespace
} // namespace staged::llama_runtime

int main() {
    using namespace staged::llama_runtime;
    for (bool replay_only : {true, false}) {
        first_stage(false, replay_only);
        first_stage(true, replay_only);
        downstream(false, false, replay_only);
        downstream(true, false, replay_only);
        downstream(false, true, replay_only);
        downstream(true, true, replay_only);
    }
    std::cout << "PHYSICAL_LOGITS_CONSUMER_OK: 12 native-mask cases; input owners and downstream wire flags unchanged\n";
}
