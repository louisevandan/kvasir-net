}

SequencePayload SequencePayload::decode(const std::vector<std::uint8_t> &bytes,
                                         const ProtocolLimits &limits) {
    if (bytes.size() > limits.max_frame_bytes) throw ProtocolError(ErrorCode::FrameTooLarge, "invalid staged adapter frame: frame too large");
    std::size_t offset = 0;
    const auto sequence_size = read_u32_at(bytes, offset);
    if (sequence_size > limits.max_name_bytes || bytes.size() - offset < sequence_size) throw ProtocolError(sequence_size > limits.max_name_bytes ? ErrorCode::NameTooLong : ErrorCode::Truncated, "invalid staged adapter frame: sequence id");
    SequencePayload result;
    result.sequence_id.assign(reinterpret_cast<const char *>(bytes.data() + offset), sequence_size);
    offset += sequence_size;
    if (!valid_utf8(result.sequence_id)) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged adapter frame: sequence id utf-8");
    const auto count = read_u32_at(bytes, offset);
    if (count > limits.max_descriptors) throw ProtocolError(ErrorCode::TooManyDescriptors, "invalid staged adapter frame: too many descriptors");
    result.descriptors.reserve(count);
    result.payloads.reserve(count);
    std::size_t payload_total = 0;
    for (std::uint32_t i = 0; i < count; ++i) {
        auto descriptor = read_descriptor(bytes, offset, limits);
        validate_alias(i, descriptor, count);
        if (descriptor.has_alias) {
            result.descriptors.push_back(std::move(descriptor));
            result.payloads.emplace_back(std::nullopt);
            continue;
        }
        const auto payload_size = read_u64(bytes, offset);
        if (payload_size > limits.max_payload_bytes || payload_size > bytes.size() - offset) throw ProtocolError(payload_size > limits.max_payload_bytes ? ErrorCode::PayloadTooLarge : ErrorCode::Truncated, "invalid staged adapter frame: payload");
        if (payload_size != descriptor.nbytes) throw ProtocolError(ErrorCode::PayloadLengthMismatch, "invalid staged adapter frame: payload length mismatch");
        if (payload_total > limits.max_payload_bytes - payload_size) throw ProtocolError(ErrorCode::PayloadTooLarge, "invalid staged adapter frame: payload too large");
        payload_total += static_cast<std::size_t>(payload_size);
        const auto payload_size_native = static_cast<std::size_t>(payload_size);
        result.payloads.push_back(std::vector<std::uint8_t>(
            bytes.begin() + offset, bytes.begin() + offset + payload_size_native));
        offset += payload_size_native;
        result.descriptors.push_back(std::move(descriptor));
    }
    if (bytes.size() - offset == kTokenCountMagic.size() + sizeof(std::uint32_t) &&
        std::equal(kTokenCountMagic.begin(), kTokenCountMagic.end(), bytes.begin() + static_cast<std::ptrdiff_t>(offset))) {
        offset += kTokenCountMagic.size();
        result.n_tokens = read_u32_at(bytes, offset);
        if (*result.n_tokens == 0) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged token count");
    }
    if (offset != bytes.size()) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged adapter frame: trailing bytes");
    return result;
}

std::vector<std::uint8_t> SequencePayload::encode_v2(const ProtocolLimits &limits) const {
    if (prompt.has_value() && !valid_utf8(*prompt)) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged HOP prompt utf-8");
    }
    if (prompt.has_value() && prompt->size() > limits.max_frame_bytes) {
        throw ProtocolError(ErrorCode::FrameTooLarge, "staged HOP prompt too large");
    }
    if (initial_tokens.has_value() && initial_tokens->size() > limits.max_descriptors) {
        throw ProtocolError(ErrorCode::TooManyDescriptors, "too many staged HOP initial tokens");
    }
    if (position.has_value() && *position > std::numeric_limits<std::uint32_t>::max()) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged HOP position");
    }
    if (outcome.has_value()) {
        if (!outcome->token.has_value() || !valid_utf8(outcome->text) ||
            outcome->text.size() > limits.max_frame_bytes ||
            (outcome->stop.has_value() &&
             (!valid_utf8(*outcome->stop) || outcome->stop->size() > limits.max_name_bytes))) {
            throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged HOP outcome metadata");
        }
    }
    if (options.size() > limits.max_frame_bytes || !valid_utf8(options)) {
        throw ProtocolError(options.size() > limits.max_frame_bytes
                                ? ErrorCode::FrameTooLarge
                                : ErrorCode::InvalidSequence,
                            "invalid staged HOP options");
    }
    std::vector<std::uint8_t> output;
    const auto flags = static_cast<std::uint8_t>(prompt.has_value() ? 1 : 0) |
                       static_cast<std::uint8_t>(initial_tokens.has_value() ? 2 : 0) |
                       static_cast<std::uint8_t>(outcome.has_value() ? 4 : 0) |
                       static_cast<std::uint8_t>(position.has_value() ? 8 : 0) |
                       static_cast<std::uint8_t>(options.empty() ? 0 : 16);
    output.push_back(flags);
    if (prompt.has_value()) {
        write_u32(output, static_cast<std::uint32_t>(prompt->size()));
        output.insert(output.end(), prompt->begin(), prompt->end());
    }
    if (initial_tokens.has_value()) {
        write_u32(output, static_cast<std::uint32_t>(initial_tokens->size()));
        for (const auto token : *initial_tokens) write_u32(output, static_cast<std::uint32_t>(token));
    }
    if (position.has_value()) write_u32(output, *position);
    if (outcome.has_value()) {
        write_u32(output, static_cast<std::uint32_t>(*outcome->token));
        write_u32(output, outcome->position);
        write_u32(output, static_cast<std::uint32_t>(outcome->text.size()));
        output.insert(output.end(), outcome->text.begin(), outcome->text.end());
        output.push_back(static_cast<std::uint8_t>(outcome->stop.has_value()));
        if (outcome->stop.has_value()) {
            write_u32(output, static_cast<std::uint32_t>(outcome->stop->size()));
            output.insert(output.end(), outcome->stop->begin(), outcome->stop->end());
        }
    }
    if (!options.empty()) {
        write_u32(output, static_cast<std::uint32_t>(options.size()));
        output.insert(output.end(), options.begin(), options.end());
    }
    const auto legacy = encode(limits);
    output.insert(output.end(), legacy.begin(), legacy.end());
    if (output.size() > limits.max_frame_bytes) throw ProtocolError(ErrorCode::FrameTooLarge, "staged HOP sequence too large");
    return output;
}

SequencePayload SequencePayload::decode_v2(const std::vector<std::uint8_t> &bytes,
                                           const ProtocolLimits &limits) {
    if (bytes.empty()) throw ProtocolError(ErrorCode::Truncated, "truncated staged HOP sequence metadata");
    std::size_t offset = 0;
    const auto flags = bytes[offset++];
    if ((flags & ~static_cast<std::uint8_t>(31)) != 0) throw ProtocolError(ErrorCode::InvalidSequence, "reserved staged HOP sequence flags");
    SequencePayload result;
    if (flags & 1) {
        const auto size = read_u32_at(bytes, offset);
        if (size > limits.max_frame_bytes || bytes.size() - offset < size) throw ProtocolError(size > limits.max_frame_bytes ? ErrorCode::FrameTooLarge : ErrorCode::Truncated, "staged HOP prompt");
        result.prompt = std::string(reinterpret_cast<const char *>(bytes.data() + offset), size);
        offset += size;
        if (!valid_utf8(*result.prompt)) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged HOP prompt utf-8");
    }
    if (flags & 2) {
        const auto count = read_u32_at(bytes, offset);
        if (count > limits.max_descriptors || bytes.size() - offset < static_cast<std::size_t>(count) * 4) throw ProtocolError(count > limits.max_descriptors ? ErrorCode::TooManyDescriptors : ErrorCode::Truncated, "staged HOP initial tokens");
        result.initial_tokens.emplace();
        result.initial_tokens->reserve(count);
        for (std::uint32_t i = 0; i < count; ++i) result.initial_tokens->push_back(static_cast<std::int32_t>(read_u32_at(bytes, offset)));
    }
    if (flags & 8) result.position = read_u32_at(bytes, offset);
    if (flags & 4) {
        SequencePayload::OutcomeMetadata metadata;
        metadata.token = static_cast<std::int32_t>(read_u32_at(bytes, offset));
        metadata.position = read_u32_at(bytes, offset);
        const auto text_size = read_u32_at(bytes, offset);
        if (text_size > limits.max_frame_bytes || bytes.size() - offset < text_size) {
            throw ProtocolError(text_size > limits.max_frame_bytes ? ErrorCode::FrameTooLarge : ErrorCode::Truncated, "staged HOP outcome text");
        }
        metadata.text.assign(reinterpret_cast<const char *>(bytes.data() + offset), text_size);
        offset += text_size;
        if (!valid_utf8(metadata.text)) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged HOP outcome text utf-8");
        if (offset >= bytes.size()) throw ProtocolError(ErrorCode::Truncated, "staged HOP outcome stop flag");
        const auto has_stop = bytes[offset++];
        if (has_stop > 1) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged HOP outcome stop flag");
        if (has_stop != 0) {
            const auto stop_size = read_u32_at(bytes, offset);
            if (stop_size > limits.max_name_bytes || bytes.size() - offset < stop_size) {
                throw ProtocolError(stop_size > limits.max_name_bytes ? ErrorCode::NameTooLong : ErrorCode::Truncated, "staged HOP outcome stop");
            }
            metadata.stop = std::string(reinterpret_cast<const char *>(bytes.data() + offset), stop_size);
            offset += stop_size;
            if (!valid_utf8(*metadata.stop)) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged HOP outcome stop utf-8");
        }
        result.outcome = std::move(metadata);
    }
    if (flags & 16) {
        const auto options_size = read_u32_at(bytes, offset);
        if (options_size > limits.max_frame_bytes || bytes.size() - offset < options_size) {
            throw ProtocolError(options_size > limits.max_frame_bytes
                                    ? ErrorCode::FrameTooLarge
                                    : ErrorCode::Truncated,
                                "staged HOP options");
        }
        result.options.assign(reinterpret_cast<const char *>(bytes.data() + offset), options_size);
        offset += options_size;
        if (!valid_utf8(result.options)) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged HOP options utf-8");
    }
    auto body = std::vector<std::uint8_t>(bytes.begin() + static_cast<std::ptrdiff_t>(offset), bytes.end());
    auto decoded = SequencePayload::decode(body, limits);
    decoded.prompt = std::move(result.prompt);
    decoded.initial_tokens = std::move(result.initial_tokens);
    decoded.position = result.position;
    decoded.options = std::move(result.options);
    decoded.outcome = std::move(result.outcome);
    return decoded;
}

std::vector<std::uint8_t> HopPayload::encode(const ProtocolLimits &limits) const {
    if (sequences.empty()) {
        throw ProtocolError(ErrorCode::InvalidSequence,
                            "invalid staged adapter frame: empty HOP sequence list");
    }
    if (sequences.size() > limits.max_descriptors) {
        throw ProtocolError(ErrorCode::TooManyDescriptors,
                            "invalid staged adapter frame: too many HOP sequences");
    }

    std::vector<std::uint8_t> output(kEnvelopeMagic.begin(), kEnvelopeMagic.end());
    output.push_back(kEnvelopeVersion);
    output.push_back(static_cast<std::uint8_t>(phase));
    output.push_back(0);
    output.push_back(0);
    write_u32(output, static_cast<std::uint32_t>(sequences.size()));
    for (const auto &sequence : sequences) {
        const auto encoded = sequence.encode_v2(limits);
        if (encoded.size() > limits.max_frame_bytes ||
            encoded.size() > std::numeric_limits<std::uint32_t>::max()) {
            throw ProtocolError(ErrorCode::FrameTooLarge,
                                "invalid staged adapter frame: HOP sequence too large");
        }
        write_u32(output, static_cast<std::uint32_t>(encoded.size()));
        output.insert(output.end(), encoded.begin(), encoded.end());
        if (output.size() > limits.max_frame_bytes) {
            throw ProtocolError(ErrorCode::FrameTooLarge,
                                "invalid staged adapter frame: HOP envelope too large");
        }
    }
    return output;
}

HopPayload HopPayload::decode(const std::vector<std::uint8_t> &bytes,
                              const ProtocolLimits &limits,
                              bool *enveloped) {
    const bool is_enveloped = bytes.size() >= kEnvelopeMagic.size() &&
        std::equal(kEnvelopeMagic.begin(), kEnvelopeMagic.end(), bytes.begin());
    if (enveloped != nullptr) {
        *enveloped = is_enveloped;
    }
    if (!is_enveloped) {
        return HopPayload{HopPhase::Decode, {SequencePayload::decode(bytes, limits)}, true};
    }

    if (bytes.size() < 8 || bytes[4] != kEnvelopeVersion) {
        std::size_t offset = kEnvelopeMagic.size();
        const auto count = read_u32_at(bytes, offset);
        if (count == 0 || count > limits.max_descriptors) throw ProtocolError(ErrorCode::InvalidSequence, "invalid legacy HOP sequence list");
        HopPayload result;
        result.legacy = true;
        result.sequences.reserve(count);
        for (std::uint32_t i = 0; i < count; ++i) {
            const auto size = read_u32_at(bytes, offset);
            if (size > limits.max_frame_bytes || bytes.size() - offset < size) throw ProtocolError(size > limits.max_frame_bytes ? ErrorCode::FrameTooLarge : ErrorCode::Truncated, "legacy HOP sequence payload");
            result.sequences.push_back(SequencePayload::decode(std::vector<std::uint8_t>(bytes.begin() + offset, bytes.begin() + offset + size), limits));
            offset += size;
        }
        if (offset != bytes.size()) throw ProtocolError(ErrorCode::InvalidSequence, "trailing legacy HOP envelope bytes");
        return result;
    }
    if (bytes[6] != 0 || bytes[7] != 0) throw ProtocolError(ErrorCode::InvalidSequence, "reserved HOP envelope flags");
    const auto phase_byte = bytes[5];
    if (phase_byte > static_cast<std::uint8_t>(HopPhase::Decode)) throw ProtocolError(ErrorCode::InvalidSequence, "unknown HOP phase");
    std::size_t offset = 8;
    const auto count = read_u32_at(bytes, offset);
    if (count == 0) {
        throw ProtocolError(ErrorCode::InvalidSequence,
                            "invalid staged adapter frame: empty HOP sequence list");
    }
    if (count > limits.max_descriptors) {
        throw ProtocolError(ErrorCode::TooManyDescriptors,
                            "invalid staged adapter frame: too many HOP sequences");
    }

    HopPayload result;
    result.sequences.reserve(count);
    for (std::uint32_t i = 0; i < count; ++i) {
        const auto sequence_bytes = read_u32_at(bytes, offset);
        if (sequence_bytes > limits.max_frame_bytes ||
            sequence_bytes > bytes.size() - offset) {
            throw ProtocolError(sequence_bytes > limits.max_frame_bytes
                                    ? ErrorCode::FrameTooLarge
                                    : ErrorCode::Truncated,
                                "invalid staged adapter frame: HOP sequence payload");
        }
        const auto begin = bytes.begin() + static_cast<std::ptrdiff_t>(offset);
        const auto end = begin + static_cast<std::ptrdiff_t>(sequence_bytes);
        result.sequences.push_back(SequencePayload::decode_v2(
            std::vector<std::uint8_t>(begin, end), limits));
        offset += sequence_bytes;
    }
    if (offset != bytes.size()) {
        throw ProtocolError(ErrorCode::InvalidSequence,
                            "invalid staged adapter frame: trailing HOP envelope bytes");
    }
    result.phase = static_cast<HopPhase>(phase_byte);
    result.legacy = false;
    return result;
}
