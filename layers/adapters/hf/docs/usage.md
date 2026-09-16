# Build and run

Every command runs with the P4 root as its working directory. Running fixtures only does not require a torch install.

```powershell
cargo build --locked -p p4-agent -p p4-event-drive --features hf-transformers
python layers/adapters/hf/scripts/testing/run.py
python layers/adapters/hf/scripts/deployment/worker/run.py target/hf/bundle-A --label A
python layers/adapters/hf/scripts/models/qwen3_5_0_8b/cli/run.py inspect --plan layers/adapters/hf/plans/qwen3_5_0_8b/single_cpu/plan.json
```

The model environment is pinned by `environments/qwen3_5_0_8b/requirements.lock`. The model preparation tool is
`scripts/models/qwen3_5_0_8b/preparation/run.py`; it creates a revision-pinned cache in the root `.cache/hf/models/`.
Model runs use that environment's Python, and `--model-dir` can point at the actual checkpoint.
The default path file is `.cache/hf/models/checkpoint-path.txt` under the P4 root. Other checkouts use their own cache or an explicit path.

Automatic loading planning runs per model in the order `profile` → `plan` → the existing `run`/`verify`.
In the request example of the [automatic planning commands](models/qwen3_5_0_8b/README.md#automatic-loading-planner),
update the available capacity and reserve to match the execution environment. Results go into a new folder under `layers/adapters/hf/target/`.
Cargo does not install environments or weights. Detailed execution and source restoration follow the [integration contract](integration/README.md).
