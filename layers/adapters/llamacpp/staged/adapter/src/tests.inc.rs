#[cfg(test)]
mod tests {
    use super::*;
    use p4_adapter::{Cache, CacheAction};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<p4_adapter::Event>>);

    impl p4_adapter::EventSink for Recorder {
        fn raise(&self, event: p4_adapter::Event) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn adapter() -> StagedAdapter {
        StagedAdapter::new(
            StagedConfig::new("stage-server", "127.0.0.1:1".parse().unwrap()).with_kv_metadata(
                "model.gguf",
                8,
                16,
            ),
        )
    }

    fn cache(action: CacheAction) -> Cache {
        Cache {
            deployment: "deployment".into(),
            stage_id: "stage-1".into(),
            generation: 3,
            operation_id: "request-7".into(),
            sequence: "sequence-7".into(),
            action,
        }
    }

    #[test]
    fn cache_actions_map_to_the_cpp_kv_operations() {
        let adapter = adapter();
        for (action, operation) in [
            (CacheAction::Persist, Operation::KvSave),
            (CacheAction::Restore, Operation::KvRestore),
            (CacheAction::Discard, Operation::KvDrop),
        ] {
            let (actual, payload) = adapter.cache_request(&cache(action)).unwrap();
            assert_eq!(actual, operation);
            assert_eq!(payload.sequence_id, "sequence-7");
            assert_eq!(payload.cache_key, "sequence-7");
            assert_eq!(payload.model_identity, "model.gguf");
            assert_eq!((payload.stage_begin, payload.stage_end), (8, 16));
            assert!(payload.operation_id.is_empty());
        }
    }

    #[test]
    fn fork_is_not_mistaken_for_a_durable_wire_operation() {
        let error = adapter()
            .cache_request(&cache(CacheAction::Fork {
                into: "branch".into(),
            }))
            .unwrap_err();
        assert!(error.contains("not represented by the wire protocol"));
    }

    #[test]
    fn staged_transaction_barrier_is_process_local_and_fail_closed_on_reconcile() {
        let adapter = adapter();
        let recorder = Recorder::default();
        adapter.start(
            p4_adapter::Work::Cache(cache(CacheAction::PreparePersist)),
            &recorder,
        );
        assert!(matches!(
            recorder.0.lock().unwrap().last(),
            Some(p4_adapter::Event::Cached { detail, .. }) if detail == "prepared"
        ));

        adapter.start(
            p4_adapter::Work::Cache(cache(CacheAction::Reconcile)),
            &recorder,
        );
        assert!(matches!(
            recorder.0.lock().unwrap().last(),
            Some(p4_adapter::Event::CacheStatus {
                state: p4_adapter::CacheReceiptState::Prepared,
                ..
            })
        ));

        adapter.start(
            p4_adapter::Work::Cache(cache(CacheAction::Abort)),
            &recorder,
        );
        adapter.start(
            p4_adapter::Work::Cache(cache(CacheAction::Reconcile)),
            &recorder,
        );
        assert!(matches!(
            recorder.0.lock().unwrap().last(),
            Some(p4_adapter::Event::CacheStatus {
                state: p4_adapter::CacheReceiptState::Absent,
                ..
            })
        ));
    }

    #[test]
    fn staged_transaction_phases_require_the_full_cache_identity() {
        let adapter = adapter();
        let recorder = Recorder::default();
        adapter.start(
            p4_adapter::Work::Cache(cache(CacheAction::PreparePersist)),
            &recorder,
        );

        let mut wrong = cache(CacheAction::Commit);
        wrong.stage_id = "other-stage".into();
        adapter.start(p4_adapter::Work::Cache(wrong.clone()), &recorder);
        assert!(matches!(
            recorder.0.lock().unwrap().last(),
            Some(p4_adapter::Event::Failed { detail, .. })
                if detail == "staged cache commit identity mismatch"
        ));

        wrong.action = CacheAction::Reconcile;
        adapter.start(p4_adapter::Work::Cache(wrong), &recorder);
        assert!(matches!(
            recorder.0.lock().unwrap().last(),
            Some(p4_adapter::Event::CacheStatus {
                state: p4_adapter::CacheReceiptState::Inconsistent,
                ..
            })
        ));
    }

    fn sequence(id: &str, state: Option<Vec<u8>>) -> p4_adapter::Sequence {
        p4_adapter::Sequence {
            sequence: id.into(),
            state,
            prompt: None,
            remaining: 2,
            options: "{}".into(),
        }
    }

    #[test]
    fn hop_input_preserves_one_inbound_cut_set_per_sequence() {
        let adapter = adapter();
        let cut_set = SequencePayload {
            sequence_id: "seq-middle".into(),
            descriptors: Vec::new(),
            payloads: Vec::new(),
            n_tokens: None,
            prompt: None,
            initial_tokens: None,
            options: String::new(),
            position: Some(0),
            outcome: None,
        }
        .encode(ProtocolLimits::default())
        .unwrap();
        let payload = adapter
            .sequence_payload(&sequence("seq-middle", Some(cut_set.clone())))
            .unwrap();
        assert_eq!(payload.sequence_id, "seq-middle");
        assert_eq!(payload.encode(ProtocolLimits::default()).unwrap(), cut_set);
    }

    #[test]
    fn hop_input_without_cut_set_is_a_valid_opaque_stage0_placeholder() {
        let adapter = adapter();
        let payload = adapter
            .sequence_payload(&sequence("seq-first", None))
            .unwrap();
        assert_eq!(payload.sequence_id, "seq-first");
        assert!(payload.descriptors.is_empty());
        assert!(payload.payloads.is_empty());
        // The current local wire has no phase/prompt/token fields. This is
        // intentionally only a framing path, not proof of stage-0 tokenize.
    }

    #[test]
    fn hop_input_copies_request_options_into_the_local_payload() {
        let adapter = adapter();
        let mut sequence = sequence("seq-options", None);
        sequence.options = r#"{"temperature":0,"grammar":"root ::= \"A\""}"#.into();
        let payload = adapter.sequence_payload(&sequence).unwrap();
        assert_eq!(payload.options, sequence.options);
        let decoded = HopPayload::decode(
            &HopPayload {
                phase: HopPhase::Prefill,
                sequences: vec![payload],
                legacy: false,
            }
            .encode(ProtocolLimits::default())
            .unwrap(),
            ProtocolLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.sequences[0].options, sequence.options);
    }

    #[test]
    fn hop_input_rejects_a_cut_set_for_another_sequence() {
        let adapter = adapter();
        let cut_set = SequencePayload {
            sequence_id: "seq-other".into(),
            descriptors: Vec::new(),
            payloads: Vec::new(),
            n_tokens: None,
            prompt: None,
            initial_tokens: None,
            options: String::new(),
            position: Some(0),
            outcome: None,
        }
        .encode(ProtocolLimits::default())
        .unwrap();
        let error = adapter
            .sequence_payload(&sequence("seq-expected", Some(cut_set)))
            .unwrap_err();
        assert!(error.contains("does not match"));
    }

    #[test]
    fn sampled_metadata_projects_into_the_existing_p4_outcome() {
        let sequence = sequence("tail", None);
        let outcome = outcome_from_result(
            &sequence,
            Some(vec![1, 2, 3]),
            Some(OutcomeMetadata {
                token: 42,
                text: "hello".into(),
                position: 1,
                stop: None,
            }),
        );
        assert_eq!(outcome.sequence, "tail");
        // The token and the position it reached are the adapter's, and they
        // travel inside the state rather than beside it.
        assert_eq!(outcome.forward, Some(vec![1, 2, 3]));
        assert_eq!(outcome.text, "hello");
        assert_eq!(outcome.stop, None);
    }

    #[test]
    fn intermediate_decode_does_not_advance_logical_position() {
        let sequence = sequence("middle", None);
        let outcome = outcome_from_result(&sequence, Some(vec![1]), None);
        // A stage that does not sample says nothing to the requester and
        // hands its state on. There is no position here to advance or hold:
        // the position moved inside the state, which is what stops a
        // four-stage chain spending four of them on one token.
        assert_eq!(outcome.forward, Some(vec![1]));
        assert!(outcome.text.is_empty());
        assert_eq!(outcome.stop, None);
    }
}
