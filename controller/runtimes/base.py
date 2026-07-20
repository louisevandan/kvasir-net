"""Mode names shared by the controller without importing either runtime."""

LLAMA_RPC = "llama_rpc"
RING_PROXY = "ring_proxy"
DEFAULT_RUNTIME_MODE = LLAMA_RPC
RUNTIME_MODES = (LLAMA_RPC, RING_PROXY)


def normalize_runtime_mode(value: object) -> str:
    mode = str(value or DEFAULT_RUNTIME_MODE).strip().lower().replace("-", "_")
    if mode not in RUNTIME_MODES:
        raise ValueError(f"unsupported runtime mode: {value}")
    return mode
