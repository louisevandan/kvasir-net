# Batching a decode lap, and what it is worth

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

Observed 2026-08-20 KST on this machine's two cards — RTX 4080 holding
layers [0,14) and RTX 3090 holding [14,28) — with
`Qwen2.5-1.5B-Instruct-Q8_0.gguf`, a plain attention model.

## The measurement that was wrong first

Every throughput number before this one was taken with a 5,000-token prompt
against 24 to 48 generated tokens. In that shape prefill is 99% of the run,
and `generation_tps_over_run` — generated tokens divided by the *whole* run —
is not a statement about generation at all. It read 8 to 13 tok/s and said
nothing.

Generation has to dominate before the aggregate means anything: a 32-token
prompt against 512 generated tokens.

## What it is worth

Every run completed with all four verdicts and no failure.

| Parallel | Completed | Generated | Wall | Aggregate | Per session | Mean hop width |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 16 | 16/16 | 7,042 | 25.8 s | **272.7 tok/s** | 52.6 | — (max 9) |
| 32 | 32/32 | 13,058 | 23.7 s | **550.2 tok/s** | 54.7 | 5.89 (max 19) |
| 64 | 64/64 | 24,509 | 31.9 s | **768.0 tok/s** | 34.0 | 10.89 (max 36) |

Sixteen to thirty-two is 2.02x, near linear. Thirty-two to sixty-four is
1.40x, and per-session throughput falls from 54.7 to 34.0 — the point where
concurrency starts costing latency rather than buying throughput. The useful
band on these two cards is between the two.

## Where the rest of it is

Mean hop width is about a sixth of the declared concurrency: 10.89 against 64.
The maximum reaches 36, so a wide hop is available; most hops simply go out
narrower than they could. A node sends a lap the moment it arrives, and in a
chain the laps arrive spread out.

That is the same observation that made an earlier attempt at holding a window
open pointless — but the reason has inverted. Then, a hop carried one sequence
however wide the window was, so waiting bought nothing and cost latency; the
table above is the proof that a wide hop is now genuinely cheaper per token.
Whatever gathers laps belongs in the adapter, where a stage server can hold
what it has just been handed, rather than in P4's scheduling.

## The control

Prefill is not batched, and it stays flat across the same runs — 44 to 50
tok/s per session at parallel 16 and 32 — while generation moves. That is what
says the difference is the decode and not the harness.

## Reproduction

```powershell
apps\p4\tools\scripts\e2e\run-local-real-two-stage.ps1 `
  -Model 'S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf' `
  -PromptFile 'apps\p4\tools\scripts\e2e\fixtures\prompt-tiny.txt' `
  -Requests 32 -Tokens 512 -PromptTokens 32 -Parallel 32 `
  -LayerBoundary 14 -LayerCount 28 -BatchSize 2048 -UBatchSize 512 `
  -MaxSecondaryVramMiB 13000 -ArriveMilliseconds 0 -VaryPrompts
```

with `P4_STAGED_DECODE_BATCH=1`. `P4_STAGED_TRACE_HOP=1` adds a line per
batched hop carrying its width.
