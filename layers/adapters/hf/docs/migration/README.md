# Migrating the standalone HF repository

On 2026-09-14, HF was integrated into P4 as an internal adapter, and build, model and two-host verification were completed.
The [completion report](../../tests/reports/migration/20260914_120000.md) records the results, failures and source binding.
All original content was moved to backup. Because of process locks and the deletion policy, only the empty original directory remains.
Baseline: P4 `5b540770d`, HF `bc656a2a09422e452a69735eb411c12899caa57e`.
The original hashes and destinations of the 128 tracked files are in the [file mapping](files.json),
and the test contract follows the [migration plan](../../tests/plans/migration-20260914.md).

The current source owner is P4 Git, and the adapter crate is built with the P4 root lock.
Initial requirements and unconnected plans are preserved in history, including the [earlier handoff](../history/initial/HANDOFF.md).
Source commits, hashes and original paths in past reports are evidence of that time, not current execution paths.

The full Git bundle of the original, the source zip and untracked evidence are kept in the local P4 `.cache/hf/migration/20260914/`.
The new model cache is `.cache/hf/models/`, the environment is `.cache/hf/environments/qwen3_5_0_8b/`, and new results go to `target/hf/`.
Environments, models and the Git backup are not included in the tracked source archive.
The existing GitHub remote is preserved; the migration work did not push or delete any remote.

The original content is preserved in the backup's `retired-project/`, and the auxiliary worktree in `retired-repair/`.
It is not developed or run as a standalone project. The existing `F:/dev/p4hfadapter/.git` and its parent are empty directories.
The execution source baseline is `38c635053`; later changes to the completion report contain documentation only.
