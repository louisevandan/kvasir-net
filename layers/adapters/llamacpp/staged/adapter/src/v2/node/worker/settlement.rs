use super::*;

impl Worker {
    pub(super) fn settle(&mut self, event: Event) -> Result<(), String> {
        let mut command: SettlementCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid settlement payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("settlement load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&command.session_id)
            .ok_or_else(|| "settlement session is not configured".to_owned())?
            .clone();
        if session.command.role() == NodeRole::First {
            return Err("settlement cannot re-enter the first node".into());
        }
        self.require_stage_source(
            &event,
            session
                .previous
                .as_ref()
                .ok_or("settlement has no declared predecessor")?,
            "settlement",
        )?;
        if command
            .sequences
            .iter()
            .any(|sequence| !sequence.proposal.is_empty())
        {
            return Err("settlement cannot carry a proposal before the terminal node".into());
        }
        self.ensure_event_id_obligations(1, 0)?;
        self.settle_stage_sequences(&mut command.sequences)?;
        let (target, content_type) = if let Some(next) = session.next {
            if command
                .sequences
                .iter()
                .any(|sequence| !sequence.proposal.is_empty())
            {
                self.effects_fenced = true;
                return Err("non-terminal settlement produced a proposal".into());
            }
            (next, SETTLE_CONTENT_TYPE)
        } else {
            (session.first, SETTLED_CONTENT_TYPE)
        };
        self.effects
            .push_back(super::effects::CommittedEffect::Forward {
                base: event.envelope,
                target,
                class: EventClass::Control,
                content_type,
                body: serde_json::to_vec(&command).map_err(|error| error.to_string())?,
            });
        self.flush_effects()
    }

    pub(super) fn settled(&mut self, event: Event) -> Result<(), String> {
        let command: SettlementCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid settlement completion payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("settlement completion load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&command.session_id)
            .ok_or_else(|| "settlement completion session is not configured".to_owned())?;
        if session.command.role() != NodeRole::First {
            return Err("settlement completion must target the first node".into());
        }
        self.require_stage_source(&event, &session.last, "settlement completion")?;
        // The physical fragment has already retired. This command confirms
        // the separate KV effect; manufacturing another outstanding fragment
        // would conflate two different barriers. Prepare the entire event,
        // including proposal identities and fence changes, before writing.
        let mut seen = std::collections::BTreeSet::new();
        let mut seen_sequences = std::collections::BTreeSet::new();
        let mut candidates = Vec::with_capacity(command.sequences.len());
        let mut next_speculative_id = self.state.next_speculative_id;
        for sequence in &command.sequences {
            if !seen.insert(sequence.key.clone()) || !seen_sequences.insert(sequence.id) {
                return Err("settlement completion repeats a request".into());
            }
            let request = self
                .state
                .requests
                .get(&sequence.key)
                .ok_or_else(|| "settled request is no longer active".to_owned())?;
            if request.sequence_id != Some(sequence.id)
                || request.incarnation != sequence.incarnation
                || self
                    .state
                    .pending_settlements
                    .get(&sequence.key)
                    .is_none_or(|expected| {
                        !expected
                            .dispatch
                            .allows_ack(command.load_generation, &command.session_id)
                            || expected.sequence.incarnation != sequence.incarnation
                            || expected.sequence.operation_id != sequence.operation_id
                            || expected.sequence.key != sequence.key
                            || expected.sequence.id != sequence.id
                            || expected.sequence.retain_from != sequence.retain_from
                            || expected.sequence.replay_position != sequence.replay_position
                            || expected.sequence.replay_tokens != sequence.replay_tokens
                    })
                || sequence.id >= self.state.sequence_capacity
                || request.command.load_generation != command.load_generation
                || request.command.session_id != command.session_id
                || request_key(&request.command.session_id, &request.command.request_id)
                    != sequence.key
                || request.outstanding != 0
                || request.ready.is_some()
                || request.prompt_cursor != request.command.tokens.len()
                || !self.state.verify_fence_matches(&sequence.key)
                || sequence.retain_from > i32::MAX as u32
            {
                return Err("settled sequence does not match its pending KV barrier".into());
            }
            let continuation = request
                .after_settlement
                .clone()
                .ok_or_else(|| "settled request lost its KV continuation".to_owned())?;
            let remaining = request
                .command
                .max_tokens
                .checked_sub(request.generated)
                .filter(|remaining| *remaining > 0)
                .ok_or_else(|| "settled request has no generation budget".to_owned())?;
            let ready = match continuation {
                super::super::state::SettlementContinuation::Replay(ready) => {
                    if !sequence.proposal.is_empty()
                        || sequence.replay_tokens.is_empty()
                        || ready.phase != Phase::Replay
                        || ready.speculative_id == 0
                        || ready.tokens != sequence.replay_tokens
                        || ready.position != sequence.replay_position
                        || u32::try_from(ready.tokens.len())
                            .ok()
                            .and_then(|count| ready.position.checked_add(count))
                            != Some(sequence.retain_from)
                        || ready.tokens.iter().any(|token| *token < 0)
                    {
                        return Err("settlement replay rows changed in flight".into());
                    }
                    ready
                }
                super::super::state::SettlementContinuation::Proposal { position, token } => {
                    if !sequence.replay_tokens.is_empty()
                        || sequence.replay_position != 0
                        || sequence.retain_from != position
                        || sequence.proposal.first() != Some(&token)
                        || sequence.proposal.iter().any(|token| *token < 0)
                        || sequence.proposal.len() > remaining as usize
                    {
                        return Err("direct settlement boundary, proposal or budget changed".into());
                    }
                    super::proposal::ready_from_proposal(
                        &mut next_speculative_id,
                        sequence.proposal.clone(),
                        position,
                    )?
                }
            };
            if ready.tokens.len() > self.state.physical_capacity {
                return Err("settlement continuation exceeds atomic physical capacity".into());
            }
            let mut candidate = request.clone();
            candidate.ready = Some(ready);
            candidate.after_settlement = None;
            candidates.push((sequence.key.clone(), candidate));
        }
        // No native call, publication or fallible interpretation follows the
        // first write. This worker is the sole mutator and keys are unique, so
        // each fence proven present above remains present until this removal.
        self.state.next_speculative_id = next_speculative_id;
        for (key, request) in candidates {
            self.state.pending_settlements.remove(&key);
            self.state.requests.insert(key.clone(), request);
            self.state
                .finish_verify_fence(&key)
                .expect("validated unique KV settlement fence remains present");
        }
        #[cfg(test)]
        self.observe_issue_state("after_settlement_committed");
        Ok(())
    }

    pub(super) fn settle_stage_sequences(
        &mut self,
        sequences: &mut [SettlementSequence],
    ) -> Result<(), String> {
        let mut prepared = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for sequence in sequences.iter() {
            if !ids.insert(sequence.id) {
                return Err("settlement repeats a slot".into());
            }
            let identity =
                self.operation_identity(&sequence.key, sequence.id, sequence.incarnation)?;
            let (prefix, body) = crate::v2::control_identity::settlement(
                self.state.load_generation,
                &identity.session_id,
                sequence,
            )?;
            let response_bound = self
                .state
                .physical_capacity
                .checked_mul(4)
                .and_then(|n| n.checked_add(prefix.len() + 4))
                .ok_or("settlement response bound overflow")?;
            super::super::ownership::StageOwners::validate_control_sizes(
                body.len(),
                response_bound,
            )?;
            let check =
                self.state
                    .stage_owners
                    .check_control(&identity, sequence.operation_id, &body)?;
            if matches!(check, super::super::ownership::ControlCheck::New) {
                // Validate every affected sequence before the first native
                // trim/restore. A new operation ID does not authorize rewind.
                self.state.stage_frontiers.prepare_settlement(
                    &identity,
                    sequence.retain_from,
                    sequence.replay_position,
                    &sequence.replay_tokens,
                )?;
            }
            prepared.push((identity, prefix, body, check));
        }
        let controls: Vec<_> = prepared
            .iter()
            .zip(sequences.iter())
            .map(|((identity, _, body, _), sequence)| {
                (identity.clone(), sequence.operation_id, body.clone())
            })
            .collect();
        self.state.stage_owners.validate_control_batch(&controls)?;
        for (sequence, (identity, prefix, body, check)) in sequences.iter_mut().zip(prepared) {
            // Slots are unique and the worker is serial. Re-prepare the delta
            // at this revision after the whole-event semantic preflight above.
            let frontier = if matches!(check, super::super::ownership::ControlCheck::New) {
                Some(self.state.stage_frontiers.prepare_settlement(
                    &identity,
                    sequence.retain_from,
                    sequence.replay_position,
                    &sequence.replay_tokens,
                )?)
            } else {
                None
            };
            let result = match check {
                super::super::ownership::ControlCheck::Replay(result) => result,
                super::super::ownership::ControlCheck::New => self.stage_request(
                    Operation::PhysicalSettle,
                    Operation::PhysicalSettle,
                    body.clone(),
                )?,
            };
            sequence.proposal = crate::v2::control_identity::settlement_reply(&prefix, &result)
                .map_err(|error| {
                    self.effects_fenced = true;
                    error
                })?;
            super::super::frontier::validate_continuation_width(
                &sequence.proposal,
                &sequence.replay_tokens,
                self.state.physical_capacity,
            )
            .map_err(|error| {
                // A native trim/restore may already have taken effect. Do
                // not publish SETTLED or create a successful control receipt.
                self.effects_fenced = true;
                error
            })?;
            let frontier = if let Some(frontier) = frontier {
                let terminal = self
                    .state
                    .sessions
                    .get(&identity.session_id)
                    .is_some_and(|session| session.command.role() == NodeRole::Last);
                Some(
                    self.state
                        .stage_frontiers
                        .complete_settlement(frontier, &sequence.proposal, terminal)
                        .map_err(|error| {
                            self.effects_fenced = true;
                            error
                        })?,
                )
            } else {
                None
            };
            self.state
                .stage_owners
                .commit_control(&identity, sequence.operation_id, &body, &result, false)
                .map_err(|error| {
                    self.effects_fenced = true;
                    error
                })?;
            if let Some(frontier) = frontier {
                self.state
                    .stage_frontiers
                    .commit(frontier)
                    .map_err(|error| {
                        self.effects_fenced = true;
                        error
                    })?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::state::{ReadyRows, SettlementContinuation};
    use super::*;
    use p4_adapter::node_adapter::completion_mailbox;

    fn pending(name: &str, id: u32, replay: bool) -> RequestState {
        let mut request = crate::v2::tests::request_state(vec![7; 4]);
        request.command.session_id = "pipeline".into();
        request.command.request_id = name.into();
        request.command.load_generation = 1;
        request.command.max_tokens = 16;
        request.sequence_id = Some(id);
        request.prompt_cursor = 4;
        request.prompt_issued = 4;
        request.generated = 2;
        request.outstanding = 0;
        request.ready = None;
        request.after_settlement = Some(if replay {
            SettlementContinuation::Replay(ReadyRows {
                phase: Phase::Replay,
                tokens: vec![7, 8],
                position: 4,
                speculative_id: 5,
            })
        } else {
            SettlementContinuation::Proposal {
                position: 6,
                token: 9,
            }
        });
        request
    }

    fn acknowledgement(name: &str, id: u32, replay: bool) -> SettlementSequence {
        SettlementSequence {
            incarnation: 1,
            operation_id: 1,
            key: request_key("pipeline", name),
            id,
            retain_from: 6,
            replay_tokens: if replay { vec![7, 8] } else { Vec::new() },
            replay_position: if replay { 4 } else { 0 },
            proposal: if replay { Vec::new() } else { vec![9, 10] },
        }
    }

    fn worker(requests: Vec<RequestState>) -> Worker {
        let address = Address::tcp("127.0.0.1", 42001);
        let endpoint = Endpoint::node(address, "first", 1);
        let (_sender, receiver) = mpsc::channel();
        let (publisher, _mailbox) = completion_mailbox(8);
        let mut worker = Worker::new(
            endpoint.clone(),
            receiver,
            publisher,
            Arc::new(Mutex::new(String::new())),
            Arc::new(AtomicBool::new(false)),
        );
        worker.state.load_generation = 1;
        worker.state.sequence_capacity = 8;
        worker.state.physical_capacity = 32;
        worker.state.next_speculative_id = 10;
        worker.state.sessions.insert(
            "pipeline".into(),
            PipelineSession {
                command: SessionCommand {
                    load_generation: 1,
                    session_id: "pipeline".into(),
                    stages: ["first", "tail"]
                        .into_iter()
                        .map(|name| NodeAddress {
                            agent: "127.0.0.1:42001".into(),
                            node: name.into(),
                            generation: 1,
                        })
                        .collect(),
                    stage_index: 0,
                },
                next: Some(Endpoint::node(Address::tcp("127.0.0.1", 42001), "tail", 1)),
                first: endpoint,
                previous: None,
                last: Endpoint::node(Address::tcp("127.0.0.1", 42001), "tail", 1),
            },
        );
        let keys: Vec<_> = requests
            .iter()
            .map(|request| request_key("pipeline", &request.command.request_id))
            .collect();
        worker.state.begin_verify_fence(&keys).unwrap();
        for (key, request) in keys.into_iter().zip(requests) {
            let mut expected = acknowledgement(
                &request.command.request_id,
                request.sequence_id.unwrap(),
                matches!(
                    request.after_settlement,
                    Some(SettlementContinuation::Replay(_))
                ),
            );
            expected.proposal.clear();
            worker.state.pending_settlements.insert(
                key.clone(),
                super::super::super::state::PendingSettlement {
                    sequence: expected,
                    // This consumer fixture represents an already dispatched
                    // control. Actual native/forward progression is tested by
                    // the full worker-loop fixtures, not manufactured here.
                    dispatch: super::super::super::state::ControlDispatch {
                        load_generation: 1,
                        session_id: "pipeline".into(),
                        phase: super::super::super::state::ControlDispatchPhase::ForwardAccepted,
                    },
                },
            );
            worker.state.requests.insert(key, request);
        }
        worker
    }

    fn event(worker: &Worker, sequences: Vec<SettlementSequence>) -> Event {
        let mut event = worker
            .state
            .requests
            .values()
            .next()
            .unwrap()
            .template
            .clone();
        event.envelope.payload_content_type = SETTLED_CONTENT_TYPE.into();
        event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "tail", 1);
        event.envelope.target = worker.endpoint.clone();
        event.payload = serde_json::to_vec(&SettlementCommand {
            load_generation: 1,
            session_id: "pipeline".into(),
            sequences,
        })
        .unwrap();
        event
    }

    fn snapshot(worker: &Worker) -> serde_json::Value {
        serde_json::json!({
            "next": worker.state.next_speculative_id,
            "pending_controls": worker.state.pending_settlements,
            "requests": worker.state.requests.iter().map(|(key, request)| {
                serde_json::json!({ "key": key, "generated": request.generated,
                    "outstanding": request.outstanding, "cursor": request.prompt_cursor,
                    "ready": request.ready.as_ref().map(|ready| format!("{ready:?}")),
                    "pending": match &request.after_settlement {
                        None => String::from("none"),
                        Some(SettlementContinuation::Replay(ready)) => format!("replay:{ready:?}"),
                        Some(SettlementContinuation::Proposal { position, token }) => format!("proposal:{position}:{token}"),
                    }, "fenced": worker.state.verify_fence_matches(key),
                })
            }).collect::<Vec<_>>()
        })
    }

    #[test]
    fn kv_acknowledgement_uses_pending_kv_not_physical_outstanding() {
        let mut worker = worker(vec![pending("a", 0, false)]);
        let event = event(&worker, vec![acknowledgement("a", 0, false)]);
        worker
            .settled(event)
            .expect("physical fragment is already settled; KV continuation is authoritative");
        let request = &worker.state.requests[&request_key("pipeline", "a")];
        assert_eq!(request.outstanding, 0);
        assert_eq!(request.generated, 2);
        assert!(request.after_settlement.is_none());
        let ready = request.ready.as_ref().unwrap();
        assert_eq!(
            (ready.phase, ready.position, ready.speculative_id),
            (Phase::Verify, 6, 10)
        );
        assert_eq!(ready.tokens, vec![9, 10]);
        assert_eq!(worker.state.next_speculative_id, 11);
        assert!(!worker.state.verify_fenced());
    }

    #[test]
    fn checkpoint_acknowledgement_prepares_exact_replay_rows() {
        let mut worker = worker(vec![pending("a", 0, true)]);
        let event = event(&worker, vec![acknowledgement("a", 0, true)]);
        worker.settled(event).unwrap();
        let request = &worker.state.requests[&request_key("pipeline", "a")];
        let ready = request.ready.as_ref().unwrap();
        assert_eq!(
            (ready.phase, ready.position, ready.speculative_id),
            (Phase::Replay, 4, 5)
        );
        assert_eq!(ready.tokens, vec![7, 8]);
        assert_eq!((request.outstanding, request.generated), (0, 2));
        assert!(request.after_settlement.is_none());
        assert_eq!(worker.state.next_speculative_id, 10);
        assert!(!worker.state.verify_fenced());
    }

    #[test]
    fn a_valid_mixed_acknowledgement_commits_both_continuations() {
        let mut worker = worker(vec![pending("a", 0, false), pending("b", 1, true)]);
        let event = event(
            &worker,
            vec![
                acknowledgement("a", 0, false),
                acknowledgement("b", 1, true),
            ],
        );
        worker.settled(event).unwrap();
        let first = &worker.state.requests[&request_key("pipeline", "a")];
        let second = &worker.state.requests[&request_key("pipeline", "b")];
        assert_eq!(first.ready.as_ref().unwrap().phase, Phase::Verify);
        assert_eq!(second.ready.as_ref().unwrap().phase, Phase::Replay);
        assert!(first.after_settlement.is_none() && second.after_settlement.is_none());
        assert_eq!((first.outstanding, second.outstanding), (0, 0));
        assert_eq!(worker.state.next_speculative_id, 11);
        assert!(!worker.state.verify_fenced());
    }

    #[test]
    fn late_malformed_ack_preserves_every_request_fence_and_speculative_counter() {
        for reversed in [false, true] {
            let mut worker = worker(vec![pending("a", 0, false), pending("b", 1, true)]);
            let before = snapshot(&worker);
            let mut sequences = vec![
                acknowledgement("a", 0, false),
                acknowledgement("b", 1, true),
            ];
            sequences[1].replay_tokens[1] += 1;
            if reversed {
                sequences.reverse();
            }
            let event = event(&worker, sequences);
            assert!(worker.settled(event).is_err());
            assert_eq!(snapshot(&worker), before);
        }
    }

    #[test]
    fn acknowledgement_does_not_consume_a_pending_physical_fragment() {
        let mut request = pending("a", 0, false);
        request.outstanding = 1;
        let mut worker = worker(vec![request]);
        let before = snapshot(&worker);
        let event = event(&worker, vec![acknowledgement("a", 0, false)]);
        assert!(worker.settled(event).is_err());
        assert_eq!(snapshot(&worker), before);
    }

    #[test]
    fn proposal_ack_cannot_replace_the_already_sampled_token() {
        for reversed in [false, true] {
            let mut worker = worker(vec![pending("a", 0, false), pending("b", 1, false)]);
            let before = snapshot(&worker);
            let mut sequences = vec![
                acknowledgement("a", 0, false),
                acknowledgement("b", 1, false),
            ];
            // Still a valid token and unchanged length/range: only comparison
            // with the stored tail decision can detect this substitution.
            sequences[1].proposal[0] += 1;
            if reversed {
                sequences.reverse();
            }
            let event = event(&worker, sequences);
            assert!(worker.settled(event).is_err());
            assert_eq!(snapshot(&worker), before);
        }
    }

    #[test]
    fn changed_retain_position_tokens_and_sequence_are_refused_before_write() {
        for mutation in 0..8 {
            let replay = mutation < 4;
            let mut worker = worker(vec![pending("a", 0, replay)]);
            let before = snapshot(&worker);
            let mut sequence = acknowledgement("a", 0, replay);
            match mutation {
                0 => {
                    sequence.replay_position += 1;
                    sequence.retain_from += 1;
                }
                1 => {
                    sequence.replay_tokens.push(9);
                    sequence.retain_from += 1;
                }
                2 => sequence.replay_tokens[0] += 1,
                3 => sequence.retain_from += 1,
                4 => sequence.retain_from += 1,
                5 => sequence.id += 1,
                6 => sequence.key = request_key("pipeline", "unknown"),
                _ => sequence.proposal = vec![-1],
            }
            let event = event(&worker, vec![sequence]);
            assert!(worker.settled(event).is_err(), "mutation {mutation}");
            assert_eq!(snapshot(&worker), before, "mutation {mutation}");
        }
    }

    #[test]
    fn duplicate_members_and_exhausted_proposal_identity_are_atomic_refusals() {
        let mut worker = worker(vec![pending("a", 0, false), pending("b", 1, false)]);
        let sequence = acknowledgement("a", 0, false);
        let before = snapshot(&worker);
        let duplicate = event(&worker, vec![sequence.clone(), sequence]);
        assert!(worker.settled(duplicate).is_err());
        assert_eq!(snapshot(&worker), before);
        worker.state.next_speculative_id = u64::MAX - 1;
        let before = snapshot(&worker);
        let event = event(
            &worker,
            vec![
                acknowledgement("a", 0, false),
                acknowledgement("b", 1, false),
            ],
        );
        assert!(worker.settled(event).is_err());
        assert_eq!(snapshot(&worker), before);
    }

    #[test]
    fn repeated_ack_without_receipt_is_refused_without_reapplying_state() {
        let mut worker = worker(vec![pending("a", 0, false)]);
        let event = event(&worker, vec![acknowledgement("a", 0, false)]);
        worker.settled(event.clone()).unwrap();
        let before = snapshot(&worker);
        assert!(worker.settled(event).is_err());
        assert_eq!(snapshot(&worker), before);
    }

    #[test]
    fn old_incarnation_or_operation_cannot_consume_a_reused_pending_kv_barrier() {
        for field in ["incarnation", "operation"] {
            let mut worker = worker(vec![pending("a", 0, false)]);
            let key = request_key("pipeline", "a");
            worker.state.requests.get_mut(&key).unwrap().incarnation = 2;
            let pending = worker.state.pending_settlements.get_mut(&key).unwrap();
            pending.sequence.incarnation = 2;
            pending.sequence.operation_id = 3;
            let mut valid = acknowledgement("a", 0, false);
            valid.incarnation = 2;
            valid.operation_id = 3;
            let mut stale = valid.clone();
            if field == "incarnation" {
                stale.incarnation = 1;
            } else {
                stale.operation_id = 2;
            }
            let before = snapshot(&worker);
            let stale_event = event(&worker, vec![stale]);
            assert!(
                worker
                    .settled(stale_event)
                    .unwrap_err()
                    .contains("pending KV barrier")
            );
            assert_eq!(
                snapshot(&worker),
                before,
                "old {field} changed new request or authority"
            );
            let valid_event = event(&worker, vec![valid]);
            worker.settled(valid_event).unwrap();
            assert!(worker.state.pending_settlements.is_empty());
            assert!(!worker.state.verify_fenced());
            assert!(worker.state.requests[&key].ready.is_some());
        }
    }
}
