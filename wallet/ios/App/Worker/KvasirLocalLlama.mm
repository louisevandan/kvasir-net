#import "KvasirLocalLlama.h"

#include "llama.h"

#include <string>
#include <vector>

// A modest local sampler: greedy is too repetitive on tiny models, so use a
// small top-k / top-p / temperature chain with a fixed seed for reproducibility.
static llama_sampler * make_sampler() {
    llama_sampler_chain_params sp = llama_sampler_chain_default_params();
    llama_sampler * chain = llama_sampler_chain_init(sp);
    llama_sampler_chain_add(chain, llama_sampler_init_top_k(40));
    llama_sampler_chain_add(chain, llama_sampler_init_top_p(0.95f, 1));
    llama_sampler_chain_add(chain, llama_sampler_init_temp(0.7f));
    llama_sampler_chain_add(chain, llama_sampler_init_dist(0xC0FFEE));
    return chain;
}

bool kvasir_local_generate(const char *model_path, const char *prompt,
                           int32_t n_ctx, int32_t max_tokens,
                           bool (*on_token)(const char *, void *), void *ctx_ptr) {
    llama_backend_init();

    llama_model_params mparams = llama_model_default_params();
    mparams.n_gpu_layers = 999;  // whole model on the A-series GPU (Metal)
    llama_model * model = llama_model_load_from_file(model_path, mparams);
    if (!model) { return false; }

    const llama_vocab * vocab = llama_model_get_vocab(model);

    llama_context_params cparams = llama_context_default_params();
    cparams.n_ctx   = n_ctx > 0 ? (uint32_t) n_ctx : 2048;
    cparams.n_batch = 512;
    llama_context * lctx = llama_init_from_model(model, cparams);
    if (!lctx) { llama_model_free(model); return false; }

    // Tokenize the (already chat-formatted) prompt.
    const int32_t n_prompt_max = (int32_t) strlen(prompt) + 16;
    std::vector<llama_token> tokens(n_prompt_max);
    int32_t n_prompt = llama_tokenize(vocab, prompt, (int32_t) strlen(prompt),
                                      tokens.data(), n_prompt_max,
                                      /*add_special=*/true, /*parse_special=*/true);
    if (n_prompt < 0) { n_prompt = -n_prompt; tokens.resize(n_prompt);
        n_prompt = llama_tokenize(vocab, prompt, (int32_t) strlen(prompt),
                                  tokens.data(), n_prompt, true, true); }
    tokens.resize(n_prompt > 0 ? n_prompt : 0);

    llama_sampler * smpl = make_sampler();
    bool ok = true;

    // Prefill.
    if (!tokens.empty()) {
        llama_batch batch = llama_batch_get_one(tokens.data(), (int32_t) tokens.size());
        ok = llama_decode(lctx, batch) == 0;
    }

    // Decode loop.
    char piece[512];
    for (int32_t generated = 0; ok && generated < max_tokens; ++generated) {
        const llama_token id = llama_sampler_sample(smpl, lctx, -1);
        if (llama_vocab_is_eog(vocab, id)) { break; }
        const int32_t n = llama_token_to_piece(vocab, id, piece, sizeof(piece) - 1, 0, false);
        if (n > 0) {
            piece[n] = '\0';
            if (on_token && !on_token(piece, ctx_ptr)) { break; }
        }
        llama_token next = id;
        llama_batch batch = llama_batch_get_one(&next, 1);
        ok = llama_decode(lctx, batch) == 0;
    }

    llama_sampler_free(smpl);
    llama_free(lctx);
    llama_model_free(model);
    return true;
}
