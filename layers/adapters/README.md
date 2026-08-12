# Concrete adapter layer

`llamacpp/` and `pipeline/` translate P4 messages into backend-specific operations. Each adapter separates application use cases, adapter-owned state, and transport infrastructure. No adapter type enters `layers/protocol/` or Agent domain state.
