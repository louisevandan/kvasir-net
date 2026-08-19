impl StagedAdapter {
    fn prepare_cache(&self, cache: p4_adapter::Cache, events: &dyn EventSink) {
        let Some(kind) = TransactionKind::from_action(&cache.action) else {
            Self::failed(
                events,
                cache.deployment,
                Some(cache.sequence),
                None,
                "staged cache prepare requires a prepare action",
            );
            return;
        };
        let operation_id = cache.operation_id.clone();
        let subject = cache.subject().clone();
        let mut transactions = self.transactions.lock().expect("staged transaction lock");
        if let Some(existing) = transactions.get(&operation_id) {
            if existing.cache != cache || existing.kind != kind {
                Self::failed(
                    events,
                    cache.deployment,
                    Some(cache.sequence),
                    None,
                    "staged cache operation identity was reused with different intent",
                );
                return;
            }
            events.raise(Event::Cached {
                deployment: cache.deployment,
                stage_id: cache.stage_id,
                generation: cache.generation,
                operation_id,
                sequence: subject,
                bytes: 0,
                detail: "already prepared".into(),
            });
            return;
        }
        transactions.insert(
            operation_id.clone(),
            PendingCache {
                cache: cache.clone(),
                kind,
            },
        );
        events.raise(Event::Cached {
            deployment: cache.deployment,
            stage_id: cache.stage_id,
            generation: cache.generation,
            operation_id,
            sequence: subject,
            bytes: 0,
            detail: "prepared".into(),
        });
    }

    fn abort_cache(&self, cache: p4_adapter::Cache, events: &dyn EventSink) {
        let subject = cache.subject().clone();
        let mut transactions = self.transactions.lock().expect("staged transaction lock");
        let Some(pending) = transactions.get(&cache.operation_id) else {
            drop(transactions);
            Self::failed(
                events,
                cache.deployment,
                Some(cache.sequence),
                None,
                "staged cache abort has no prepared operation",
            );
            return;
        };
        if !same_cache_identity(&pending.cache, &cache) {
            drop(transactions);
            Self::failed(
                events,
                cache.deployment,
                Some(cache.sequence),
                None,
                "staged cache abort identity mismatch",
            );
            return;
        }
        let removed = transactions.remove(&cache.operation_id).is_some();
        events.raise(Event::Cached {
            deployment: cache.deployment,
            stage_id: cache.stage_id,
            generation: cache.generation,
            operation_id: cache.operation_id,
            sequence: subject,
            bytes: 0,
            detail: if removed {
                "aborted"
            } else {
                "already aborted"
            }
            .into(),
        });
    }

    fn reconcile_cache(&self, cache: p4_adapter::Cache, events: &dyn EventSink) {
        let state = self
            .transactions
            .lock()
            .expect("staged transaction lock")
            .get(&cache.operation_id)
            .map(|pending| {
                if same_cache_identity(&pending.cache, &cache) {
                    p4_adapter::CacheReceiptState::Prepared
                } else {
                    p4_adapter::CacheReceiptState::Inconsistent
                }
            })
            .unwrap_or(p4_adapter::CacheReceiptState::Absent);
        events.raise(Event::CacheStatus {
            deployment: cache.deployment,
            stage_id: cache.stage_id,
            generation: cache.generation,
            operation_id: cache.operation_id,
            sequence: cache.sequence,
            state,
            bytes: 0,
            detail: "process-local staged receipt".into(),
        });
    }

    fn commit_cache(&self, cache: p4_adapter::Cache, events: &dyn EventSink) {
        let pending = self
            .transactions
            .lock()
            .expect("staged transaction lock")
            .get(&cache.operation_id)
            .cloned();
        let Some(pending) = pending else {
            Self::failed(
                events,
                cache.deployment,
                Some(cache.sequence),
                None,
                "staged cache commit has no prepared operation",
            );
            return;
        };
        if !same_cache_identity(&pending.cache, &cache) {
            Self::failed(
                events,
                cache.deployment,
                Some(cache.sequence),
                None,
                "staged cache commit identity mismatch",
            );
            return;
        }
        let mut mutation = pending.cache;
        mutation.action = pending.kind.action();
        if self.cache_direct(mutation, events) {
            self.transactions
                .lock()
                .expect("staged transaction lock")
                .remove(&cache.operation_id);
        }
    }

    fn cache(&self, cache: p4_adapter::Cache, events: &dyn EventSink) {
        let native_transactions = self.state() == LoadState::Loaded
            && self
                .lifecycle
                .lock()
                .map(|lifecycle| lifecycle.transaction_capable())
                .unwrap_or(false);
        if native_transactions
            && matches!(
                cache.action,
                p4_adapter::CacheAction::PreparePersist
                    | p4_adapter::CacheAction::PrepareRestore
                    | p4_adapter::CacheAction::PrepareDiscard
                    | p4_adapter::CacheAction::Commit
                    | p4_adapter::CacheAction::Abort
                    | p4_adapter::CacheAction::Reconcile
            )
        {
            let _ = self.cache_transaction_native(cache, events);
            return;
        }
        match cache.action {
            p4_adapter::CacheAction::PreparePersist
            | p4_adapter::CacheAction::PrepareRestore
            | p4_adapter::CacheAction::PrepareDiscard => self.prepare_cache(cache, events),
            p4_adapter::CacheAction::Commit => self.commit_cache(cache, events),
            p4_adapter::CacheAction::Abort => self.abort_cache(cache, events),
            p4_adapter::CacheAction::Reconcile => self.reconcile_cache(cache, events),
            _ => {
                let _ = self.cache_direct(cache, events);
            }
        }
    }

    fn transaction_request(
        &self,
        cache: &p4_adapter::Cache,
    ) -> Result<(Operation, KvPayload), String> {
        let (operation, flags) = match &cache.action {
            p4_adapter::CacheAction::PreparePersist => (Operation::KvPrepare, 1),
            p4_adapter::CacheAction::PrepareRestore => (Operation::KvPrepare, 2),
            p4_adapter::CacheAction::PrepareDiscard => (Operation::KvPrepare, 3),
            p4_adapter::CacheAction::Commit => (Operation::KvCommit, 0),
            p4_adapter::CacheAction::Abort => (Operation::KvAbort, 0),
            p4_adapter::CacheAction::Reconcile => (Operation::KvReconcile, 0),
            _ => return Err("not a staged transaction action".into()),
        };
        let model_identity = self
            .config
            .model_identity
            .clone()
            .ok_or_else(|| "staged KV model identity is not configured".to_owned())?;
        if self.config.stage_begin < 0 || self.config.stage_end <= self.config.stage_begin {
            return Err("staged KV layer range is not configured".into());
        }
        if cache.operation_id.is_empty() {
            return Err("staged KV transaction operation_id is required".into());
        }
        Ok((
            operation,
            KvPayload {
                sequence_id: cache.sequence.clone(),
                cache_key: cache.sequence.clone(),
                model_identity,
                stage_begin: self.config.stage_begin,
                stage_end: self.config.stage_end,
                flags,
                expected_checksum: String::new(),
                operation_id: cache.operation_id.clone(),
            },
        ))
    }

    fn cache_transaction_native(&self, cache: p4_adapter::Cache, events: &dyn EventSink) -> bool {
        let deployment = cache.deployment.clone();
        let sequence = cache.sequence.clone();
        let operation_id = cache.operation_id.clone();
        let stage_id = cache.stage_id.clone();
        let generation = cache.generation;
        let action = cache.action.clone();
        let result = (|| {
            let (operation, payload) = self.transaction_request(&cache)?;
            let body = payload
                .encode(self.config.protocol_limits)
                .map_err(|error| format!("cannot encode KV transaction: {error}"))?;
            let frame = Frame::new(operation, body)
                .map_err(|error| format!("cannot create KV transaction: {error}"))?;
            let response = self
                .lifecycle
                .lock()
                .map_err(|_| "staged lifecycle lock is poisoned".to_owned())?
                .request(frame)
                .map_err(|error| format!("KV transaction failed: {error:?}"))?;
            if response.header.operation == Operation::Error {
                return Err(format!(
                    "stage server rejected KV transaction: {}",
                    String::from_utf8_lossy(&response.body)
                ));
            }
            if response.header.operation != Operation::KvReceipt {
                return Err(format!(
                    "stage server returned {:?} for KV transaction",
                    response.header.operation
                ));
            }
            let receipt = KvReceipt::decode(&response.body, self.config.protocol_limits)
                .map_err(|error| format!("cannot decode KV receipt: {error}"))?;
            if receipt.operation_id != operation_id
                || receipt.sequence_id != sequence
                || receipt.cache_key != sequence
            {
                return Err("stage server KV receipt identity does not match request".into());
            }
            Ok(receipt)
        })();
        match result {
            Ok(receipt) => {
                if matches!(action, p4_adapter::CacheAction::Reconcile) {
                    let state = match receipt.state {
                        KvReceiptState::Absent => p4_adapter::CacheReceiptState::Absent,
                        KvReceiptState::Prepared => p4_adapter::CacheReceiptState::Prepared,
                        KvReceiptState::Committed => p4_adapter::CacheReceiptState::Committed,
                        KvReceiptState::Aborted => p4_adapter::CacheReceiptState::Aborted,
                        KvReceiptState::Inconsistent | KvReceiptState::Committing => {
                            p4_adapter::CacheReceiptState::Inconsistent
                        }
                    };
                    events.raise(Event::CacheStatus {
                        deployment,
                        stage_id,
                        generation,
                        operation_id,
                        sequence,
                        state,
                        bytes: receipt.bytes,
                        detail: receipt.detail,
                    });
                } else {
                    let expected = match action {
                        p4_adapter::CacheAction::PreparePersist
                        | p4_adapter::CacheAction::PrepareRestore
                        | p4_adapter::CacheAction::PrepareDiscard => KvReceiptState::Prepared,
                        p4_adapter::CacheAction::Commit => KvReceiptState::Committed,
                        p4_adapter::CacheAction::Abort => KvReceiptState::Aborted,
                        _ => unreachable!("native transaction action was validated above"),
                    };
                    if receipt.state != expected {
                        Self::failed(
                            events,
                            deployment,
                            Some(sequence),
                            None,
                            format!(
                                "native KV transaction ended in {:?}, expected {:?}",
                                receipt.state, expected
                            ),
                        );
                        return false;
                    }
                    events.raise(Event::Cached {
                        deployment,
                        stage_id,
                        generation,
                        operation_id,
                        sequence: cache.subject().clone(),
                        bytes: receipt.bytes,
                        detail: receipt.detail,
                    });
                }
                true
            }
            Err(detail) => {
                Self::failed(events, deployment, Some(sequence), None, detail);
                false
            }
        }
    }
}
