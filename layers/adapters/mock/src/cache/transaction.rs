use super::*;
use p4_adapter::{Event, EventSink};

impl Mock {
    pub(crate) fn cache(&self, cache: p4_adapter::Cache, events: &dyn EventSink) {
        spin(self.profile.trailing_hop);
        let refuse = |detail: String| {
            events.raise(Event::Failed {
                deployment: cache.deployment.clone(),
                sequence: Some(cache.sequence.clone()),
                hop_id: None,
                detail,
            })
        };
        if let Some(error) = &self.cache_journal_error {
            return refuse(format!("cache journal recovery failed: {error}"));
        }
        if matches!(cache.action, p4_adapter::CacheAction::Commit)
            && self.profile.fault == crate::profile::Fault::CacheCommit
        {
            return refuse(format!("mock cache commit failed for {}", cache.sequence));
        }
        let (bytes, detail) = match &cache.action {
            p4_adapter::CacheAction::Reconcile => {
                let receipt = if let Some(receipt) = self
                    .aborted
                    .lock()
                    .expect("aborted")
                    .get(&cache.operation_id)
                    .cloned()
                {
                    Some((p4_adapter::CacheReceiptState::Aborted, receipt))
                } else if let Some(receipt) = self
                    .committed
                    .lock()
                    .expect("committed")
                    .get(&cache.operation_id)
                    .cloned()
                {
                    Some((p4_adapter::CacheReceiptState::Committed, receipt))
                } else {
                    self.prepared
                        .lock()
                        .expect("prepared")
                        .get(&cache.operation_id)
                        .cloned()
                        .map(|receipt| (p4_adapter::CacheReceiptState::Prepared, receipt))
                };
                let (state, bytes, detail) = match receipt {
                    Some((state, receipt)) => {
                        manifest::reconcile_receipt(self, &cache, state, &receipt)
                    }
                    None => (
                        p4_adapter::CacheReceiptState::Absent,
                        0,
                        "receipt state=absent".into(),
                    ),
                };
                events.raise(Event::CacheStatus {
                    deployment: cache.deployment.clone(),
                    stage_id: cache.stage_id.clone(),
                    generation: cache.generation,
                    operation_id: cache.operation_id.clone(),
                    sequence: cache.sequence.clone(),
                    state,
                    bytes,
                    detail,
                });
                return;
            }
            p4_adapter::CacheAction::PreparePersist => {
                let Some(progress) = self
                    .produced
                    .lock()
                    .expect("produced")
                    .get(&cache.sequence)
                    .copied()
                else {
                    return refuse(format!("nothing resident for {}", cache.sequence));
                };
                let bytes =
                    u64::from(progress.lifetime + 1) * self.profile.reserved_per_stage.max(1_024);
                let previous = match self.available_bytes(&cache) {
                    Ok(value) => value,
                    Err(error) => {
                        return refuse(format!(
                            "could not read durable cache {}: {error}",
                            cache.sequence
                        ));
                    }
                };
                let prepared = PreparedCache::Persist {
                    identity: CacheIdentity {
                        deployment: cache.deployment.clone(),
                        stage_id: cache.stage_id.clone(),
                        generation: cache.generation,
                        sequence: cache.sequence.clone(),
                    },
                    bytes,
                    previous,
                    resident: self
                        .produced
                        .lock()
                        .expect("produced")
                        .get(&cache.sequence)
                        .copied(),
                };
                if let Err(error) = self.persist_prepared(&cache.operation_id, &prepared) {
                    return refuse(format!(
                        "could not journal persist {}: {error}",
                        cache.sequence
                    ));
                }
                self.prepared
                    .lock()
                    .expect("prepared")
                    .insert(cache.operation_id.clone(), prepared);
                (bytes, format!("prepared persist {}", cache.sequence))
            }
            p4_adapter::CacheAction::Persist => {
                // Progress is what there is to persist. A sequence that has
                // run further has more state, which is the whole reason an
                // operator wants it off the device.
                let Some(progress) = self
                    .produced
                    .lock()
                    .expect("produced")
                    .get(&cache.sequence)
                    .copied()
                else {
                    return refuse(format!("nothing resident for {}", cache.sequence));
                };
                let bytes =
                    u64::from(progress.lifetime + 1) * self.profile.reserved_per_stage.max(1_024);
                if let Err(error) =
                    self.write_durable(&cache_identity(&cache), bytes, progress.position)
                {
                    return refuse(format!("could not persist {}: {error}", cache.sequence));
                }
                self.produced
                    .lock()
                    .expect("produced")
                    .remove(&cache.sequence);
                self.persisted
                    .lock()
                    .expect("persisted")
                    .insert(cache.sequence.clone(), bytes);
                (bytes, format!("persisted and freed {}", cache.sequence))
            }
            p4_adapter::CacheAction::PrepareRestore => {
                let state = match self.available_bytes(&cache) {
                    Ok(value) => value,
                    Err(error) => {
                        return refuse(format!(
                            "could not read durable cache {}: {error}",
                            cache.sequence
                        ));
                    }
                };
                let Some(state) = state else {
                    return refuse(format!("nothing persisted for {}", cache.sequence));
                };
                let prepared = PreparedCache::Restore {
                    identity: CacheIdentity {
                        deployment: cache.deployment.clone(),
                        stage_id: cache.stage_id.clone(),
                        generation: cache.generation,
                        sequence: cache.sequence.clone(),
                    },
                    bytes: state.bytes,
                    position: state.position,
                    resident: self
                        .produced
                        .lock()
                        .expect("produced")
                        .get(&cache.sequence)
                        .copied(),
                };
                if let Err(error) = self.persist_prepared(&cache.operation_id, &prepared) {
                    return refuse(format!(
                        "could not journal restore {}: {error}",
                        cache.sequence
                    ));
                }
                self.prepared
                    .lock()
                    .expect("prepared")
                    .insert(cache.operation_id.clone(), prepared);
                (state.bytes, format!("prepared restore {}", cache.sequence))
            }
            p4_adapter::CacheAction::Restore => {
                let state = match self.available_bytes(&cache) {
                    Ok(Some(state)) => state,
                    Ok(None) => return refuse(format!("nothing persisted for {}", cache.sequence)),
                    Err(error) => {
                        return refuse(format!(
                            "could not read durable cache {}: {error}",
                            cache.sequence
                        ));
                    }
                };
                // The size is what the copy was, so the progress it stood for
                // comes back with it — a restore that forgot how far the
                // conversation had got would be a restore in name only. The
                // durable copy remains available, as it does after a real
                // llama_state_seq restore; Discard is the operation that
                // removes it.
                let lifetime = (state.bytes / self.profile.reserved_per_stage.max(1_024))
                    .saturating_sub(1) as u32;
                self.produced
                    .lock()
                    .expect("produced")
                    // The mock encodes the durable conversation progress in
                    // its opaque size. Restoring at the lifetime offset keeps
                    // the next visible token monotonic across a persist/
                    // restore cycle instead of silently restarting at zero.
                    .insert(
                        cache.sequence.clone(),
                        Progress {
                            turn: lifetime,
                            lifetime,
                            position: state.position,
                        },
                    );
                (state.bytes, format!("restored {}", cache.sequence))
            }
            p4_adapter::CacheAction::Fork { into } => {
                let state = match self.available_bytes(&cache) {
                    Ok(Some(state)) => state,
                    Ok(None) => return refuse(format!("nothing persisted for {}", cache.sequence)),
                    Err(error) => {
                        return refuse(format!(
                            "could not read durable cache {}: {error}",
                            cache.sequence
                        ));
                    }
                };
                // Copied, never aliased. Two branches that shared state would
                // each corrupt the other the moment either continued.
                let mut persisted = self.persisted.lock().expect("persisted");
                persisted.insert(into.clone(), state.bytes);
                let mut target = cache_identity(&cache);
                target.sequence = into.clone();
                if let Err(error) = self.write_durable(&target, state.bytes, state.position) {
                    return refuse(format!("could not fork {into}: {error}"));
                }
                (
                    state.bytes,
                    format!("forked {} into {into}", cache.sequence),
                )
            }
            p4_adapter::CacheAction::PrepareDiscard => {
                let state = match self.available_bytes(&cache) {
                    Ok(Some(state)) => state,
                    Ok(None) => return refuse(format!("nothing persisted for {}", cache.sequence)),
                    Err(error) => {
                        return refuse(format!(
                            "could not read durable cache {}: {error}",
                            cache.sequence
                        ));
                    }
                };
                let prepared = PreparedCache::Discard {
                    identity: CacheIdentity {
                        deployment: cache.deployment.clone(),
                        stage_id: cache.stage_id.clone(),
                        generation: cache.generation,
                        sequence: cache.sequence.clone(),
                    },
                    previous: Some(state),
                };
                if let Err(error) = self.persist_prepared(&cache.operation_id, &prepared) {
                    return refuse(format!(
                        "could not journal discard {}: {error}",
                        cache.sequence
                    ));
                }
                self.prepared
                    .lock()
                    .expect("prepared")
                    .insert(cache.operation_id.clone(), prepared);
                (state.bytes, format!("prepared discard {}", cache.sequence))
            }
            p4_adapter::CacheAction::Discard => {
                let removed = self
                    .persisted
                    .lock()
                    .expect("persisted")
                    .remove(&cache.sequence);
                let durable = self.remove_durable(&cache.sequence);
                if removed.is_none() && !durable.unwrap_or(false) {
                    return refuse(format!("nothing persisted for {}", cache.sequence));
                }
                (0, format!("discarded {}", cache.sequence))
            }
            p4_adapter::CacheAction::Commit => {
                let prepared = self
                    .prepared
                    .lock()
                    .expect("prepared")
                    .get(&cache.operation_id)
                    .cloned()
                    .or_else(|| {
                        self.committed
                            .lock()
                            .expect("committed")
                            .get(&cache.operation_id)
                            .cloned()
                    });
                let Some(prepared) = prepared else {
                    if self
                        .aborted
                        .lock()
                        .expect("aborted")
                        .contains_key(&cache.operation_id)
                    {
                        return refuse(format!(
                            "cache operation {} was already aborted",
                            cache.operation_id
                        ));
                    }
                    return refuse(format!(
                        "no prepared cache operation {}",
                        cache.operation_id
                    ));
                };
                let receipt = prepared.clone();
                match prepared {
                    PreparedCache::Persist {
                        identity,
                        bytes,
                        resident,
                        ..
                    } if identity.matches(&cache) => {
                        if self
                            .committed
                            .lock()
                            .expect("committed")
                            .contains_key(&cache.operation_id)
                        {
                            (bytes, format!("commit already applied {}", cache.sequence))
                        } else {
                            let position = resident.map_or(0, |progress| progress.position);
                            if let Err(error) = self.write_durable(&identity, bytes, position) {
                                return refuse(format!(
                                    "could not commit persist {}: {error}",
                                    cache.sequence
                                ));
                            }
                            self.produced
                                .lock()
                                .expect("produced")
                                .remove(&cache.sequence);
                            self.persisted
                                .lock()
                                .expect("persisted")
                                .insert(cache.sequence.clone(), bytes);
                            if let Err(error) = journal::mark_committed(
                                self.cache_dir.as_deref(),
                                &cache.operation_id,
                                &receipt,
                            ) {
                                return refuse(format!(
                                    "committed persist but could not mark journal {}: {error}",
                                    cache.operation_id
                                ));
                            }
                            let committed = self
                                .prepared
                                .lock()
                                .expect("prepared")
                                .remove(&cache.operation_id)
                                .expect("prepared cache disappeared");
                            self.committed
                                .lock()
                                .expect("committed")
                                .insert(cache.operation_id.clone(), committed);
                            (bytes, format!("committed persist {}", cache.sequence))
                        }
                    }
                    PreparedCache::Restore {
                        identity,
                        bytes,
                        position,
                        ..
                    } if identity.matches(&cache) => {
                        if self
                            .committed
                            .lock()
                            .expect("committed")
                            .contains_key(&cache.operation_id)
                        {
                            (bytes, format!("commit already applied {}", cache.sequence))
                        } else {
                            let lifetime = (bytes / self.profile.reserved_per_stage.max(1_024))
                                .saturating_sub(1)
                                as u32;
                            self.produced.lock().expect("produced").insert(
                                cache.sequence.clone(),
                                Progress {
                                    turn: lifetime,
                                    lifetime,
                                    position,
                                },
                            );
                            if let Err(error) = journal::mark_committed(
                                self.cache_dir.as_deref(),
                                &cache.operation_id,
                                &receipt,
                            ) {
                                return refuse(format!(
                                    "committed restore but could not mark journal {}: {error}",
                                    cache.operation_id
                                ));
                            }
                            let committed = self
                                .prepared
                                .lock()
                                .expect("prepared")
                                .remove(&cache.operation_id)
                                .expect("prepared cache disappeared");
                            self.committed
                                .lock()
                                .expect("committed")
                                .insert(cache.operation_id.clone(), committed);
                            (bytes, format!("committed restore {}", cache.sequence))
                        }
                    }
                    PreparedCache::Discard { identity, .. } if identity.matches(&cache) => {
                        if self
                            .committed
                            .lock()
                            .expect("committed")
                            .contains_key(&cache.operation_id)
                        {
                            (0, format!("commit already applied {}", cache.sequence))
                        } else {
                            if let Err(error) = self.remove_durable(&cache.sequence) {
                                return refuse(format!(
                                    "could not commit discard {}: {error}",
                                    cache.sequence
                                ));
                            }
                            self.persisted
                                .lock()
                                .expect("persisted")
                                .remove(&cache.sequence);
                            if let Err(error) = journal::mark_committed(
                                self.cache_dir.as_deref(),
                                &cache.operation_id,
                                &receipt,
                            ) {
                                return refuse(format!(
                                    "committed discard but could not mark journal {}: {error}",
                                    cache.operation_id
                                ));
                            }
                            let committed = self
                                .prepared
                                .lock()
                                .expect("prepared")
                                .remove(&cache.operation_id)
                                .expect("prepared cache disappeared");
                            self.committed
                                .lock()
                                .expect("committed")
                                .insert(cache.operation_id.clone(), committed);
                            (0, format!("committed discard {}", cache.sequence))
                        }
                    }
                    _ => return refuse("cache commit identity does not match prepare".into()),
                }
            }
            p4_adapter::CacheAction::Abort => {
                if self
                    .aborted
                    .lock()
                    .expect("aborted")
                    .contains_key(&cache.operation_id)
                {
                    return events.raise(Event::Cached {
                        deployment: cache.deployment.clone(),
                        stage_id: cache.stage_id.clone(),
                        generation: cache.generation,
                        operation_id: cache.operation_id.clone(),
                        sequence: cache.sequence.clone(),
                        bytes: 0,
                        detail: format!("abort already applied {}", cache.sequence),
                    });
                }
                let prepared = self
                    .prepared
                    .lock()
                    .expect("prepared")
                    .get(&cache.operation_id)
                    .cloned()
                    .or_else(|| {
                        self.committed
                            .lock()
                            .expect("committed")
                            .get(&cache.operation_id)
                            .cloned()
                    });
                let Some(prepared) = prepared else {
                    return refuse(format!(
                        "no prepared cache operation {}",
                        cache.operation_id
                    ));
                };
                let receipt = prepared.clone();
                match prepared {
                    PreparedCache::Persist {
                        identity,
                        previous,
                        resident,
                        ..
                    } if identity.matches(&cache) => {
                        match previous {
                            Some(state) => {
                                if let Err(error) =
                                    self.write_durable(&identity, state.bytes, state.position)
                                {
                                    return refuse(format!("could not abort persist: {error}"));
                                }
                                self.persisted
                                    .lock()
                                    .expect("persisted")
                                    .insert(cache.sequence.clone(), state.bytes);
                            }
                            None => {
                                if let Err(error) = self.remove_durable(&cache.sequence) {
                                    return refuse(format!("could not abort persist: {error}"));
                                }
                                self.persisted
                                    .lock()
                                    .expect("persisted")
                                    .remove(&cache.sequence);
                            }
                        }
                        self.produced
                            .lock()
                            .expect("produced")
                            .remove(&cache.sequence);
                        if let Some(progress) = resident {
                            self.produced
                                .lock()
                                .expect("produced")
                                .insert(cache.sequence.clone(), progress);
                        }
                    }
                    PreparedCache::Restore {
                        identity, resident, ..
                    } if identity.matches(&cache) => {
                        let mut produced = self.produced.lock().expect("produced");
                        produced.remove(&cache.sequence);
                        if let Some(progress) = resident {
                            produced.insert(cache.sequence.clone(), progress);
                        }
                    }
                    PreparedCache::Discard { identity, previous } if identity.matches(&cache) => {
                        if let Some(state) = previous {
                            if let Err(error) =
                                self.write_durable(&identity, state.bytes, state.position)
                            {
                                return refuse(format!("could not abort discard: {error}"));
                            }
                            self.persisted
                                .lock()
                                .expect("persisted")
                                .insert(cache.sequence.clone(), state.bytes);
                        }
                    }
                    _ => return refuse("cache abort identity does not match prepare".into()),
                }
                if let Err(error) =
                    journal::mark_aborted(self.cache_dir.as_deref(), &cache.operation_id, &receipt)
                {
                    return refuse(format!("could not clear aborted journal: {error}"));
                }
                self.prepared
                    .lock()
                    .expect("prepared")
                    .remove(&cache.operation_id);
                self.committed
                    .lock()
                    .expect("committed")
                    .remove(&cache.operation_id);
                self.aborted
                    .lock()
                    .expect("aborted")
                    .insert(cache.operation_id.clone(), receipt);
                (0, format!("aborted cache operation {}", cache.operation_id))
            }
        };
        events.raise(Event::Cached {
            deployment: cache.deployment.clone(),
            stage_id: cache.stage_id.clone(),
            generation: cache.generation,
            operation_id: cache.operation_id.clone(),
            sequence: cache.subject().clone(),
            bytes,
            detail,
        });
    }
}
