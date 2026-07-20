// On-device single-model inference for the app's AI chat, using the embedded
// llama.cpp (Metal). Separate from the ring stage: this runs a whole small model
// locally so a downloaded GGUF is usable without the network.
#ifndef KVASIR_LOCAL_LLAMA_H
#define KVASIR_LOCAL_LLAMA_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Streams generated text to `on_token` (called on a background thread) until the
// model emits end-of-generation or `max_tokens` is reached. Returns false if the
// model could not be loaded. Blocking; run off the main thread.
// `on_token` receives NUL-terminated UTF-8 fragments; return false from it to stop.
bool kvasir_local_generate(const char *model_path, const char *prompt,
                           int32_t n_ctx, int32_t max_tokens,
                           bool (*on_token)(const char *piece, void *ctx), void *ctx);

#ifdef __cplusplus
}
#endif

#endif
