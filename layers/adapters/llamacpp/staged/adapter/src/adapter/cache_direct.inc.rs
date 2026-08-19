impl StagedAdapter {
    fn cache_request(&self, cache: &p4_adapter::Cache) -> Result<(Operation, KvPayload), String> {
        let operation = match &cache.action {
            p4_adapter::CacheAction::Persist => Operation::KvSave,
            p4_adapter::CacheAction::Restore => Operation::KvRestore,
            p4_adapter::CacheAction::Discard => Operation::KvDrop,
            p4_adapter::CacheAction::Fork { .. } => {
                return Err("staged KV fork is not represented by the wire protocol".into());
            }
            p4_adapter::CacheAction::PreparePersist
            | p4_adapter::CacheAction::PrepareRestore
            | p4_adapter::CacheAction::PrepareDiscard
            | p4_adapter::CacheAction::Commit
            | p4_adapter::CacheAction::Abort
            | p4_adapter::CacheAction::Reconcile => {
                return Err("staged KV transactions are not supported by this runtime".into());
            }
        };
        let model_identity = self
            .config
            .model_identity
            .clone()
            .ok_or_else(|| "staged KV model identity is not configured".to_owned())?;
        if self.config.stage_begin < 0 || self.config.stage_end <= self.config.stage_begin {
            return Err("staged KV layer range is not configured".into());
        }
        let payload = KvPayload {
            sequence_id: cache.sequence.clone(),
            cache_key: cache.sequence.clone(),
            model_identity,
            stage_begin: self.config.stage_begin,
            stage_end: self.config.stage_end,
            flags: 0,
            expected_checksum: String::new(),
            operation_id: String::new(),
        };
        Ok((operation, payload))
    }

    fn cache_direct(&self, cache: p4_adapter::Cache, events: &dyn EventSink) -> bool {
        let deployment = cache.deployment.clone();
        let sequence = cache.sequence.clone();
        let operation_id = cache.operation_id.clone();
        let stage_id = cache.stage_id.clone();
        let generation = cache.generation;
        let action = cache.action.clone();
        let result = (|| {
            let (operation, payload) = self.cache_request(&cache)?;
            let body = payload
                .encode(self.config.protocol_limits)
                .map_err(|error| format!("cannot encode KV request: {error}"))?;
            let frame = Frame::new(operation, body)
                .map_err(|error| format!("cannot create KV request: {error}"))?;
            let response = self
                .lifecycle
                .lock()
                .map_err(|_| "staged lifecycle lock is poisoned".to_owned())?
                .request(frame)
                .map_err(|error| format!("KV request failed: {error:?}"))?;
            if response.header.operation == Operation::Error {
                let detail = String::from_utf8_lossy(&response.body);
                return Err(format!("stage server rejected KV request: {detail}"));
            }
            if response.header.operation != Operation::KvResult {
                return Err(format!(
                    "stage server returned {:?} for KV request",
                    response.header.operation
                ));
            }
            let result = KvResult::decode(&response.body, self.config.protocol_limits)
                .map_err(|error| format!("cannot decode KV response: {error}"))?;
            if result.sequence_id != sequence || result.cache_key != sequence {
                return Err("stage server KV identity does not match request".into());
            }
            Ok(result)
        })();
        match result {
            Ok(result) => {
                let verb = match action {
                    p4_adapter::CacheAction::Persist => "persisted",
                    p4_adapter::CacheAction::Restore => "restored",
                    p4_adapter::CacheAction::Discard => "discarded",
                    p4_adapter::CacheAction::Fork { .. } => "forked",
                    p4_adapter::CacheAction::PreparePersist
                    | p4_adapter::CacheAction::PrepareRestore
                    | p4_adapter::CacheAction::PrepareDiscard
                    | p4_adapter::CacheAction::Commit
                    | p4_adapter::CacheAction::Abort => "transaction",
                    p4_adapter::CacheAction::Reconcile => "reconcile",
                };
                events.raise(Event::Cached {
                    deployment,
                    stage_id,
                    generation,
                    operation_id,
                    sequence: cache.subject().clone(),
                    bytes: result.bytes,
                    detail: format!("{verb} {sequence}"),
                });
                true
            }
            Err(detail) => {
                Self::failed(events, deployment, Some(sequence), None, detail);
                false
            }
        }
    }
}
