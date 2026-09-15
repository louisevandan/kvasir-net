//! Scripted native semantics, NOT a copy of adapter state transitions. Literal
//! rows, accepted tokens and KV mutations are the oracle. Actual run/drive,
//! return, SETTLE/SETTLED and RELEASE/RELEASED remain production consumers.
//! Native Replay logits availability/checkpoint bytes/sampler acceptance are
//! NOT exercised: output=false Replay decisions are scripted at the boundary.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scenario {
    Full,
    Direct,
    Checkpoint,
}

impl Scenario {
    pub(super) fn sequence_capacity(self) -> u32 {
        if self == Self::Full { 1 } else { 2 }
    }
    fn max_tokens(self) -> u32 {
        if self == Self::Full { 5 } else { 4 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum KvEffect {
    Append(u32, i32),
    Trim(u32),
    Restore(u32),
    Release,
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub(super) struct ScriptTrace {
    effects: BTreeMap<NativeKey, Vec<KvEffect>>,
    settles: BTreeMap<NativeKey, usize>,
    released_kv: BTreeMap<NativeKey, Vec<i32>>,
    settlement_ids: BTreeMap<NativeKey, u64>,
    release_ids: BTreeMap<NativeKey, u64>,
}

#[derive(Default)]
struct Progress {
    round: usize,
    row: usize,
    awaiting_settle: bool,
    speculative_id: u64,
}

pub(super) struct NativeScript {
    scenario: Scenario,
    progress: BTreeMap<NativeKey, Progress>,
}

struct Round {
    phase: Phase,
    inputs: Vec<(u32, i32)>,
    generated_before: u32,
    output: PhysicalOutcome,
    settle: bool,
}

fn outcome(generated: &[(i32, u32, bool)], proposal: &[i32]) -> PhysicalOutcome {
    PhysicalOutcome {
        owner_index: 0,
        generated: generated
            .iter()
            .map(|&(token, position, stop)| GeneratedToken {
                token,
                position,
                text: format!("token-{token} "),
                stop: stop.then(|| "length".into()),
            })
            .collect(),
        proposal: proposal.to_vec(),
        retain_from: None,
        replay_tokens: vec![],
        replay_position: 0,
    }
}

fn rounds(scenario: Scenario, probe: bool) -> Vec<Round> {
    if probe {
        return vec![Round {
            phase: Phase::Prefill,
            inputs: vec![(0, 10)],
            generated_before: 0,
            output: outcome(&[(1000, 1, true)], &[]),
            settle: false,
        }];
    }
    let first_proposal = if scenario == Scenario::Full {
        vec![1000, 1001]
    } else {
        vec![1000, 9001]
    };
    let mut result = vec![Round {
        phase: Phase::Prefill,
        inputs: vec![(0, 10), (1, 11), (2, 12)],
        generated_before: 0,
        output: outcome(&[(1000, 3, false)], &first_proposal),
        settle: false,
    }];
    match scenario {
        Scenario::Full => result.extend([
            Round {
                phase: Phase::Verify,
                inputs: vec![(3, 1000), (4, 1001)],
                generated_before: 1,
                output: outcome(&[(1001, 4, false), (1002, 5, false)], &[1002, 1003]),
                settle: false,
            },
            Round {
                phase: Phase::Verify,
                inputs: vec![(5, 1002), (6, 1003)],
                generated_before: 3,
                output: outcome(&[(1003, 6, false), (1004, 7, true)], &[]),
                settle: false,
            },
        ]),
        Scenario::Direct => {
            let mut partial = outcome(&[(1001, 4, false)], &[]);
            partial.retain_from = Some(4);
            result.extend([
                Round {
                    phase: Phase::Verify,
                    inputs: vec![(3, 1000), (4, 9001)],
                    generated_before: 1,
                    output: partial,
                    settle: true,
                },
                Round {
                    phase: Phase::Verify,
                    inputs: vec![(4, 1001), (5, 1002)],
                    generated_before: 2,
                    output: outcome(&[(1002, 5, false), (1003, 6, true)], &[]),
                    settle: false,
                },
            ]);
        }
        Scenario::Checkpoint => {
            let mut partial = outcome(&[], &[]);
            partial.retain_from = Some(5);
            partial.replay_position = 3;
            partial.replay_tokens = vec![1000, 1001];
            result.extend([
                Round {
                    phase: Phase::Verify,
                    inputs: vec![(3, 1000), (4, 9001)],
                    generated_before: 1,
                    output: partial,
                    settle: true,
                },
                Round {
                    phase: Phase::Replay,
                    inputs: vec![(3, 1000), (4, 1001)],
                    generated_before: 1,
                    output: outcome(&[(1001, 4, false), (1002, 5, false)], &[1002]),
                    settle: false,
                },
                Round {
                    phase: Phase::Decode,
                    inputs: vec![(5, 1002)],
                    generated_before: 3,
                    output: outcome(&[(1003, 6, true)], &[]),
                    settle: false,
                },
            ]);
        }
    }
    result
}

pub(super) fn split_logical(logical: &LogicalBatch, next: &mut u64) -> Vec<PhysicalCapsule> {
    let mut capsules = Vec::new();
    let mut start = 0;
    while start < logical.0.len() {
        let first = &logical.0[start].owner;
        let atomic = matches!(first.phase, Phase::Verify | Phase::Replay);
        let mut end = start + 1;
        while end < logical.0.len() {
            let owner = &logical.0[end].owner;
            if atomic {
                if native_key(owner) != native_key(first)
                    || owner.speculative_id != first.speculative_id
                    || owner.phase != first.phase
                {
                    break;
                }
            } else if matches!(owner.phase, Phase::Verify | Phase::Replay)
                || end - start == PHYSICAL_CAPACITY
            {
                break;
            }
            end += 1;
        }
        assert!(
            end - start <= PHYSICAL_CAPACITY,
            "fixture cannot split an atomic group"
        );
        assert!(
            logical.0[start..end]
                .iter()
                .all(|row| row.token == row.owner.input_token)
        );
        let mut capsule = physical(
            *next,
            logical.0[start..end]
                .iter()
                .map(|row| row.owner.clone())
                .collect(),
        );
        if atomic {
            capsule.invocation.n_seq_tokens = capsule.owners.len() as u32;
            capsule.invocation.n_seqs = 1;
        }
        capsules.push(capsule);
        *next += 1;
        start = end;
    }
    capsules
}

impl NativeScript {
    pub(super) fn new(scenario: Scenario) -> Self {
        Self {
            scenario,
            progress: BTreeMap::new(),
        }
    }

    pub(super) fn compute(
        &mut self,
        role: NodeRole,
        trace: &mut NativeTrace,
        set: &mut CapsuleSet,
    ) -> Result<(), String> {
        for capsule in &mut set.0 {
            let mut group_start = 0;
            for (index, owner) in capsule.owners.iter().enumerate() {
                let key = native_key(owner);
                let probe = owner.request_id == "fence-probe";
                let expected = rounds(self.scenario, probe);
                if trace
                    .live
                    .keys()
                    .any(|prior| prior.0 == key.0 && prior != &key)
                {
                    return Err("script slot reused before native RELEASE".into());
                }
                let progress = self.progress.entry(key.clone()).or_default();
                if progress.awaiting_settle {
                    return Err("script append before required native SETTLE".into());
                }
                let round = expected
                    .get(progress.round)
                    .ok_or("script has no further inputs")?;
                if progress.row == 0 {
                    group_start = index;
                    if round.phase == Phase::Verify {
                        if owner.speculative_id <= progress.speculative_id {
                            return Err("new Verify did not get fresh speculative identity".into());
                        }
                        progress.speculative_id = owner.speculative_id;
                    }
                }
                let atomic = matches!(round.phase, Phase::Verify | Phase::Replay);
                let expected_output = match round.phase {
                    Phase::Prefill => progress.row + 1 == round.inputs.len(),
                    Phase::Replay => false,
                    _ => true,
                };
                if owner.phase != round.phase
                    || (owner.position, owner.input_token) != round.inputs[progress.row]
                    || owner.generated_tokens != round.generated_before
                    || owner.max_tokens != if probe { 1 } else { self.scenario.max_tokens() }
                    || owner.output != expected_output
                    || (atomic
                        && (owner.speculative_id != progress.speculative_id
                            || owner.speculative_count as usize != round.inputs.len()
                            || owner.speculative_index as usize != progress.row))
                {
                    return Err(format!(
                        "script row mismatch for {} round {} row {}: {owner:?}",
                        owner.request_id, progress.round, progress.row
                    ));
                }
                let live = trace.live.entry(key.clone()).or_default();
                if live.len() != owner.position as usize {
                    return Err(format!(
                        "script append {} follows KV {}",
                        owner.position,
                        live.len()
                    ));
                }
                live.push(owner.input_token);
                trace
                    .written
                    .entry(key.clone())
                    .or_default()
                    .push((owner.position, owner.input_token));
                trace
                    .speculative
                    .effects
                    .entry(key.clone())
                    .or_default()
                    .push(KvEffect::Append(owner.position, owner.input_token));
                progress.row += 1;
                if progress.row == round.inputs.len() {
                    if role == NodeRole::Last {
                        let mut decision = round.output.clone();
                        decision.owner_index = if atomic { group_start } else { index } as u32;
                        capsule.outcomes.push(decision);
                        trace.sampler_calls += 1;
                    }
                    progress.round += 1;
                    progress.row = 0;
                    progress.awaiting_settle = round.settle;
                } else if atomic && index + 1 == capsule.owners.len() {
                    return Err("script atomic group split across capsules".into());
                }
            }
            if role == NodeRole::Last {
                capsule.terminal = true;
                capsule.tensors.clear();
            }
        }
        Ok(())
    }

    pub(super) fn settle(
        &mut self,
        role: NodeRole,
        trace: &mut NativeTrace,
        body: &[u8],
    ) -> Result<Vec<u8>, String> {
        let (key, prefix, operation) = control_identity(body)?;
        let bytes = body.get(prefix..).ok_or("script SETTLE prefix")?;
        let expected: Vec<u8> = match self.scenario {
            Scenario::Full => return Err("full acceptance must not SETTLE".into()),
            Scenario::Direct => [4u32, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect(),
            Scenario::Checkpoint => [5u32, 3, 2, 1000, 1001]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect(),
        };
        if bytes != expected {
            return Err("script SETTLE differs from literal retain/replay contract".into());
        }
        let progress = self
            .progress
            .get_mut(&key)
            .ok_or("script SETTLE unknown identity")?;
        if !progress.awaiting_settle || progress.round != 2 || progress.row != 0 {
            return Err("script SETTLE has no pending partial verification".into());
        }
        let live = trace.live.get_mut(&key).ok_or("script SETTLE lost KV")?;
        if live != &[10, 11, 12, 1000, 9001] {
            return Err("script SETTLE initial KV differs from tentative writes".into());
        }
        // Native handle_physical_settle: replay_count!=0 selects replay_position,
        // NOT retain_from. A checkpoint restores the pre-Verify target state.
        let (keep, effect) = if self.scenario == Scenario::Direct {
            (4, KvEffect::Trim(4))
        } else {
            (3, KvEffect::Restore(3))
        };
        live.truncate(keep);
        trace
            .speculative
            .effects
            .entry(key.clone())
            .or_default()
            .push(effect);
        *trace.speculative.settles.entry(key.clone()).or_default() += 1;
        if trace
            .speculative
            .settlement_ids
            .insert(key, operation)
            .is_some()
        {
            return Err("script unexpectedly repeated native settlement".into());
        }
        progress.awaiting_settle = false;
        let proposal: &[i32] = if self.scenario == Scenario::Direct && role == NodeRole::Last {
            &[1001, 1002]
        } else {
            &[]
        };
        let mut response = body[..prefix].to_vec();
        response.extend_from_slice(&(proposal.len() as u32).to_le_bytes());
        for token in proposal {
            response.extend_from_slice(&token.to_le_bytes());
        }
        Ok(response)
    }

    pub(super) fn release(
        &mut self,
        trace: &mut NativeTrace,
        key: NativeKey,
        operation: u64,
    ) -> Result<(), String> {
        let progress = self
            .progress
            .get(&key)
            .ok_or("script RELEASE unknown request")?;
        let probe = key.1.ends_with("\0fence-probe");
        if progress.awaiting_settle
            || progress.row != 0
            || progress.round != rounds(self.scenario, probe).len()
        {
            return Err("script RELEASE before all literal rows completed".into());
        }
        if trace
            .speculative
            .settlement_ids
            .get(&key)
            .is_some_and(|prior| operation <= *prior)
        {
            return Err("script RELEASE reused an older control operation".into());
        }
        let live = trace
            .live
            .remove(&key)
            .ok_or("script RELEASE has no live KV")?;
        trace.speculative.released_kv.insert(key.clone(), live);
        trace
            .speculative
            .effects
            .entry(key.clone())
            .or_default()
            .push(KvEffect::Release);
        trace.speculative.release_ids.insert(key.clone(), operation);
        *trace.releases.entry(key).or_default() += 1;
        Ok(())
    }
}

fn control_identity(body: &[u8]) -> Result<(NativeKey, usize, u64), String> {
    // Deliberately independent of the adapter's control_identity parser.
    if body.len() < 44
        || &body[..8] != b"P4ID\x01\x00\x00\x00"
        || u64::from_le_bytes(body[8..16].try_into().unwrap()) != 1
    {
        return Err("script control header".into());
    }
    let incarnation = u64::from_le_bytes(body[16..24].try_into().unwrap());
    let operation = u64::from_le_bytes(body[24..32].try_into().unwrap());
    if incarnation == 0 || operation == 0 {
        return Err("script zero control identity".into());
    }
    let slot = u32::from_le_bytes(body[32..36].try_into().unwrap());
    let mut at = 36usize;
    let mut strings = Vec::new();
    for _ in 0..2 {
        let len = u32::from_le_bytes(
            body.get(at..at + 4)
                .ok_or("script control length")?
                .try_into()
                .unwrap(),
        ) as usize;
        at += 4;
        let end = at.checked_add(len).ok_or("script control overflow")?;
        strings.push(
            std::str::from_utf8(body.get(at..end).ok_or("script control string")?)
                .map_err(|e| e.to_string())?
                .to_owned(),
        );
        at = end;
    }
    if strings[0] != "loop-session" || !strings[1].starts_with("loop-session\0") {
        return Err("script control session".into());
    }
    Ok(((slot, strings.remove(1), incarnation), at, operation))
}

fn resume_control(h: &mut Harness) {
    h.hold_control = None;
    h.pending.extend(h.held_control.drain(..));
}

fn expected_effects(scenario: Scenario, probe: bool) -> Vec<KvEffect> {
    use KvEffect::*;
    if probe {
        return vec![Append(0, 10), Release];
    }
    let mut effects = vec![Append(0, 10), Append(1, 11), Append(2, 12), Append(3, 1000)];
    effects.extend(match scenario {
        Scenario::Full => vec![Append(4, 1001), Append(5, 1002), Append(6, 1003)],
        Scenario::Direct => vec![Append(4, 9001), Trim(4), Append(4, 1001), Append(5, 1002)],
        Scenario::Checkpoint => vec![
            Append(4, 9001),
            Restore(3),
            Append(3, 1000),
            Append(4, 1001),
            Append(5, 1002),
        ],
    });
    effects.push(Release);
    effects
}

fn finish(h: &mut Harness, scenario: Scenario, commands: &[InferenceCommand]) {
    let count: usize = commands.iter().map(|c| c.max_tokens as usize).sum();
    h.until(
        "scripted outputs and chained release acknowledgements",
        |h| {
            h.outputs.len() == count
                && h.received
                    .iter()
                    .filter(|e| {
                        e.envelope.source == endpoint(0)
                            && e.envelope.payload_content_type
                                == crate::v2::RELEASE_RECEIPT_CONTENT_TYPE
                    })
                    .map(|e| {
                        serde_json::from_slice::<crate::v2::ReleaseReceipt>(&e.payload)
                            .unwrap()
                            .members
                            .len()
                    })
                    .sum::<usize>()
                    == commands.len()
                && h.nodes
                    .iter()
                    .all(|n| n.native.lock().unwrap().releases.len() == commands.len())
        },
    );
    for command in commands {
        let probe = command.request_id == "fence-probe";
        let expected: Vec<_> = if probe {
            vec![(1000, 1, Some("length"))]
        } else if scenario == Scenario::Full {
            vec![
                (1000, 3, None),
                (1001, 4, None),
                (1002, 5, None),
                (1003, 6, None),
                (1004, 7, Some("length")),
            ]
        } else {
            vec![
                (1000, 3, None),
                (1001, 4, None),
                (1002, 5, None),
                (1003, 6, Some("length")),
            ]
        };
        let outputs: Vec<_> = h
            .outputs
            .iter()
            .filter(|o| o.request_id == command.request_id)
            .collect();
        assert_eq!(outputs.len(), command.max_tokens as usize);
        assert_eq!(
            outputs
                .iter()
                .map(|o| (o.token, o.position, o.stop.as_deref()))
                .collect::<Vec<_>>(),
            expected
        );
        assert!(
            outputs
                .iter()
                .all(|o| o.text == format!("token-{} ", o.token))
        );
        for node in &h.nodes {
            let native = node.native.lock().unwrap();
            let keys: Vec<_> = native
                .releases
                .keys()
                .filter(|key| key.1 == request_key("loop-session", &command.request_id))
                .collect();
            assert_eq!(keys.len(), 1);
            let key = keys[0];
            assert_eq!(native.releases[key], 1);
            assert_eq!(
                native.speculative.effects[key],
                expected_effects(scenario, probe)
            );
            let kv = if probe {
                vec![10]
            } else if scenario == Scenario::Full {
                vec![10, 11, 12, 1000, 1001, 1002, 1003]
            } else {
                vec![10, 11, 12, 1000, 1001, 1002]
            };
            assert_eq!(native.speculative.released_kv[key], kv);
            assert_eq!(
                native.speculative.settles.get(key).copied().unwrap_or(0),
                usize::from(!probe && scenario != Scenario::Full)
            );
            assert!(native.live.is_empty());
        }
    }
    let (settlement_ids, release_ids) = {
        let first = h.nodes[0].native.lock().unwrap();
        (
            first.speculative.settlement_ids.clone(),
            first.speculative.release_ids.clone(),
        )
    };
    for node in &h.nodes[1..] {
        let native = node.native.lock().unwrap();
        assert_eq!(
            native.speculative.settlement_ids, settlement_ids,
            "all stages must apply the same identity-bound settlement operation"
        );
        assert_eq!(
            native.speculative.release_ids, release_ids,
            "all stages must release the same identity-bound operation"
        );
    }
    h.wait_for_observations();
    super::release_notifications::assert_complete(h);
}

#[test]
fn b2_speculative_full_acceptance_uses_no_settle_and_waits_for_release_before_slot_reuse() {
    for stages in [2, 4] {
        let commands = [request("full-first", 3, 5), request("full-reuse", 3, 5)];
        let mut h = Harness::configured(stages, 2, 16, &commands, 0, Some(Scenario::Full));
        h.hold_control = Some((RELEASED_CONTENT_TYPE.into(), 0));
        h.until(
            "first request releases every stage but head ack is held",
            |h| !h.held_control.is_empty(),
        );
        h.pump_for(Duration::from_millis(30));
        assert_eq!(
            h.nodes[0].native.lock().unwrap().logical_calls,
            3,
            "pending request must not reuse slot before RELEASED"
        );
        assert_eq!(h.outputs.len(), 5);
        for node in &h.nodes {
            let n = node.native.lock().unwrap();
            assert_eq!(n.releases.len(), 1);
            assert!(n.live.is_empty());
        }
        resume_control(&mut h);
        finish(&mut h, Scenario::Full, &commands);
        for node in &h.nodes {
            let n = node.native.lock().unwrap();
            let keys: Vec<_> = n.releases.keys().collect();
            assert_eq!(keys[0].0, keys[1].0);
            assert_ne!(
                keys[0].2, keys[1].2,
                "slot reuse needs a new request incarnation"
            );
        }
    }
}

fn partial_acceptance(scenario: Scenario) {
    for stages in [2, 4] {
        let command = request("partial", 3, 4);
        let mut h = Harness::configured(stages, 2, 16, &[command.clone()], 0, Some(scenario));
        // SETTLE is a chain, not N independent acknowledgements. Hold its last
        // hop first; then separately hold the ONE tail-to-head SETTLED reply.
        h.hold_control = Some((SETTLE_CONTENT_TYPE.into(), stages - 1));
        h.until("partial result reaches last SETTLE hop", |h| {
            !h.held_control.is_empty()
        });
        let mut probe = request("fence-probe", 1, 1);
        probe.tokens.clear();
        probe.prompt = Some("Independent runnable work while a KV settlement is pending".into());
        h.enqueue(&probe);
        h.until("independent probe handled by actual head", |h| {
            h.nodes[0].native.lock().unwrap().tokenize_calls == 1
        });
        h.pump_for(Duration::from_millis(30));
        assert_eq!(
            h.nodes[0].native.lock().unwrap().logical_calls,
            2,
            "global Verify fence must also hold independently runnable work"
        );
        for (index, node) in h.nodes.iter().enumerate() {
            assert_eq!(
                node.native
                    .lock()
                    .unwrap()
                    .speculative
                    .settles
                    .values()
                    .sum::<usize>(),
                usize::from(index + 1 < stages)
            );
        }
        resume_control(&mut h);
        h.hold_control = Some((SETTLED_CONTENT_TYPE.into(), 0));
        h.until("all stages settle but aggregate completion is held", |h| {
            !h.held_control.is_empty()
        });
        h.pump_for(Duration::from_millis(30));
        assert_eq!(
            h.nodes[0].native.lock().unwrap().logical_calls,
            2,
            "tail's KV ACK must reach the head before further issue"
        );
        assert_eq!(
            h.outputs.len(),
            if scenario == Scenario::Direct { 2 } else { 1 },
            "checkpoint Verify must not emit accepted output before Replay"
        );
        for node in &h.nodes {
            assert_eq!(
                node.native
                    .lock()
                    .unwrap()
                    .speculative
                    .settles
                    .values()
                    .sum::<usize>(),
                1
            );
        }
        resume_control(&mut h);
        finish(&mut h, scenario, &[command, probe]);
        if scenario == Scenario::Checkpoint {
            let output_events: Vec<_> = h
                .received
                .iter()
                .filter(|event| event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
                .cloned()
                .collect();
            super::output_contract::assert_live_matches(
                &format!("checkpoint-{stages}"),
                &h.submissions,
                &output_events,
                &h.received,
            );
            super::output_contract::assert_live_prefill_counts(
                &format!("checkpoint-{stages}"),
                &h.received,
            );
        }
    }
}

#[test]
fn b2_speculative_direct_partial_acceptance_trims_and_obeys_the_all_stage_settlement_chain() {
    partial_acceptance(Scenario::Direct);
}

#[test]
fn b2_speculative_checkpoint_replay_restores_before_reappend_and_emits_only_after_replay() {
    partial_acceptance(Scenario::Checkpoint);
}

#[test]
fn b2_completion_full_settles_both_speculative_continuations_without_native_reentry() {
    for scenario in [Scenario::Direct, Scenario::Checkpoint] {
        let command = request("partial", 3, 4);
        let key = request_key("loop-session", &command.request_id);
        let observed = Arc::new((
            Mutex::new(None::<serde_json::Value>),
            std::sync::Condvar::new(),
        ));
        let captured = Arc::clone(&observed);
        let observer: IssueObserver = Arc::new(move |point, state| {
            if point != "after_settlement_committed" {
                return;
            }
            let request = &state.requests[&key];
            let ready = request
                .ready
                .as_ref()
                .expect("settled continuation is ready");
            let view = serde_json::json!({
                "pending": state.pending_settlements.len(),
                "fenced": state.verify_fenced(),
                "outstanding": request.outstanding,
                "generated": request.generated,
                "after_settlement": request.after_settlement.is_some(),
                "phase": format!("{:?}", ready.phase),
                "position": ready.position,
                "tokens": ready.tokens,
                "speculative_id": ready.speculative_id,
            });
            let (slot, changed) = &*captured;
            let mut slot = slot.lock().unwrap();
            assert!(slot.is_none(), "one genuine SETTLED commits only once");
            *slot = Some(view);
            changed.notify_all();
        });
        let submission = submission_event(&command, 1, default_route());
        let mut h = Harness::observed_events(
            2,
            2,
            1,
            &[submission],
            0,
            Some(scenario),
            Some(observer),
            None,
        );
        h.hold_control = Some((SETTLED_CONTENT_TYPE.into(), 0));
        h.until(
            "all native stages settled and the genuine SETTLED is held",
            |h| h.held_control.len() == 1,
        );
        let acknowledgement = h.held_control.pop_front().unwrap();
        let ack_bytes = p4_protocol::event::encode(&acknowledgement).unwrap();
        assert_eq!(acknowledgement.envelope.source, endpoint(1));
        assert_eq!(acknowledgement.envelope.target, endpoint(0));
        let ack: SettlementCommand = serde_json::from_slice(&acknowledgement.payload).unwrap();
        assert_eq!(ack.sequences.len(), 1);
        assert_eq!(ack.sequences[0].key, request_key("loop-session", "partial"));
        for node in &h.nodes {
            assert_eq!(
                node.native
                    .lock()
                    .unwrap()
                    .speculative
                    .settles
                    .values()
                    .sum::<usize>(),
                1
            );
        }
        h.pump_for(Duration::from_millis(20));
        assert!(h.pending.is_empty());
        assert_eq!(h.nodes[0].mailbox.try_take(), Poll::Empty);
        assert!(observed.0.lock().unwrap().is_none());
        let outputs_before = h.outputs.len();
        assert_eq!(
            outputs_before,
            if scenario == Scenario::Direct { 2 } else { 1 }
        );
        let native_before: Vec<_> = h
            .nodes
            .iter()
            .map(|node| format!("{:?}", *node.native.lock().unwrap()))
            .collect();

        // Genuine idempotent SESSION commands create both completions; no
        // synthetic filler or request/frontier state is inserted by the test.
        let session = SessionCommand {
            load_generation: 1,
            session_id: "loop-session".into(),
            stages: (0..2).map(node_address).collect(),
            stage_index: 0,
        };
        let sessions: Vec<_> = ["full-settled-ready-a", "full-settled-ready-b"]
            .into_iter()
            .map(|name| {
                event_wire(event(
                    0,
                    name,
                    SESSION_CONTENT_TYPE,
                    serde_json::to_vec(&session).unwrap(),
                ))
            })
            .collect();
        // Only the observational sticky status is reset, after genuine ACK
        // withholding and an empty mailbox have both been independently seen.
        *h.nodes[0].snapshot.lock().unwrap() = "fixture:awaiting_SESSION_pressure".into();
        h.paused_completions[0] = true;
        for input in &sessions {
            h.nodes[0]
                .sender
                .as_ref()
                .unwrap()
                .try_send(WorkerInput::Event(input.clone()))
                .unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while h.nodes[0].snapshot.lock().unwrap().as_str() != "completion_queue_full:waiting" {
            assert!(
                Instant::now() < deadline,
                "actual SESSION publication never reached Full"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            p4_protocol::event::encode(&acknowledgement).unwrap(),
            ack_bytes
        );
        h.nodes[0]
            .sender
            .as_ref()
            .unwrap()
            .try_send(WorkerInput::Event(event_wire(acknowledgement)))
            .unwrap();
        let (slot, changed) = &*observed;
        let (view, _) = changed
            .wait_timeout_while(slot.lock().unwrap(), Duration::from_millis(200), |view| {
                view.is_none()
            })
            .unwrap();
        let before_room = view.clone();
        drop(view);
        let native_before_room: Vec<_> = h
            .nodes
            .iter()
            .map(|node| format!("{:?}", *node.native.lock().unwrap()))
            .collect();
        assert!(!h.nodes[0].thread.as_ref().unwrap().is_finished());
        assert_eq!(h.outputs.len(), outputs_before);

        // Recover first even on the old blocking consumer, then retain every
        // existing literal token/text/position/KV/release/observation oracle.
        let Poll::Event(occupied) = h.nodes[0].mailbox.try_take() else {
            panic!("Full must contain the first real SESSION_READY");
        };
        let occupied = event_wire(occupied);
        assert_eq!(
            occupied.envelope.payload_content_type,
            SESSION_READY_CONTENT_TYPE
        );
        assert_eq!(
            occupied.envelope.causation_id.as_deref(),
            Some(sessions[0].envelope.event_id.as_str())
        );
        h.received.push(occupied);
        h.paused_completions[0] = false;
        h.hold_control = None;
        finish(&mut h, scenario, &[command]);
        for input in &sessions {
            let replies: Vec<_> = h
                .received
                .iter()
                .filter(|output| {
                    output.envelope.causation_id.as_deref()
                        == Some(input.envelope.event_id.as_str())
                })
                .collect();
            assert_eq!(replies.len(), 1);
            assert_eq!(
                replies[0].envelope.payload_content_type,
                SESSION_READY_CONTENT_TYPE
            );
            assert_eq!(replies[0].envelope.source, endpoint(0));
            assert_eq!(replies[0].envelope.target, reply_target(input).unwrap());
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&replies[0].payload).unwrap(),
                serde_json::json!({"session_id":"loop-session", "state":"ready", "load_generation":1})
            );
        }
        assert_eq!(
            native_before_room, native_before,
            "SETTLED service must perform no native work while completion remains Full"
        );
        let view =
            before_room.expect("genuine SETTLED must commit before output capacity is restored");
        assert_eq!(view["pending"], 0);
        assert_eq!(view["fenced"], false);
        assert_eq!(view["outstanding"], 0);
        assert_eq!(view["after_settlement"], false);
        assert!(view["speculative_id"].as_u64().unwrap() > 0);
        if scenario == Scenario::Direct {
            assert_eq!(view["generated"], 2);
            assert_eq!(view["phase"], "Verify");
            assert_eq!(view["position"], 4);
            assert_eq!(view["tokens"], serde_json::json!([1001, 1002]));
        } else {
            assert_eq!(view["generated"], 1);
            assert_eq!(view["phase"], "Replay");
            assert_eq!(view["position"], 3);
            assert_eq!(view["tokens"], serde_json::json!([1000, 1001]));
        }
    }
}

#[test]
fn b2_speculative_unload_refuses_head_settlement_with_no_physical_flights_then_resumes() {
    for stages in [2, 4] {
        let command = request("unload-head-partial", 3, 4);
        let mut h =
            Harness::configured(stages, 2, 16, &[command.clone()], 0, Some(Scenario::Direct));
        h.hold_control = Some((SETTLED_CONTENT_TYPE.into(), 0));
        h.until(
            "all native stages settled but head acknowledgement is held",
            |h| !h.held_control.is_empty(),
        );
        h.pump_for(Duration::from_millis(20));
        assert_eq!(
            h.outputs.len(),
            2,
            "direct acceptance has emitted two tokens"
        );
        for node in &h.nodes {
            let native = node.native.lock().unwrap();
            assert_eq!(native.speculative.settles.values().sum::<usize>(), 1);
            assert_eq!(
                native.live.values().collect::<Vec<_>>(),
                vec![&vec![10, 11, 12, 1000]]
            );
        }
        super::unload::assert_stale_unload(&mut h, 0, "stale-head-settled-held");
        super::unload::assert_stale_unload(&mut h, stages - 1, "stale-tail-settled-held");
        // Inspect the actual consumer's refusal census, not a fabricated
        // RequestState. TAIL_BATCH is already settled; its zero flight count
        // must not erase the independently pending all-stage KV barrier.
        let work = super::unload::assert_busy_unload(&mut h, 0, "busy-head-settled-held");
        assert_eq!(work["flight_batches"], 0);
        assert_eq!(work["flight_executions"], 0);
        assert_eq!(work["open_batch_view"], 0);
        assert_eq!(work["pending_settlements"], 1);
        assert_eq!(work["verify_fenced"], true);
        assert_eq!(
            h.outputs.len(),
            2,
            "busy UNLOAD must not manufacture output"
        );
        resume_control(&mut h);
        finish(&mut h, Scenario::Direct, &[command]);
        // Receipt history may remain; it is not live KV or outstanding work.
        // A completed speculative lifecycle must still be unloadable.
        for index in 0..stages {
            super::unload::assert_idle_unload(&mut h, index, &format!("idle-after-direct-{index}"));
        }
    }
}

#[test]
fn b2_speculative_unload_refuses_middle_tentative_kv_without_head_requests_then_resumes() {
    let command = request("unload-middle-checkpoint", 3, 4);
    let mut h = Harness::configured(4, 2, 16, &[command.clone()], 0, Some(Scenario::Checkpoint));
    // Every PHYSICAL Verify has executed, but stage 1 has not yet received
    // SETTLE. It owns tentative KV/frontier without owning a head RequestState.
    h.hold_control = Some((SETTLE_CONTENT_TYPE.into(), 1));
    h.until(
        "checkpoint settlement is held before the first middle stage",
        |h| !h.held_control.is_empty(),
    );
    h.pump_for(Duration::from_millis(20));
    assert_eq!(
        h.outputs.len(),
        1,
        "checkpoint acceptance emits nothing before Replay"
    );
    {
        let native = h.nodes[1].native.lock().unwrap();
        assert_eq!(native.speculative.settles.values().sum::<usize>(), 0);
        assert_eq!(
            native.live.values().collect::<Vec<_>>(),
            vec![&vec![10, 11, 12, 1000, 9001]]
        );
    }
    let work = super::unload::assert_busy_unload(&mut h, 1, "busy-middle-verify-held");
    assert_eq!(work["requests"], 0);
    assert_eq!(work["pending"], 0);
    assert_eq!(work["pending_settlements"], 0);
    assert_eq!(work["flight_batches"], 0);
    assert_eq!(work["active_owners"], 1);
    assert_eq!(work["active_frontiers"], 1);
    assert_eq!(h.outputs.len(), 1);
    resume_control(&mut h);
    finish(&mut h, Scenario::Checkpoint, &[command]);
    for index in 0..4 {
        super::unload::assert_idle_unload(&mut h, index, &format!("idle-after-checkpoint-{index}"));
    }
}
