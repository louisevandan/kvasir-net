use super::*;
use p4_adapter::{Allocation, Event, EventSink, Hop};

impl Mock {
    pub(crate) fn load(&self, load: p4_adapter::Load, events: &dyn EventSink) {
        let deployment = load.deployment.clone();
        if load.artifact.is_empty() || load.plan.trim().is_empty() {
            events.raise(Event::Failed {
                deployment,
                sequence: None,
                hop_id: None,
                detail: "mock adapter requires artifact and opaque load plan".into(),
            });
            return;
        }
        self.loads
            .lock()
            .expect("load log lock")
            .push(LoadObservation {
                artifact: load.artifact.clone(),
                plan: load.plan.clone(),
                capability_snapshot_id: load.capability_snapshot_id.clone(),
                capability_expires_at: load.capability_expires_at,
            });
        self.plans.lock().expect("plan log lock").push(load.plan);
        let stages = self.profile.stages.max(1);
        let step = self.profile.stage_cost();
        for stage in 0..stages {
            spin(step);
            events.raise(Event::LoadProgress {
                deployment: deployment.clone(),
                stage,
                percent: (stage + 1) * 100 / stages,
                detail: "mock stage advancing".into(),
            });
        }
        if self.profile.fault == Fault::Load {
            events.raise(Event::Failed {
                deployment,
                sequence: None,
                hop_id: None,
                detail: "mock deployment was asked to fail its load".into(),
            });
            return;
        }
        *self.loaded.lock().expect("loaded lock") = Some(load.artifact);
        events.raise(Event::Loaded {
            generation: self.generation.fetch_add(1, Ordering::SeqCst) + 1,
            allocations: (0..stages)
                .map(|stage| Allocation {
                    category: format!("stage{stage}.declared_reservation"),
                    bytes: self.profile.reserved_per_stage,
                })
                .collect(),
            deployment,
        });
    }

    pub(crate) fn hop(&self, hop: Hop, events: &dyn EventSink) {
        let began = std::time::Instant::now();
        let first_sequence = hop.sequences.first().map(|s| s.sequence.clone());

        // Whether each sequence is beginning work here or continuing a lap.
        // `p4_adapter::Hop` carries no phase of its own to read: an execution
        // holding both is a fact about a backend, not about P4. This node's
        // own bookkeeping already answers the question a backend would ask
        // itself instead — a sequence is resident in `produced` exactly when
        // a hop here has already processed it once.
        let residency: Vec<bool> = {
            let produced = self.produced.lock().expect("produced");
            hop.sequences
                .iter()
                .map(|sequence| produced.contains_key(&sequence.sequence))
                .collect()
        };
        let hop_phase = if residency.first().copied().unwrap_or(false) {
            HopPhase::Decode
        } else {
            HopPhase::Prefill
        };

        let observation = HopObservation {
            hop_id: hop.id,
            phase: hop_phase,
            sequences: hop
                .sequences
                .iter()
                .map(|sequence| SequenceObservation {
                    sequence: sequence.sequence.clone(),
                    inbound_cut_set: crate::decode_state(sequence.state.as_ref()).1,
                    position: crate::decode_state(sequence.state.as_ref()).0,
                    prompt: sequence.prompt.clone(),
                    remaining: sequence.remaining,
                    options: sequence.options.clone(),
                })
                .collect(),
            outcomes: Vec::new(),
        };
        self.hops
            .lock()
            .expect("hop observation lock")
            .push(observation);
        if let Some(ended) = self.rested.lock().expect("rest lock").take() {
            self.idle.fetch_add(
                began.duration_since(ended).as_nanos() as u64,
                Ordering::Relaxed,
            );
        }
        self.widths
            .lock()
            .expect("width log lock")
            .push(hop.width());
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_running.fetch_max(now, Ordering::SeqCst);

        let mut valid: Vec<(p4_adapter::Sequence, bool)> = Vec::with_capacity(hop.sequences.len());
        for (sequence, resident) in hop.sequences.into_iter().zip(residency) {
            self.options
                .lock()
                .expect("options log lock")
                .push(sequence.options.clone());
            if !is_json_object(&sequence.options) {
                events.raise(Event::Failed {
                    deployment: hop.deployment.clone(),
                    sequence: Some(sequence.sequence),
                    hop_id: Some(hop.id),
                    detail: "mock adapter rejected non-object generation options".into(),
                });
            } else {
                match (resident, self.requires_restore(&sequence.sequence)) {
                    (false, Ok(true)) => events.raise(Event::Failed {
                        deployment: hop.deployment.clone(),
                        sequence: Some(sequence.sequence),
                        hop_id: Some(hop.id),
                        detail: "mock sequence is persisted and requires Restore before Hop".into(),
                    }),
                    (false, Err(error)) => events.raise(Event::Failed {
                        deployment: hop.deployment.clone(),
                        sequence: Some(sequence.sequence),
                        hop_id: Some(hop.id),
                        detail: format!("mock could not inspect durable cache: {error}"),
                    }),
                    _ => valid.push((sequence, !resident)),
                }
            }
        }
        if valid.is_empty() {
            self.running.fetch_sub(1, Ordering::SeqCst);
            return;
        }

        if self.profile.fault == Fault::Stubborn {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }

        if self.profile.fault == Fault::Silence {
            // Never answers. The node stays busy until its deadline or a
            // cancellation ends the work, which is the point of this fault.
            let started = std::time::Instant::now();
            while !events.cancelled() && started.elapsed() < std::time::Duration::from_millis(500) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            self.running.fetch_sub(1, Ordering::SeqCst);
            if events.cancelled() {
                events.raise(Event::Failed {
                    deployment: hop.deployment,
                    sequence: first_sequence,
                    hop_id: Some(hop.id),
                    detail: "mock silent hop cancelled by its deadline".into(),
                });
            }
            return;
        }
        let duration = self
            .profile
            .hop_cost(self.position, hop_phase == HopPhase::Prefill);
        if spin_until(duration, || events.cancelled()) {
            self.running.fetch_sub(1, Ordering::SeqCst);
            events.raise(Event::Failed {
                deployment: hop.deployment,
                sequence: first_sequence,
                hop_id: Some(hop.id),
                detail: "mock hop cancelled by its deadline".into(),
            });
            return;
        }
        self.running.fetch_sub(1, Ordering::SeqCst);
        // Recorded where the time was actually spent, not around the whole
        // call: what a chain is asked afterwards is how much of the wall clock
        // its stages were computing, and bookkeeping is not computing.
        self.busy
            .fetch_add(began.elapsed().as_nanos() as u64, Ordering::Relaxed);
        *self.rested.lock().expect("rest lock") = Some(std::time::Instant::now());

        if self.profile.fault == Fault::Hop {
            events.raise(Event::Failed {
                deployment: hop.deployment,
                sequence: first_sequence,
                hop_id: Some(hop.id),
                detail: "mock deployment was asked to fail its hops".into(),
            });
            return;
        }
        let outcomes = valid
            .iter()
            .map(|(sequence, is_prefill)| self.outcome(sequence, *is_prefill))
            .collect::<Vec<_>>();
        if let Some(observation) = self
            .hops
            .lock()
            .expect("hop observation lock")
            .iter_mut()
            .rev()
            .find(|observation| observation.hop_id == hop.id)
        {
            observation.outcomes = outcomes.clone();
        }
        events.raise(Event::HopComplete {
            hop_id: hop.id,
            expected: valid
                .iter()
                .map(|(sequence, _)| sequence.sequence.clone())
                .collect(),
            outcomes,
            deployment: hop.deployment,
        });
    }

    /// Counts a token for this sequence and decides whether it is finished.
    ///
    /// The count is kept here rather than read from the request each lap,
    /// because a request does not carry its own progress back down — a backend
    /// holding a sequence open is what knows how far it has got.
    fn outcome(&self, sequence: &p4_adapter::Sequence, is_prefill: bool) -> Outcome {
        // Every stage holds this sequence's attention state for its own layer
        // range — that is what pipeline parallelism is — so every stage counts
        // it as resident. Only the last one counts tokens.
        let mut produced = self.produced.lock().expect("sequence progress lock");
        let progress = produced
            .entry(sequence.sequence.clone())
            .or_insert_with(|| Progress {
                turn: 0,
                lifetime: 0,
                position: crate::decode_state(sequence.state.as_ref()).0,
            });
        progress.lifetime = progress.lifetime.saturating_add(1);
        // Token position is global to the request, not local to a pipeline
        // stage. Only the terminal stage owns logits and advances it; middle
        // stages must carry the position unchanged or an N-stage ring turns
        // one decode into N position increments and skips visible tokens.
        //
        // Prefill and a lap compute the same formula here, and always have:
        // what used to be a two-armed match on the removed `Phase` did the
        // identical thing in both arms.
        let carried = crate::decode_state(sequence.state.as_ref()).0;
        let requested_position = if self.terminal {
            carried.saturating_add(1)
        } else {
            carried
        };
        // Real decode laps carry the preceding outcome position. Older mock
        // fixture paths intentionally keep the body opaque, so they arrive
        // with position zero on every lap. Honor the carried position when it
        // is present, while retaining the resident progress as a compatibility
        // fallback for those opaque fixture requests.
        let position = if !is_prefill && self.terminal {
            requested_position.max(progress.position.saturating_add(1))
        } else {
            requested_position
        };
        // A lap that ran and said nothing. State moved — the sequence is a lap
        // further along — but no text left the backend, so neither the token
        // position nor the turn may advance here. Returning before both is the
        // whole point: a muted lap must be indistinguishable from one that has
        // not happened yet, as far as what the caller is owed.
        if self.terminal
            && self.profile.mute_every > 0
            && progress.lifetime.is_multiple_of(self.profile.mute_every)
        {
            let carried_position = progress.position;
            let lifetime = progress.lifetime;
            return Outcome {
                sequence: sequence.sequence.clone(),
                forward: Some(crate::encode_state(
                    carried_position,
                    (self.distribution == Distribution::Staged)
                        .then(|| mock_cut_set(&sequence.sequence, lifetime)),
                )),
                text: String::new(),
                stop: None,
            };
        }
        progress.position = progress.position.max(position);
        let lifetime = progress.lifetime;

        if !self.terminal {
            // A middle stage advanced its share and has nothing to say about
            // the token. Counting here would make an n-stage chain produce n
            // tokens per lap.
            return Outcome {
                sequence: sequence.sequence.clone(),
                forward: Some(crate::encode_state(
                    position,
                    (self.distribution == Distribution::Staged)
                        .then(|| mock_cut_set(&sequence.sequence, lifetime)),
                )),
                text: String::new(),
                stop: None,
            };
        }
        // The lifetime was already counted above, for every stage. Here only
        // the turn advances, because only the last stage produces tokens.
        progress.turn = progress.turn.saturating_add(1);
        // llama-server clamps max_tokens to one when the caller supplies zero.
        // Keep the same first-token-then-terminal shape instead of ending a
        // zero-valued request before it reaches the backend.
        let requested = sequence.remaining.max(1);
        let finished = progress.turn > requested;
        let output_position = if finished {
            position.saturating_sub(1)
        } else {
            position
        };
        Outcome {
            sequence: sequence.sequence.clone(),
            forward: Some(crate::encode_state(
                output_position,
                (self.distribution == Distribution::Staged)
                    .then(|| mock_cut_set(&sequence.sequence, progress.lifetime)),
            )),
            text: if finished {
                String::new()
            } else {
                format!("{}#{output_position} ", sequence.sequence)
            },
            stop: finished.then(|| "stop".to_string()),
        }
    }
}
