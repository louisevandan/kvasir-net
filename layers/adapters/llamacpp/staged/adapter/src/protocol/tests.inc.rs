#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trips_with_little_endian_header() {
        let frame = Frame::new(Operation::Hop, vec![1, 2, 3, 4]).unwrap();
        let encoded = frame.encode(ProtocolLimits::default()).unwrap();
        assert_eq!(&encoded[..4], b"LCP4");
        assert_eq!(&encoded[4..6], &PROTOCOL_REVISION.to_le_bytes());
        assert_eq!(
            Frame::decode(&encoded, ProtocolLimits::default()).unwrap(),
            frame
        );
    }

    #[test]
    fn rejects_unknown_operation_and_bad_length() {
        let mut unknown = Frame::new(Operation::Hop, vec![])
            .unwrap()
            .encode(ProtocolLimits::default())
            .unwrap();
        unknown[6] = 99;
        assert_eq!(
            Frame::decode(&unknown, ProtocolLimits::default()),
            Err(FrameError::UnknownOperation(99))
        );

        let mut truncated = Frame::new(Operation::Hop, vec![1, 2])
            .unwrap()
            .encode(ProtocolLimits::default())
            .unwrap();
        truncated[8] = 3;
        assert!(matches!(
            Frame::decode(&truncated, ProtocolLimits::default()),
            Err(FrameError::LengthMismatch { .. })
        ));
    }

    #[test]
    fn enforces_frame_limit() {
        let frame = Frame::new(Operation::Hop, vec![0; 16]).unwrap();
        let limits = ProtocolLimits {
            max_frame_bytes: HEADER_BYTES + 8,
            ..ProtocolLimits::default()
        };
        assert_eq!(frame.encode(limits), Err(FrameError::FrameTooLarge));
    }

    #[test]
    fn validates_descriptor_limits() {
        let descriptor = Descriptor {
            wire_type: WireType::F32,
            dimensions: vec![4],
            strides: vec![1],
            nbytes: 64,
            view_offset: 0,
            alias_of: None,
            flags: 0,
            name: "hidden".to_owned(),
        };
        descriptor.validate(ProtocolLimits::default()).unwrap();
        let limits = ProtocolLimits {
            max_payload_bytes: 32,
            ..ProtocolLimits::default()
        };
        assert_eq!(
            descriptor.validate(limits),
            Err(FrameError::PayloadTooLarge)
        );
    }

    fn descriptor(name: &str, nbytes: u64, alias_of: Option<u32>) -> Descriptor {
        Descriptor {
            wire_type: WireType::F32,
            dimensions: vec![nbytes],
            strides: vec![1],
            nbytes,
            view_offset: 0,
            alias_of,
            flags: 0,
            name: name.to_owned(),
        }
    }

    #[test]
    fn sequence_payload_round_trips_multiple_descriptors() {
        let payload = SequencePayload {
            sequence_id: "seq-42".into(),
            descriptors: vec![
                descriptor("hidden", 4, None),
                descriptor("residual", 2, None),
            ],
            payloads: vec![Some(vec![1, 2, 3, 4]), Some(vec![5, 6])],
            n_tokens: None,
            prompt: None,
            initial_tokens: None,
            position: None,
            options: String::new(),
            outcome: None,
        };
        let encoded = payload.encode(ProtocolLimits::default()).unwrap();
        assert_eq!(
            SequencePayload::decode(&encoded, ProtocolLimits::default()).unwrap(),
            payload
        );
    }

    #[test]
    fn sequence_payload_v2_round_trips_opaque_options_and_legacy_stays_empty() {
        let payload = SequencePayload {
            sequence_id: "options-seq".into(),
            descriptors: Vec::new(),
            payloads: Vec::new(),
            n_tokens: Some(1),
            prompt: Some("hello".into()),
            initial_tokens: None,
            position: Some(4),
            options: r#"{"temperature":0,"grammar":"root ::= \"A\""}"#.into(),
            outcome: None,
        };
        let hop = HopPayload {
            phase: HopPhase::Prefill,
            sequences: vec![payload.clone()],
            legacy: false,
        };
        let decoded = HopPayload::decode(
            &hop.encode(ProtocolLimits::default()).unwrap(),
            ProtocolLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.sequences[0].options, payload.options);

        let legacy = payload.encode(ProtocolLimits::default()).unwrap();
        assert!(SequencePayload::decode(&legacy, ProtocolLimits::default())
            .unwrap()
            .options
            .is_empty());
    }

    #[test]
    fn alias_descriptor_omits_payload() {
        let payload = SequencePayload {
            sequence_id: "seq-7".into(),
            descriptors: vec![descriptor("base", 4, None), descriptor("view", 4, Some(0))],
            payloads: vec![Some(vec![9, 8, 7, 6]), None],
            n_tokens: None,
            prompt: None,
            initial_tokens: None,
            position: None,
            options: String::new(),
            outcome: None,
        };
        let encoded = payload.encode(ProtocolLimits::default()).unwrap();
        let decoded = SequencePayload::decode(&encoded, ProtocolLimits::default()).unwrap();
        assert_eq!(decoded, payload);
        let non_alias_payload = SequencePayload {
            sequence_id: "seq-7".into(),
            descriptors: vec![descriptor("base", 4, None), descriptor("view", 4, None)],
            payloads: vec![Some(vec![9, 8, 7, 6]), Some(vec![9, 8, 7, 6])],
            n_tokens: None,
            prompt: None,
            initial_tokens: None,
            position: None,
            options: String::new(),
            outcome: None,
        };
        assert!(
            encoded.len()
                < non_alias_payload
                    .encode(ProtocolLimits::default())
                    .unwrap()
                    .len()
        );
    }

    #[test]
    fn hop_payload_uses_hmux_for_multiple_sequences() {
        let payload = HopPayload {
            phase: HopPhase::Prefill,
            sequences: vec![
                SequencePayload {
                    sequence_id: "seq-a".into(),
                    descriptors: Vec::new(),
                    payloads: Vec::new(),
                    n_tokens: None,
                    prompt: None,
                    initial_tokens: None,
                    position: None,
                    options: String::new(),
                    outcome: None,
                },
                SequencePayload {
                    sequence_id: "seq-b".into(),
                    descriptors: Vec::new(),
                    payloads: Vec::new(),
                    n_tokens: None,
                    prompt: None,
                    initial_tokens: None,
                    position: None,
                    options: String::new(),
                    outcome: None,
                },
            ],
            legacy: false,
        };
        let encoded = payload.encode(ProtocolLimits::default()).unwrap();
        assert_eq!(&encoded[..4], b"HMUX");
        assert_eq!(
            HopPayload::decode(&encoded, ProtocolLimits::default()).unwrap(),
            payload
        );
    }

    #[test]
    fn hop_payload_decoder_accepts_legacy_one_sequence_body() {
        let sequence = SequencePayload {
            sequence_id: "legacy".into(),
            descriptors: Vec::new(),
            payloads: Vec::new(),
            n_tokens: None,
            prompt: None,
            initial_tokens: None,
            position: None,
            options: String::new(),
            outcome: None,
        };
        let encoded = sequence.encode(ProtocolLimits::default()).unwrap();
        assert_eq!(
            HopPayload::decode(&encoded, ProtocolLimits::default())
                .unwrap()
                .sequences,
            vec![sequence]
        );
    }

    #[test]
    fn hop_metadata_and_explicit_token_count_round_trip() {
        let payload = SequencePayload {
            sequence_id: "tail".into(),
            descriptors: Vec::new(),
            payloads: Vec::new(),
            n_tokens: Some(1),
            prompt: None,
            initial_tokens: None,
            position: Some(3),
            options: String::new(),
            outcome: Some(OutcomeMetadata {
                token: 42,
                text: "hello".into(),
                position: 4,
                stop: Some("eos".into()),
            }),
        };
        let hop = HopPayload {
            phase: HopPhase::Decode,
            sequences: vec![payload.clone()],
            legacy: false,
        };
        let decoded = HopPayload::decode(
            &hop.encode(ProtocolLimits::default()).unwrap(),
            ProtocolLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.sequences, vec![payload.clone()]);
        let cut = payload.encode(ProtocolLimits::default()).unwrap();
        assert_eq!(
            SequencePayload::decode(&cut, ProtocolLimits::default())
                .unwrap()
                .n_tokens,
            Some(1)
        );
    }

    #[test]
    fn kv_payload_matches_cpp_field_order_and_round_trips() {
        let payload = KvPayload {
            sequence_id: "seq-7".into(),
            cache_key: "request-7".into(),
            model_identity: "S:/models/model.gguf".into(),
            stage_begin: 4,
            stage_end: 12,
            flags: 0,
            expected_checksum: String::new(),
            operation_id: "".into(),
        };
        let bytes = payload.encode(ProtocolLimits::default()).unwrap();
        assert_eq!(&bytes[..4], &5u32.to_le_bytes());
        assert_eq!(
            KvPayload::decode(&bytes, ProtocolLimits::default()).unwrap(),
            payload
        );
        assert_eq!(&bytes[bytes.len() - 5..], b"\x01\x00\x00\x00-");
    }

    #[test]
    fn kv_result_uses_little_endian_u64_and_round_trips() {
        let result = KvResult {
            sequence_id: "seq-7".into(),
            cache_key: "request-7".into(),
            bytes: 0x0102_0304_0506_0708,
            checksum: "a".repeat(64),
        };
        let bytes = result.encode(ProtocolLimits::default()).unwrap();
        let u64_offset = 4 + 5 + 4 + 9;
        assert_eq!(
            &bytes[u64_offset..u64_offset + 8],
            &result.bytes.to_le_bytes()
        );
        assert_eq!(
            KvResult::decode(&bytes, ProtocolLimits::default()).unwrap(),
            result
        );
    }

    #[test]
    fn kv_receipt_round_trips() {
        let receipt = KvReceipt {
            operation_id: "operation-1".into(),
            sequence_id: "seq-7".into(),
            cache_key: "request-7".into(),
            model_identity: "model".into(),
            stage_begin: 4,
            stage_end: 12,
            kind: 1,
            state: KvReceiptState::Prepared,
            bytes: 42,
            checksum: "a".repeat(64),
            detail: "prepared".into(),
        };
        let bytes = receipt.encode(ProtocolLimits::default()).unwrap();
        assert_eq!(KvReceipt::decode(&bytes, ProtocolLimits::default()).unwrap(), receipt);
    }
}
