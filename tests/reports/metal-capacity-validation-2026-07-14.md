# Metal controller capacity validation

## Environment

- Controller: `Mac Metal Slot 1` (`ctrl-02b066`)
- Node: `Apple M4 Pro` / 48 GiB Metal + unified-memory budget / 14 CPU cores
- Candidate: `Qwen3.5-35B-A3B-Q4_K_M.gguf` (19.72 GiB)

## Results

| Scenario | Result |
| --- | --- |
| Stale Metal-slot ownership | Reconnected to the current controller; the controller now owns one node. |
| GPU placement plan | 40 layers, estimated 21.40 GiB VRAM usage of the 48 GiB budget; placement itself fits. |
| Controller model load | Blocked before load: the Docker hub reports 0.00 GiB available master RAM. The planner requires 19.71 GiB for the mmap-resident model weights. |
| Inference | Not run because loading was correctly rejected before a runtime was started. |

The hub container has 7.75 GiB total RAM available to it (0.78 GiB in use), and the default 8 GiB master reserve leaves no loadable master-RAM capacity. Increase Docker Desktop's memory allocation to at least 28 GiB, then repeat the load and inference test.

## Screenshots

- `screenshots/metal-slot-reconnected.png`: Metal node reconnected and shown as one controller node.
- `screenshots/metal-controller-bound.png`: Controller node table showing the bound Apple M4 Pro slot and its 48 GiB / 14-core allocation.
