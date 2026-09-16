# MTP/speculative minimum-slice audit

> Document status (2026-09-06): **Date- and environment-scoped evidence**. These are observations for the date, commit, model and topology stated in the body. They are not evidence that the current implementation or any other distributed environment is complete.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

Audited 2026-08-18 against pinned compat revision
`3e3a7a416d6588597523c792fc5874a543d59ed9`. The upstream checkout was not
modified.

## Conclusion

No safe execution slice can be enabled in the current staged server without
changing the HOP contract. `mtp_execution=0` and `speculative_execution=0`
remain correct.

A single-stage/full-model test can validate upstream MTP or speculative
decoding only through the upstream `common_speculative` driver or its example
program. It cannot validate staged execution: the staged runtime creates one
ordinary `llama_context`, while upstream MTP/speculative initialization creates
an additional context and runs a proposal/verification loop.

## MTP findings

The pinned upstream exposes `LLAMA_CONTEXT_TYPE_MTP` and
`llama_model_n_layer_nextn()`. `common_speculative_init_result` sets the draft
context type to MTP and creates that context against the target context. The
MTP implementation also enables next-token hidden embeddings and consumes
hidden rows across calls; it is not equivalent to passing more ordinary token
IDs through `llama_decode`.

The current staged loader cannot safely own the auxiliary tensors. The
compatibility loader assigns repeating tensors by `tn.bid` in the ordinary
layer range, while the current graph/model patches constrain the stage end to
`hparams.n_layer()`. MTP/NextN tensors use the auxiliary range at or after the
ordinary layer count. Therefore merely setting `--mtp` or changing the tail
stage flag would either leave required tensors unowned or make the ownership
and graph ranges disagree.

An auxiliary-ownership-only patch is therefore useful as a future diagnostic,
but is not an execution slice: it would need a separate ownership range,
context creation with `LLAMA_CONTEXT_TYPE_MTP`, hidden-embedding transport,
and a test model containing the matching MTP tensors. No such model is
available in the current `S:\models` root during this audit.

## Speculative findings

The pinned `common/speculative.cpp` driver owns both target and draft state.
Its loop drafts tokens, verifies them in the target, accepts a prefix, and
rolls back or advances sequence state. MTP has the same essential requirement
through a second context, hidden-row carryover, and acceptance state.

The staged runtime currently has only:

- one partial `llama_context` per stage;
- one ordinary `SequencePayload` input/output per sequence;
- one token count and one sampled outcome;
- ordinary KV save/restore/drop.

The HOP wire has no proposal token list, accepted-prefix length, target/draft
phase, or rollback checkpoint. Adding only an `accepted` list would be unsafe:
the target and draft KV states would still diverge on rejection, and a later
HOP could silently consume a state that includes rejected tokens. Rollback
must be an atomic operation over every participating context and stage.

## Safe validation boundary

The following remains safe and is already enforced by the staged server:

```text
mtp_parser=1;mtp_execution=0;
speculative_parser=1;speculative_execution=0
```

Requested execution is rejected before model loading with
`CAPABILITY_UNAVAILABLE`; parser visibility is not advertised as execution.
Ordinary decode on an MTP-named model does not count as MTP evidence.

## Required next slices

1. Add a compatibility-only auxiliary ownership/load diagnostic and validate
   it with a real MTP GGUF; keep execution capability at zero.
2. Add a local single-process target/draft test harness that proves proposal,
   acceptance, rejection, and rollback using upstream APIs without involving
   the staged HOP.
3. Only after those pass, extend staged HOP and adapter together with proposal
   lists, accepted-prefix metadata, and atomic rollback identity.
4. Turn either capability bit on only after a real acceptance/rejection/
   rollback/unload test passes for the exact staged binary.

No implementation was made in this audit because step 1 lacks a target MTP
model and the ownership patch alone would create a misleading partial state.
