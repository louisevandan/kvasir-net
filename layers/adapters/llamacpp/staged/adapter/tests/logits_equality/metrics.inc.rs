fn compare_logits(full: &SequencePayload, staged: &SequencePayload) -> Comparison {
    fn logits(sequence: &SequencePayload) -> (&p4_llamacpp_staged_adapter::Descriptor, &[u8]) {
        let index = sequence
            .descriptors
            .iter()
            .position(|descriptor| {
                descriptor.wire_type == WireType::F32
                    && (descriptor.name.to_ascii_lowercase().contains("logits")
                        // Current llama.cpp names the terminal vocabulary row
                        // `result_output`; older revisions called it logits.
                        || descriptor.name == "result_output")
            })
            .unwrap_or_else(|| {
                panic!(
                    "NO_LOGITS_OUTPUT: no logits-shaped output descriptor was returned; outputs={:?}",
                    sequence
                        .descriptors
                        .iter()
                        .map(|descriptor| (
                            descriptor.name.as_str(),
                            descriptor.wire_type,
                            descriptor.dimensions.as_slice(),
                            descriptor.nbytes
                        ))
                        .collect::<Vec<_>>()
                )
            });
        let payload = sequence.payloads[index]
            .as_deref()
            .expect("logits descriptor must carry a payload");
        (&sequence.descriptors[index], payload)
    }

    let (full_descriptor, full_bytes) = logits(full);
    let (staged_descriptor, staged_bytes) = logits(staged);
    assert_eq!(full_descriptor.wire_type, staged_descriptor.wire_type);
    assert_eq!(full_descriptor.dimensions, staged_descriptor.dimensions);
    assert_eq!(full_bytes.len(), staged_bytes.len(), "logits payload length");

    let mut comparison = Comparison {
        values: 0,
        max_abs: 0.0,
        max_relative: 0.0,
    };
    for (full_bytes, staged_bytes) in full_bytes.chunks_exact(4).zip(staged_bytes.chunks_exact(4)) {
        let full_value = f32::from_le_bytes(full_bytes.try_into().expect("F32 logits"));
        let staged_value = f32::from_le_bytes(staged_bytes.try_into().expect("F32 logits"));
        assert!(full_value.is_finite() && staged_value.is_finite(), "non-finite logits");
        let absolute = (full_value - staged_value).abs();
        let relative = absolute
            / full_value.abs().max(staged_value.abs()).max(f32::MIN_POSITIVE);
        comparison.values += 1;
        comparison.max_abs = comparison.max_abs.max(absolute);
        comparison.max_relative = comparison.max_relative.max(relative);
    }
    assert!(full_bytes.len() % 4 == 0, "F32 logits payload alignment");
    comparison
}
