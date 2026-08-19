namespace {

void validate_text_field(const std::string &value, const ProtocolLimits &limits,
                         const char *field, std::size_t maximum = 0) {
    const auto limit = maximum == 0 ? limits.max_name_bytes : maximum;
    if (value.empty() || value.size() > limit || !valid_utf8(value)) {
        throw ProtocolError(value.size() > limit ? ErrorCode::NameTooLong
                                                  : ErrorCode::InvalidSequence,
                            std::string("invalid KV ") + field);
    }
}

void validate_cache_key(const std::string &value, const ProtocolLimits &limits) {
    validate_text_field(value, limits, "cache key", 256);
    for (const auto ch : value) {
        const auto safe = (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z')
            || (ch >= '0' && ch <= '9') || ch == '.' || ch == '_' || ch == '-';
        if (!safe) {
            throw ProtocolError(ErrorCode::InvalidSequence,
                                "invalid KV cache key");
        }
    }
}

void write_string(std::vector<std::uint8_t> &output, const std::string &value,
                  const ProtocolLimits &limits, const char *field,
                  std::size_t maximum = 0) {
    validate_text_field(value, limits, field, maximum);
    write_u32(output, static_cast<std::uint32_t>(value.size()));
    output.insert(output.end(), value.begin(), value.end());
}

std::string read_string(const std::vector<std::uint8_t> &bytes, std::size_t &offset,
                        const ProtocolLimits &limits, const char *field,
                        std::size_t maximum = 0) {
    const auto length = read_u32_at(bytes, offset);
    const auto limit = maximum == 0 ? limits.max_name_bytes : maximum;
    if (length == 0 || length > limit || length > bytes.size() - offset) {
        throw ProtocolError(length > limit ? ErrorCode::NameTooLong
                                            : ErrorCode::Truncated,
                            std::string("invalid KV ") + field);
    }
    std::string result(reinterpret_cast<const char *>(bytes.data() + offset), length);
    offset += length;
    if (!valid_utf8(result)) {
        throw ProtocolError(ErrorCode::InvalidSequence,
                            std::string("invalid KV ") + field + " utf-8");
    }
    return result;
}

void ensure_kv_body_size(const std::vector<std::uint8_t> &bytes,
                         const ProtocolLimits &limits) {
    if (bytes.size() > limits.max_frame_bytes) {
        throw ProtocolError(ErrorCode::FrameTooLarge, "KV payload too large");
    }
}

} // namespace

std::vector<std::uint8_t> KvPayload::encode(const ProtocolLimits &limits) const {
    validate_text_field(sequence_id, limits, "sequence id");
    validate_cache_key(cache_key, limits);
    validate_text_field(model_identity, limits, "model identity", 4096);
    if (stage_begin < 0 || stage_end <= stage_begin || flags > kKvDiscard) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid KV stage metadata");
    }
    if (!expected_checksum.empty() &&
        (expected_checksum.size() != 64 || !valid_utf8(expected_checksum))) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid KV checksum");
    }
    if (!operation_id.empty()) {
        validate_text_field(operation_id, limits, "operation id");
    }

    std::vector<std::uint8_t> output;
    write_string(output, sequence_id, limits, "sequence id");
    write_string(output, cache_key, limits, "cache key", 256);
    write_string(output, model_identity, limits, "model identity", 4096);
    write_u32(output, static_cast<std::uint32_t>(stage_begin));
    write_u32(output, static_cast<std::uint32_t>(stage_end));
    write_u32(output, flags);
    write_string(output, expected_checksum.empty() ? std::string("-") : expected_checksum,
                 limits, "checksum", 64);
    if (!operation_id.empty()) {
        write_string(output, operation_id, limits, "operation id");
    }
    if (output.size() > limits.max_frame_bytes) {
        throw ProtocolError(ErrorCode::FrameTooLarge, "KV payload too large");
    }
    return output;
}

KvPayload KvPayload::decode(const std::vector<std::uint8_t> &bytes,
                            const ProtocolLimits &limits) {
    ensure_kv_body_size(bytes, limits);
    std::size_t offset = 0;
    KvPayload result;
    result.sequence_id = read_string(bytes, offset, limits, "sequence id");
    result.cache_key = read_string(bytes, offset, limits, "cache key", 256);
    result.model_identity = read_string(bytes, offset, limits, "model identity", 4096);
    if (bytes.size() - offset < 12) {
        throw ProtocolError(ErrorCode::Truncated, "invalid KV stage metadata");
    }
    result.stage_begin = static_cast<std::int32_t>(read_u32_at(bytes, offset));
    result.stage_end = static_cast<std::int32_t>(read_u32_at(bytes, offset));
    result.flags = read_u32_at(bytes, offset);
    const auto checksum = read_string(bytes, offset, limits, "checksum", 64);
    result.expected_checksum = checksum == "-" ? std::string{} : checksum;
    if (offset < bytes.size()) {
        result.operation_id = read_string(bytes, offset, limits, "operation id");
    }
    if (result.stage_begin < 0 || result.stage_end <= result.stage_begin || result.flags > kKvDiscard
        || (!result.expected_checksum.empty() && result.expected_checksum.size() != 64)
        || offset != bytes.size()) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid KV metadata");
    }
    validate_cache_key(result.cache_key, limits);
    return result;
}

std::vector<std::uint8_t> KvResult::encode(const ProtocolLimits &limits) const {
    validate_text_field(sequence_id, limits, "sequence id");
    validate_cache_key(cache_key, limits);
    if (checksum.size() != 64 || !valid_utf8(checksum)) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid KV result checksum");
    }
    std::vector<std::uint8_t> output;
    write_string(output, sequence_id, limits, "sequence id");
    write_string(output, cache_key, limits, "cache key", 256);
    write_u64(output, bytes);
    write_string(output, checksum, limits, "checksum", 64);
    return output;
}

KvResult KvResult::decode(const std::vector<std::uint8_t> &bytes,
                          const ProtocolLimits &limits) {
    ensure_kv_body_size(bytes, limits);
    std::size_t offset = 0;
    KvResult result;
    result.sequence_id = read_string(bytes, offset, limits, "sequence id");
    result.cache_key = read_string(bytes, offset, limits, "cache key", 256);
    result.bytes = read_u64(bytes, offset);
    result.checksum = read_string(bytes, offset, limits, "checksum", 64);
    if (result.checksum.size() != 64 || offset != bytes.size()) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid KV result");
    }
    validate_cache_key(result.cache_key, limits);
    return result;
}

std::vector<std::uint8_t> KvReceipt::encode(const ProtocolLimits &limits) const {
    validate_text_field(operation_id, limits, "operation id");
    validate_text_field(sequence_id, limits, "sequence id");
    validate_cache_key(cache_key, limits);
    validate_text_field(model_identity, limits, "model identity", 4096);
    const auto kind_valid = kind >= kKvPersist && kind <= kKvDiscard;
    const auto absent_kind = kind == kKvDirect
        && (state == KvReceiptState::Absent || state == KvReceiptState::Inconsistent);
    if (stage_begin < 0 || stage_end <= stage_begin || (!kind_valid && !absent_kind)
        || static_cast<std::uint8_t>(state) > static_cast<std::uint8_t>(KvReceiptState::Committing)) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid KV receipt metadata");
    }
    if (checksum.size() != 64 || !valid_utf8(checksum)) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid KV receipt checksum");
    }
    validate_text_field(detail, limits, "receipt detail", 4096);
    std::vector<std::uint8_t> output;
    write_string(output, operation_id, limits, "operation id");
    write_string(output, sequence_id, limits, "sequence id");
    write_string(output, cache_key, limits, "cache key", 256);
    write_string(output, model_identity, limits, "model identity", 4096);
    write_u32(output, static_cast<std::uint32_t>(stage_begin));
    write_u32(output, static_cast<std::uint32_t>(stage_end));
    write_u32(output, kind);
    output.push_back(static_cast<std::uint8_t>(state));
    write_u64(output, bytes);
    write_string(output, checksum, limits, "receipt checksum", 64);
    write_string(output, detail, limits, "receipt detail", 4096);
    return output;
}

KvReceipt KvReceipt::decode(const std::vector<std::uint8_t> &bytes,
                            const ProtocolLimits &limits) {
    ensure_kv_body_size(bytes, limits);
    std::size_t offset = 0;
    KvReceipt result;
    result.operation_id = read_string(bytes, offset, limits, "operation id");
    result.sequence_id = read_string(bytes, offset, limits, "sequence id");
    result.cache_key = read_string(bytes, offset, limits, "cache key", 256);
    result.model_identity = read_string(bytes, offset, limits, "model identity", 4096);
    result.stage_begin = static_cast<std::int32_t>(read_u32_at(bytes, offset));
    result.stage_end = static_cast<std::int32_t>(read_u32_at(bytes, offset));
    result.kind = read_u32_at(bytes, offset);
    if (offset >= bytes.size()) throw ProtocolError(ErrorCode::Truncated, "invalid KV receipt state");
    result.state = static_cast<KvReceiptState>(bytes[offset++]);
    result.bytes = read_u64(bytes, offset);
    result.checksum = read_string(bytes, offset, limits, "receipt checksum", 64);
    result.detail = read_string(bytes, offset, limits, "receipt detail", 4096);
    const auto kind_valid = result.kind >= kKvPersist && result.kind <= kKvDiscard;
    const auto absent_kind = result.kind == kKvDirect
        && (result.state == KvReceiptState::Absent || result.state == KvReceiptState::Inconsistent);
    if (result.stage_begin < 0 || result.stage_end <= result.stage_begin
        || (!kind_valid && !absent_kind)
        || static_cast<std::uint8_t>(result.state) > static_cast<std::uint8_t>(KvReceiptState::Committing)
        || result.checksum.size() != 64 || offset != bytes.size()) {
        throw ProtocolError(ErrorCode::InvalidSequence, "invalid KV receipt");
    }
    validate_cache_key(result.cache_key, limits);
    return result;
}
