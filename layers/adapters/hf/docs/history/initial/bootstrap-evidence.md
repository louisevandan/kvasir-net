> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

# Standalone project initialization record

Work date: 2026-09-13. Scope: the folder, a separate Git repository, planning documents and handoff.

## What was created

- Created `F:\dev\p4hfadapter` as a new directory.
- Initialized a standalone repository with `git init -b main`. It is not a subdirectory or worktree of P4.
- Wrote documents covering per-model Python execution, the Rust/P4 boundary, quantization, verification and the future integration plan.
- No P4 files were moved or deleted; the new documents were written in this repository.
- Prepared `.gitignore` and the UTF-8/LF document rules. Models, credentials and local outputs are not tracked.

## P4 preservation baseline

Initial HEAD: `d122125bafeaa6d32790761669f1bfa5868d8078`.
The SHA256 of the 1,034 initial non-ignored tracked/untracked files, together with the Git status/HEAD, was recorded in `.local/p4-before.json`.
The existing dirty files were as follows. This list is an observation, not changes made by this project.

```text
 M README.md
 M docs/distributed-batching-roadmap.md
 M docs/document-map.md
 M test/benchmarks/cluster-inference/README.md
 M test/benchmarks/cluster-inference/placement-policy.test.ts
 M tools/cluster-inference/placement-policy.ts
?? docs/external-analysis-improvement-plan.md
?? test/benchmarks/cluster-inference/model-loading-policy.test.ts
?? tools/cluster-inference/audit-model-loading.ts
?? tools/cluster-inference/model-loading-policy.ts
```

## Final checks

- `python .local/verify_docs.py`: checked 15 Markdown files, 41 internal links and 13 P4 reference links; 0 errors.
- Passed the UTF-8/LF, final newline, code fence and required document/README index checks.
- 13 official external links are included. This local checker does not re-fetch external web pages.
- `git rev-parse --show-toplevel` is `F:/dev/p4hfadapter`, and `--git-dir` is `.git`.
- Confirmed that `.local/`, weights, `.venv/`, `target/` and `.env` are ignored, with an exception for `.env.example`.
- The Git author comes from the existing global settings, unchanged. There is no separate remote and no push.
- The initial commit contains only the full set of non-ignored new files in this repository. Confirm the exact hash with `git log -1`.

This is document verification, not the result of any Python/Rust/model/quantization/distributed real-hardware test.

## Observed concurrent P4 changes

On a re-check at 16:22 KST, the P4 HEAD had changed to `484b856ee7e53aea5b850b654c45da53cb0724a6`.
The new commit title was `feat(outer): plan model loading from typed fleet profiles`.
Against the initial 1,034, there were 1,037 non-ignored files; 6 content changes and 3 new paths were observed.
At the time of the re-check, the dirty files were the README, the roadmap, the document map, and the untracked `docs/batching-code-review.md` and
`docs/external-analysis-improvement-plan.md`.

Therefore, **this is not a verdict that the entire P4 working tree/HEAD is identical to its initial state.**
This initialization ran only read commands against P4; the only target of writes, Git initialization, document creation and commits was p4hfadapter.
P4 changes made by separate work were neither reverted nor brought into this project.
The initial hash list and the comparison result are in the ignored `.local/p4-before.json` and `.local/p4-preservation-result.json`.
These local audit files are not included in clones on other computers, so the observation summary above is kept as the basis for the handoff.

## Not done

P4 modification and integrated build, Python/Rust implementation and package installation, model download/quantization/inference,
remote deployment, process manipulation, Git remote creation and push were not performed.
