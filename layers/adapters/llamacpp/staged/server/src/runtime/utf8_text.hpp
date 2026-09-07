#pragma once

#include <string>

// Byte validity only: no Unicode normalization and no llama/backend types.
namespace staged::runtime {
inline bool valid_utf8_text(const std::string & value) {
    for (std::size_t i = 0; i < value.size();) {
        const auto byte = static_cast<unsigned char>(value[i]);
        if (byte < 0x80) { ++i; continue; }
        if (byte < 0xC2 || byte > 0xF4) return false;
        const std::size_t width = byte < 0xE0 ? 2 : byte < 0xF0 ? 3 : 4;
        if (i + width > value.size()) return false;
        const auto next = static_cast<unsigned char>(value[i + 1]);
        if ((byte == 0xE0 && next < 0xA0) || (byte == 0xED && next >= 0xA0)
            || (byte == 0xF0 && next < 0x90) || (byte == 0xF4 && next >= 0x90)) {
            return false;
        }
        for (std::size_t j = 1; j < width; ++j) {
            if ((static_cast<unsigned char>(value[i + j]) & 0xC0) != 0x80) return false;
        }
        i += width;
    }
    return true;
}
} // namespace staged::runtime
