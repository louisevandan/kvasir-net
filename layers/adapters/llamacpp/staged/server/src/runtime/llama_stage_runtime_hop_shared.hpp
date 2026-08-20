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

// The rank a staged descriptor must report, which is not a choice.
//
// A supplied cut-set is matched to a graph tensor on type, rank, byte count,
// and the leading `rank` entries of ne and nb — see the input matcher in the
// compatibility series. The rank it compares against is `ggml_n_dims`, which
// counts dimensions up to the last one greater than one and is never less
// than one. So a one-row cut of a 2,048-wide model is rank 1 whatever axis
// produced it, and a five-row cut is rank 2. Stating it once here keeps the
// merge and the split from each inventing a rule that happens to fit.
inline std::size_t ggml_rank(const std::vector<std::uint64_t> & ne) {
    for (std::size_t axis = ne.size(); axis > 1; --axis) {
        if (ne[axis - 1] > 1) return axis;
    }
    return 1;
}

// A cut-set row and a merged cut-set are both contiguous, and the matcher
// compares nb exactly, so the strides are computed from the shape rather than
// inherited from whichever descriptor this one was derived from.
inline void make_contiguous(protocol::Descriptor & descriptor, std::uint64_t element_bytes) {
    const auto rank = ggml_rank(descriptor.dimensions);
    descriptor.dimensions.resize(rank);
    descriptor.strides.assign(rank, element_bytes);
    std::uint64_t stride = element_bytes;
    for (std::size_t axis = 0; axis < rank; ++axis) {
        descriptor.strides[axis] = stride;
        stride *= descriptor.dimensions[axis];
    }
    descriptor.nbytes = stride;
    descriptor.view_offset = 0;
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
