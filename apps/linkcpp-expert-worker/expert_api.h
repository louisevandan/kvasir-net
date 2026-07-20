// Public C entry for running one MoE expert-shard serve loop in-process.
//
// The desktop linkcpp-expert-worker binary wraps this with a CLI; mobile apps
// (iOS cannot exec bundled binaries) link the linkcpp-expert static library and
// call it from a worker thread instead. The call blocks for the life of the
// serve loop — the same accept/serve loop the binary runs for `--serve`.
//
// n_embd is passed in because a mini expert slice does not carry the model's
// embedding length; the caller (the hub-driven agent) supplies it per model.
#ifndef LINKCPP_EXPERT_API_H
#define LINKCPP_EXPERT_API_H

#ifdef __cplusplus
extern "C" {
#endif

// Loads the expert slice at model_path, selects a non-CPU backend when present
// (falling back to CPU), and serves expert-FFN dispatch on 127.0.0.1:port for
// the given layer. Blocks until the listener closes. Returns 0 ok, 1 failure.
int linkcpp_expert_run(const char * model_path, int port, int layer, int n_embd);

#ifdef __cplusplus
}
#endif

#endif
