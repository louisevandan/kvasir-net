# Overview

P4 is a control-plane protocol. An external caller reaches an agent ingress; the local ControllerProcessor issues a session when absent, and dispatches only to a previously created NodeSlot with a ready model binding.

```text
External caller -> IngressSubmit -> ControllerProcessor -> Agent NodeSlot -> Adapter -> ConcreteRuntime
                    IngressAccepted <- TOKEN / DONE <-------------------------------
```

`NodeSlot` is model-independent: it records agent ownership, an adapter handle, and resource responsibility. `ModelLoad` creates or reuses concrete backend state at adapter discretion and records a versioned `binding_id`. `ModelUnload` removes the binding while retaining the NodeSlot.

The agent discovers host CPU/OS and NVIDIA GPU facts on `InventoryQuery`, and adapters self-register before a controller may create a slot. It never serializes native hidden states, KV pages, llama.cpp private types, or GPU handles. Those remain in each adapter's data plane.
