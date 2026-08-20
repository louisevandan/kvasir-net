#pragma once

// Helpers shared by the staged hop paths.
//
// One sequence at a time and a whole decode lap at once are two shapes of the
// same execution, so descriptor conversion and the sequence-slot table live
// here rather than being written twice and drifting apart.

#include "llama_stage_runtime.hpp"

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <string>
#include <unordered_map>

#include "ggml.h"

namespace staged::llama_runtime::hop {


inline bool fail_hop(const char * message, std::string * error) {
    if (error != nullptr) {
        *error = message;
    }
    return false;
}

inline bool hop_trace_enabled() {
    return std::getenv("P4_STAGED_TRACE_HOP") != nullptr;
}

inline const char * hop_phase_name(protocol::HopPhase phase) {
    return phase == protocol::HopPhase::Prefill ? "prefill" : "decode";
}

inline bool valid_utf8_text(const std::string & value) {
    for (std::size_t i = 0; i < value.size();) {
        const auto byte = static_cast<unsigned char>(value[i]);
        std::size_t width = byte < 0x80 ? 1 : byte < 0xE0 ? 2 : byte < 0xF0 ? 3 : 4;
        if (width == 1) { ++i; continue; }
        if (i + width > value.size() || (byte == 0xE0 && static_cast<unsigned char>(value[i + 1]) < 0xA0) ||
            (byte == 0xED && static_cast<unsigned char>(value[i + 1]) >= 0xA0) ||
            (byte == 0xF0 && static_cast<unsigned char>(value[i + 1]) < 0x90) ||
            (byte == 0xF4 && static_cast<unsigned char>(value[i + 1]) >= 0x90)) return false;
        for (std::size_t j = 1; j < width; ++j) {
            if ((static_cast<unsigned char>(value[i + j]) & 0xC0) != 0x80) return false;
        }
        i += width;
    }
    return true;
}

// How much of `value` is whole UTF-8.
//
// A token boundary is not a character boundary. Korean, Japanese, Chinese and
// emoji are several bytes each and llama.cpp will hand back a piece of one,
// so a streaming caller that emits whatever arrived turns the tail of every
// other token into a replacement character. What is incomplete is not
// corrupt: it is the beginning of something whose remainder is in the next
// token, so it is held rather than emitted, and the next round sends both.
inline std::size_t complete_utf8_prefix(const std::string & value) {
    std::size_t end = value.size();
    // A continuation byte can only be the tail of a sequence that starts
    // within the last three bytes; anything longer is not UTF-8 at all.
    for (std::size_t back = 0; back < 4 && back < end; ++back) {
        const std::size_t at = end - 1 - back;
        const auto byte = static_cast<unsigned char>(value[at]);
        if ((byte & 0xC0) == 0x80) continue;
        const std::size_t width = byte < 0x80 ? 1 : byte < 0xE0 ? 2 : byte < 0xF0 ? 3 : 4;
        return at + width <= end ? end : at;
    }
    return end;
}

inline int32_t ggml_type_from_wire(protocol::WireType type) {
    switch (type) {
    case protocol::WireType::F32: return GGML_TYPE_F32;
    case protocol::WireType::F16: return GGML_TYPE_F16;
    case protocol::WireType::Q8: return GGML_TYPE_Q8_0;
    case protocol::WireType::Q4: return GGML_TYPE_Q4_0;
    case protocol::WireType::Bytes: return -1;
    }
    return -1;
}

inline bool copy_descriptor(const protocol::Descriptor & source,
                     llama_linkcpp_tensor_desc * destination,
                     std::string * error) {
    if (destination == nullptr || source.dimensions.size() > 4 ||
        source.dimensions.size() != source.strides.size()) {
        return fail_hop("invalid staged tensor descriptor", error);
    }
    const auto type = ggml_type_from_wire(source.wire_type);
    if (type < 0) {
        return fail_hop("unsupported staged tensor wire type", error);
    }
    std::memset(destination, 0, sizeof(*destination));
    destination->type = type;
    destination->n_dims = static_cast<int32_t>(source.dimensions.size());
    for (std::size_t i = 0; i < source.dimensions.size(); ++i) {
        destination->ne[i] = static_cast<int64_t>(source.dimensions[i]);
        destination->nb[i] = source.strides[i];
    }
    destination->nbytes = source.nbytes;
    destination->view_offset = source.view_offset;
    destination->alias_of = source.has_alias
        ? static_cast<int32_t>(source.alias_of) : -1;
    destination->flags = source.flags;
    const auto name_size = std::min(source.name.size(), sizeof(destination->name) - 1);
    std::memcpy(destination->name, source.name.data(), name_size);
    destination->name[name_size] = '\0';
    return true;
}

inline protocol::WireType wire_type_from_ggml(int32_t type) {
    switch (type) {
    case GGML_TYPE_F32: return protocol::WireType::F32;
    case GGML_TYPE_F16: return protocol::WireType::F16;
    case GGML_TYPE_Q8_0: return protocol::WireType::Q8;
    case GGML_TYPE_Q4_0: return protocol::WireType::Q4;
    default: return protocol::WireType::Bytes;
    }
}

inline protocol::Descriptor protocol_descriptor(const llama_linkcpp_tensor_desc & source) {
    protocol::Descriptor descriptor;
    descriptor.wire_type = wire_type_from_ggml(source.type);
    const auto dimensions = std::clamp(source.n_dims, 0, 4);
    descriptor.dimensions.reserve(static_cast<std::size_t>(dimensions));
    descriptor.strides.reserve(static_cast<std::size_t>(dimensions));
    for (int32_t i = 0; i < dimensions; ++i) {
        descriptor.dimensions.push_back(static_cast<std::uint64_t>(source.ne[i]));
        descriptor.strides.push_back(source.nb[i]);
    }
    descriptor.nbytes = source.nbytes;
    descriptor.view_offset = source.view_offset;
    if (source.alias_of >= 0) {
        descriptor.has_alias = true;
        descriptor.alias_of = static_cast<std::uint32_t>(source.alias_of);
    }
    descriptor.flags = static_cast<std::uint8_t>(source.flags);
    descriptor.name = source.name;
    return descriptor;
}

inline bool local_sequence(std::unordered_map<std::string, llama_seq_id> & ids,
                    llama_seq_id & next, const std::string & id,
                    uint32_t sequence_limit, llama_seq_id * result,
                    std::string * error) {
    const auto found = ids.find(id);
    if (found != ids.end()) {
        *result = found->second;
        return true;
    }
    if (sequence_limit == 0) {
        return fail_hop("staged HOP sequence table is full", error);
    }
    // Round-robin, deliberately. Handing out the lowest free slot would put a
    // new sequence straight onto the slot a finished one just left, and a lap
    // still in flight for the old one would then meet the new one's cache.
    // Batching does not need dense slots: a unified KV drops llama.cpp's rule
    // that a ubatch only accepts consecutive sequence ids.
    const auto start = next < 0
        ? 0U
        : static_cast<uint32_t>(next) % sequence_limit;
    for (uint32_t offset = 0; offset < sequence_limit; ++offset) {
        const auto candidate = (start + offset) % sequence_limit;
        const auto used = std::any_of(ids.begin(), ids.end(),
            [candidate](const auto & entry) {
                return entry.second == static_cast<llama_seq_id>(candidate);
            });
        if (used) continue;
        const auto value = static_cast<llama_seq_id>(candidate);
        ids.emplace(id, value);
        next = static_cast<llama_seq_id>((candidate + 1U) % sequence_limit);
        *result = value;
        if (std::getenv("P4_STAGED_TRACE_SEQUENCE_RELEASE") != nullptr) {
            std::fprintf(stderr, "P4_STAGED_HOP_ALLOC sequence=%s slot=%lld limit=%u active=%zu\\n",
                         id.c_str(), static_cast<long long>(value), sequence_limit, ids.size());
        }
        return true;
    }
    if (std::getenv("P4_STAGED_TRACE_SEQUENCE_RELEASE") != nullptr) {
        std::fprintf(stderr, "P4_STAGED_HOP_ALLOC_FULL sequence=%s limit=%u active=%zu\\n",
                     id.c_str(), sequence_limit, ids.size());
    }
    return fail_hop("staged HOP sequence table is full", error);
}


} // namespace staged::llama_runtime::hop
