# Decisions

Each of these was made, or remade, against something that went wrong. The
defect is the argument.

## There is no controller

It was drawn as a participant between OUTER and the agents, and described as
holding the node list. Both were wrong. The node list is external state, and a
controller only ever had it because inference and load commands carry it. What
the controller actually was is the entry point of the agent serving as the
VPC's ingress — and that entry point is the agent. Nothing was left for a
separate object to own or decide.

An agent therefore holds one kind of internal entity. `Recipient` has two
variants, and a third would mean something acquired state it should not have.

## The address is the identity

There is no agent id. A relay has to answer "is this mine" without consulting
anything, or the answer needs state and it stops being a relay. So the target
travels in the envelope, and the address is what an agent is called.

## The chain travels whole

Carrying only the remainder would be smaller. Carrying all of it means a node
can report where in the order it sat, and a failed request can be retried
without reconstructing what it was meant to visit. Frame size is the cheaper
thing to spend.

## The hop is the unit, and it carries a window

A hop that could hold one sequence would force the node to simulate batching
above the adapter boundary — which is where a process-wide gate was already
measured putting the throttle three tiers above the GPU, so the runtime
reported a limit of fifty while sixteen arrived.

## Reporting is per stage

A model spread over layer ranges finishes when its slowest piece does. One
total figure hid a non-final stage reserving the whole model's footprint, which
was the root cause behind a long run of symptoms.

## Nothing returns a value

A response path pinned to a call stack dies when that frame returns, so long
work cannot be split into tasks while one exists. Handlers are procedures; a
requester registers a continuation. Order comes from each step registering the
next, never from the queue.

## Preference is bounded, everywhere

Two starvation bugs, one shape. The node's select preferred hop completions —
correct in intent, since a completion frees the node — and under load produced
one per hop without pause, so arrivals were never polled at all. They sat in a
channel, in no queue, invisible to every depth reading. The agent's lane
priority had the same shape.

Strict priority is not a preference but a veto. Both now yield.

## Backpressure rather than refusal, on the send path

Refusing looked safe. But a frame on the way out is usually already a reply,
and a reply carries no reply address, so there was nothing to answer with and
it went out silently — indistinguishable from a lost route. It now waits, and
the wait propagates back to the node, which is the thing producing the work.

Refusal is still right at the socket reader, where blocking would stall every
other route on that connection. The difference is that a refusal there can be
counted and reported, and the sender has a deadline.

## The mock is not a convenience

It is the second implementation that keeps the adapter interface honest. What
it can implement is the interface; that it finishes without a backend concept
is the evidence the boundary is clean. It also makes a fleet loadable anywhere,
which is what turns "P4 is not at fault" into something testable rather than
asserted.

## Measurement over argument

Three defects in this layer survived every unit test and were found by running
four processes and reading counters. Depth alone was not enough — depth shows
what is waiting, never what already left — so an agent counts what each
decision did and what each node step passed through. That is what turned a long
stretch of guessing into one measurement.
