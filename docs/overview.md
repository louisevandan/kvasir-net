# What this is

> Document status (2026-09-06): **Per-path reference; re-audit required**. This contains the earlier Chain/Hop description and the decisions made at the time. Current guarantees of the event path are confirmed against the code and the new verification conventions.
> Current goals, status and ordering follow the [execution roadmap](distributed-batching-roadmap.md); document authority and reading paths follow the [document map](document-map.md).

The communication layer for inference distributed across machines. It carries
work to backends and answers back; it does not run models and does not own
throughput.

## What it is not

Not a general messaging system. The workload is known in detail and the layer
is shaped to it: a model spread across nodes as layer ranges, a prefill chain
source-routed through them, a node-driven decode ring where one token costs a
full lap, and cohort batching under a declared ceiling. Anything not in service
of those is absent on purpose.

## What it generalises over

Backends that load a model across more than one device. They differ in who owns
the boundary between the pieces:

| | Owns the boundary | Chain length |
| --- | --- | --- |
| llama.cpp pipeline | we do | n |
| vLLM, SGLang | the backend does | 1 |

That is the only axis. Adding one is a name and an `Adapter` implementation.

## The claim it makes

That a problem in a running system is not this layer's.

The claim is testable, which is the point. Every step is exercised against a
mock backend that is arithmetic — no device, no runtime, nothing below to
blame — and an agent reports its lane depths beside its node depths, so a
slowdown is attributed rather than argued about.

## Terms

**OUTER** — whatever asks. Owns placement, load plans, chains, and every
business identifier.

**Agent** — a socket program with one main queue and a pool of workers. Owns
exactly one kind of internal entity: the node.

**Node** — an id, until a load connects it to a materialised adapter. Holds its
own queue, and its own long work.

**Adapter** — a backend, behind an interface that asks for hops and reports
events.

There is no controller. It existed for the firewall, meaning it was the entry
point of the agent it belonged to — and that entry point is the agent.
