# Test Plan: HF repository migration

Created: 2026-09-14. P4 base `5b540770d`, HF source `bc656a2`.

## Goal and environment

Verify the build, fixtures, real model behavior and restoration of a single P4 checkout on Windows/PowerShell/Rust/Python3.13.
Verify the advance backup of the original Git repository and untracked assets, and remove the original only at the very end.
Use a separate P4 worktree and new output directories. Model revision, lock, expected values and limits stay exactly as in the existing contract.

## Steps and expected results

1. MIG-RED: restore the pre-migration P4 source without the HF sibling. Preserve the Cargo metadata exit101 and the missing dependency.
2. MIG-GRAPH: HF is included in the single workspace, with exactly 1 of each P4 neutral package identity. The sibling-dependency removal mutation fails.
3. MIG-UNIT: Python suite, HF retained fixture, cache/epoch, and independent Rust/Python mutations. A missing fixture is not a success.
4. MIG-ALL: seal the final inputs, and aggregate the exits, all summaries and ignored counts of the full P4 workspace with the feature on and off.
5. MIG-RUNTIME: on a new agent, 12 fixture scenarios, the Qwen local matrix/epoch/replacement/reclaim, and normal llama generation with the feature on and off.
6. MIG-DIST: with a new worker bundle and agent, the Qwen matrix across two real hosts, then abort/re-acceptance after return-path disconnection. Resources belonging to other work are not terminated.
7. MIG-RESTORE: a locked build with the feature on and off from one source zip and a standalone restore.py, in a location without Git or the sibling.
8. MIG-DOC: full document index, links and UTF-8 LF, and 0 omissions in the file mapping. Re-verify asset copy hashes, environments and models.
9. MIG-REMOVE: bind the final P4 source and verification, preserve the changes and assets of the HF linked worktree and clean up its registration, then remove the original folder.

## Evidence

Initial counterexamples and the backup are preserved in `.cache/hf/migration/20260914/`, and run outputs in `target/hf/migration/`.
The report in `tests/reports/migration/` records commands, exits, source/binary/model hashes, errors, cleanup results and anything not run.
Failing checks are not deleted, and the BF16 tolerance is not relaxed. A small-Qwen pass is not H0–H7/SLO acceptance.
