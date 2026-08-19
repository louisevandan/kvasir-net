namespace {

constexpr std::array<std::uint8_t, 4> kMagic{'L', 'C', 'P', '4'};
constexpr std::array<std::uint8_t, 4> kTokenCountMagic{'N', 'T', 'O', 'K'};

[[nodiscard]] std::string value_message(const char *kind, std::uint64_t value) {
    return std::string("invalid staged adapter frame: ") + kind + " " +
           std::to_string(value);
}

[[nodiscard]] Operation operation_from_byte(std::uint8_t value) {
    if (value < static_cast<std::uint8_t>(Operation::Hello) ||
        value > static_cast<std::uint8_t>(Operation::KvReceipt)) {
        throw ProtocolError(ErrorCode::UnknownOperation,
                            value_message("unknown operation", value));
    }
    return static_cast<Operation>(value);
}

WireType wire_type_from_byte_impl(std::uint8_t value) {
    switch (value) {
    case static_cast<std::uint8_t>(WireType::F32):
        return WireType::F32;
    case static_cast<std::uint8_t>(WireType::F16):
        return WireType::F16;
    case static_cast<std::uint8_t>(WireType::Q8):
        return WireType::Q8;
    case static_cast<std::uint8_t>(WireType::Q4):
        return WireType::Q4;
    case static_cast<std::uint8_t>(WireType::Bytes):
        return WireType::Bytes;
    default:
        throw ProtocolError(ErrorCode::UnknownWireType,
                            value_message("unknown wire type", value));
    }
}

[[nodiscard]] std::uint16_t read_u16(const std::vector<std::uint8_t> &bytes,
                                     std::size_t offset) {
    return static_cast<std::uint16_t>(bytes[offset]) |
           static_cast<std::uint16_t>(bytes[offset + 1]) << 8U;
}

[[nodiscard]] std::uint32_t read_u32(const std::vector<std::uint8_t> &bytes,
                                     std::size_t offset) {
    return static_cast<std::uint32_t>(bytes[offset]) |
           static_cast<std::uint32_t>(bytes[offset + 1]) << 8U |
           static_cast<std::uint32_t>(bytes[offset + 2]) << 16U |
           static_cast<std::uint32_t>(bytes[offset + 3]) << 24U;
}

[[nodiscard]] std::uint64_t read_u64(const std::vector<std::uint8_t> &bytes,
                                     std::size_t &offset) {
    if (bytes.size() - offset < 8) {
        throw ProtocolError(ErrorCode::Truncated,
                            "invalid staged adapter frame: truncated sequence");
    }
    std::uint64_t value = 0;
    for (unsigned shift = 0; shift < 64; shift += 8) {
        value |= static_cast<std::uint64_t>(bytes[offset++]) << shift;
    }
    return value;
}

void write_u16(std::vector<std::uint8_t> &bytes, std::uint16_t value) {
    bytes.push_back(static_cast<std::uint8_t>(value));
    bytes.push_back(static_cast<std::uint8_t>(value >> 8U));
}

void write_u32(std::vector<std::uint8_t> &bytes, std::uint32_t value) {
    bytes.push_back(static_cast<std::uint8_t>(value));
    bytes.push_back(static_cast<std::uint8_t>(value >> 8U));
    bytes.push_back(static_cast<std::uint8_t>(value >> 16U));
    bytes.push_back(static_cast<std::uint8_t>(value >> 24U));
}

void write_u64(std::vector<std::uint8_t> &bytes, std::uint64_t value) {
    for (unsigned shift = 0; shift < 64; shift += 8) {
        bytes.push_back(static_cast<std::uint8_t>(value >> shift));
    }
}

[[nodiscard]] std::uint32_t read_u32_at(const std::vector<std::uint8_t> &bytes,
                                         std::size_t &offset) {
    if (bytes.size() - offset < 4) {
        throw ProtocolError(ErrorCode::Truncated,
                            "invalid staged adapter frame: truncated sequence");
    }
    const auto value = read_u32(bytes, offset);
    offset += 4;
    return value;
}

[[nodiscard]] bool valid_utf8(const std::string &value) {
    const auto *bytes = reinterpret_cast<const std::uint8_t *>(value.data());
    std::size_t i = 0;
    while (i < value.size()) {
        const auto first = bytes[i++];
        std::size_t continuation = 0;
        std::uint32_t codepoint = 0;
        if (first <= 0x7f) {
            continue;
        } else if (first >= 0xc2 && first <= 0xdf) {
            continuation = 1;
            codepoint = first & 0x1f;
        } else if (first >= 0xe0 && first <= 0xef) {
            continuation = 2;
            codepoint = first & 0x0f;
        } else if (first >= 0xf0 && first <= 0xf4) {
            continuation = 3;
            codepoint = first & 0x07;
        } else {
            return false;
        }
        if (i + continuation > value.size()) {
            return false;
        }
        for (std::size_t n = 0; n < continuation; ++n) {
            const auto next = bytes[i++];
            if ((next & 0xc0) != 0x80) {
                return false;
            }
            codepoint = (codepoint << 6U) | (next & 0x3f);
        }
        if ((continuation == 2 && codepoint < 0x800) ||
            (continuation == 3 && codepoint < 0x10000) ||
            codepoint > 0x10ffff || (codepoint >= 0xd800 && codepoint <= 0xdfff)) {
            return false;
        }
    }
    return true;
}

void validate_alias(std::size_t index, const Descriptor &descriptor,
                    std::size_t count) {
    if (descriptor.has_alias &&
        (descriptor.alias_of >= count || descriptor.alias_of == index)) {
        throw ProtocolError(ErrorCode::InvalidDescriptor,
                            "invalid staged adapter frame: alias target");
    }
}

void write_descriptor(std::vector<std::uint8_t> &output, const Descriptor &descriptor,
                      const ProtocolLimits &limits) {
    descriptor.validate(limits);
    output.push_back(static_cast<std::uint8_t>(descriptor.wire_type));
    output.push_back(static_cast<std::uint8_t>(descriptor.dimensions.size()));
    for (const auto value : descriptor.dimensions) write_u64(output, value);
    for (const auto value : descriptor.strides) write_u64(output, value);
    write_u64(output, descriptor.nbytes);
    write_u64(output, descriptor.view_offset);
    write_u32(output, descriptor.has_alias ? descriptor.alias_of : UINT32_MAX);
    output.push_back(descriptor.flags);
    write_u32(output, static_cast<std::uint32_t>(descriptor.name.size()));
    output.insert(output.end(), descriptor.name.begin(), descriptor.name.end());
}

[[nodiscard]] Descriptor read_descriptor(const std::vector<std::uint8_t> &bytes,
                                         std::size_t &offset,
                                         const ProtocolLimits &limits) {
    if (offset >= bytes.size()) throw ProtocolError(ErrorCode::Truncated, "invalid staged adapter frame: truncated descriptor");
    const auto wire_type = wire_type_from_byte(bytes[offset++]);
    if (offset >= bytes.size()) throw ProtocolError(ErrorCode::Truncated, "invalid staged adapter frame: truncated descriptor");
    const auto rank = bytes[offset++];
    if (rank > 8) throw ProtocolError(ErrorCode::InvalidDescriptor, "invalid staged adapter frame: dimension/stride rank");
    Descriptor descriptor;
    descriptor.wire_type = wire_type;
    for (std::uint8_t i = 0; i < rank; ++i) descriptor.dimensions.push_back(read_u64(bytes, offset));
    for (std::uint8_t i = 0; i < rank; ++i) descriptor.strides.push_back(read_u64(bytes, offset));
    descriptor.nbytes = read_u64(bytes, offset);
    descriptor.view_offset = read_u64(bytes, offset);
    const auto alias = read_u32_at(bytes, offset);
    descriptor.has_alias = alias != UINT32_MAX;
    descriptor.alias_of = alias;
    if (offset >= bytes.size()) throw ProtocolError(ErrorCode::Truncated, "invalid staged adapter frame: truncated descriptor");
    descriptor.flags = bytes[offset++];
    const auto name_size = read_u32_at(bytes, offset);
    if (name_size > limits.max_name_bytes || bytes.size() - offset < name_size) {
        throw ProtocolError(name_size > limits.max_name_bytes ? ErrorCode::NameTooLong : ErrorCode::Truncated,
                            "invalid staged adapter frame: descriptor name");
    }
    descriptor.name.assign(reinterpret_cast<const char *>(bytes.data() + offset), name_size);
    offset += name_size;
    if (!valid_utf8(descriptor.name)) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged adapter frame: descriptor name utf-8");
    descriptor.validate(limits);
    return descriptor;
}

} // namespace

WireType wire_type_from_byte(std::uint8_t value) {
    return wire_type_from_byte_impl(value);
}

ProtocolError::ProtocolError(ErrorCode code, const std::string &message)
    : std::runtime_error(message), code_(code) {}

Frame Frame::make(Operation operation, std::vector<std::uint8_t> body) {
    if (body.size() > std::numeric_limits<std::uint32_t>::max()) {
        throw ProtocolError(ErrorCode::FrameTooLarge,
                            "invalid staged adapter frame: frame too large");
    }
    return Frame{{kProtocolRevision, operation,
                  static_cast<std::uint32_t>(body.size())},
                 std::move(body)};
}

std::vector<std::uint8_t> Frame::encode(const ProtocolLimits &limits) const {
    if (body.size() > std::numeric_limits<std::uint32_t>::max()) {
        throw ProtocolError(ErrorCode::FrameTooLarge,
                            "invalid staged adapter frame: frame too large");
    }
    const auto actual = static_cast<std::uint32_t>(body.size());
    if (actual != header.body_bytes) {
        throw ProtocolError(ErrorCode::LengthMismatch,
                            "invalid staged adapter frame: body length mismatch");
    }
    if (limits.max_frame_bytes < kHeaderBytes ||
        body.size() > limits.max_frame_bytes - kHeaderBytes) {
        throw ProtocolError(ErrorCode::FrameTooLarge,
                            "invalid staged adapter frame: frame too large");
    }

    std::vector<std::uint8_t> output;
    output.reserve(kHeaderBytes + body.size());
    output.insert(output.end(), kMagic.begin(), kMagic.end());
    write_u16(output, header.revision);
    output.push_back(static_cast<std::uint8_t>(header.operation));
    output.push_back(0);
    write_u32(output, header.body_bytes);
    output.insert(output.end(), body.begin(), body.end());
    return output;
}

Frame Frame::decode(const std::vector<std::uint8_t> &bytes,
                    const ProtocolLimits &limits) {
    if (bytes.size() < kHeaderBytes) {
        throw ProtocolError(ErrorCode::Truncated,
                            "invalid staged adapter frame: truncated header");
    }
    if (bytes.size() > limits.max_frame_bytes) {
        throw ProtocolError(ErrorCode::FrameTooLarge,
                            "invalid staged adapter frame: frame too large");
    }
    if (!std::equal(kMagic.begin(), kMagic.end(), bytes.begin())) {
        throw ProtocolError(ErrorCode::BadMagic,
                            "invalid staged adapter frame: bad magic");
    }

    const auto revision = read_u16(bytes, 4);
    if (revision != kProtocolRevision) {
        throw ProtocolError(ErrorCode::UnsupportedRevision,
                            value_message("unsupported revision", revision));
    }
    if (bytes[7] != 0) {
        throw ProtocolError(ErrorCode::ReservedFlags,
                            value_message("reserved flags", bytes[7]));
    }
    const auto operation = operation_from_byte(bytes[6]);
    const auto declared = read_u32(bytes, 8);
    const auto actual = bytes.size() - kHeaderBytes;
    if (declared != actual) {
        throw ProtocolError(ErrorCode::LengthMismatch,
                            "invalid staged adapter frame: body length mismatch");
    }

    return Frame{{revision, operation, declared},
                 std::vector<std::uint8_t>(bytes.begin() + kHeaderBytes,
                                            bytes.end())};
}

void Descriptor::validate(const ProtocolLimits &limits) const {
    if (dimensions.size() != strides.size() || dimensions.size() > 8) {
        throw ProtocolError(ErrorCode::InvalidDescriptor,
                            "invalid staged adapter frame: dimension/stride rank");
    }
    if (name.size() > limits.max_name_bytes) {
        throw ProtocolError(ErrorCode::NameTooLong,
                            "invalid staged adapter frame: name too long");
    }
    if (nbytes > limits.max_payload_bytes) {
        throw ProtocolError(ErrorCode::PayloadTooLarge,
                            "invalid staged adapter frame: payload too large");
    }
}

std::vector<std::uint8_t> SequencePayload::encode(const ProtocolLimits &limits) const {
    if (descriptors.size() != payloads.size()) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged adapter frame: descriptor/payload count");
    if (descriptors.size() > limits.max_descriptors) throw ProtocolError(ErrorCode::TooManyDescriptors, "invalid staged adapter frame: too many descriptors");
    if (n_tokens.has_value() && *n_tokens == 0) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged token count");
    if (!valid_utf8(sequence_id) || sequence_id.size() > limits.max_name_bytes) throw ProtocolError(sequence_id.size() > limits.max_name_bytes ? ErrorCode::NameTooLong : ErrorCode::InvalidSequence, "invalid staged adapter frame: sequence id");
    std::vector<std::uint8_t> output;
    write_u32(output, static_cast<std::uint32_t>(sequence_id.size()));
    output.insert(output.end(), sequence_id.begin(), sequence_id.end());
    write_u32(output, static_cast<std::uint32_t>(descriptors.size()));
    std::size_t payload_total = 0;
    for (std::size_t i = 0; i < descriptors.size(); ++i) {
        const auto &descriptor = descriptors[i];
        validate_alias(i, descriptor, descriptors.size());
        write_descriptor(output, descriptor, limits);
        if (!descriptor.has_alias) {
            if (!payloads[i].has_value()) throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged adapter frame: missing descriptor payload");
            const auto &payload = *payloads[i];
            if (payload.size() != descriptor.nbytes) throw ProtocolError(ErrorCode::PayloadLengthMismatch, "invalid staged adapter frame: payload length mismatch");
            if (payload_total > limits.max_payload_bytes ||
                payload.size() > limits.max_payload_bytes - payload_total) {
                throw ProtocolError(ErrorCode::PayloadTooLarge,
                                    "invalid staged adapter frame: payload too large");
            }
            payload_total += payload.size();
            write_u64(output, payload.size());
            output.insert(output.end(), payload.begin(), payload.end());
        } else if (payloads[i].has_value()) {
            throw ProtocolError(ErrorCode::InvalidSequence, "invalid staged adapter frame: alias payload must be omitted");
        }
    }
    if (n_tokens.has_value()) {
        output.insert(output.end(), kTokenCountMagic.begin(), kTokenCountMagic.end());
        write_u32(output, *n_tokens);
    }
    if (output.size() > limits.max_frame_bytes) throw ProtocolError(ErrorCode::FrameTooLarge, "invalid staged adapter frame: frame too large");
    return output;
