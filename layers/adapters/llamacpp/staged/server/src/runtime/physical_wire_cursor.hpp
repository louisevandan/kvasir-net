#pragma once

#include <cstdint>
#include <cstring>
#include <limits>
#include <string>
#include <vector>

namespace staged::llama_runtime::physical_wire {

inline constexpr std::size_t kMaxRows = 65'536;
inline constexpr std::size_t kMaxTensors = 16'384;
inline constexpr std::size_t kMaxString = 4'096;

inline bool fail(const char * message, std::string * error) {
    if (error != nullptr) *error = message;
    return false;
}

class Cursor final {
public:
    explicit Cursor(const std::vector<std::uint8_t> & bytes) : bytes_(bytes) {}

    bool take(std::size_t count, const std::uint8_t ** result) {
        if (result == nullptr || count > bytes_.size() - offset_) return false;
        *result = bytes_.data() + offset_;
        offset_ += count;
        return true;
    }
    bool byte(std::uint8_t * value) {
        const std::uint8_t * data = nullptr;
        if (!take(1, &data)) return false;
        *value = data[0];
        return true;
    }
    bool u16(std::uint16_t * value) {
        const std::uint8_t * data = nullptr;
        if (!take(2, &data)) return false;
        *value = static_cast<std::uint16_t>(data[0])
            | static_cast<std::uint16_t>(data[1]) << 8U;
        return true;
    }
    bool u32(std::uint32_t * value) {
        const std::uint8_t * data = nullptr;
        if (!take(4, &data)) return false;
        *value = static_cast<std::uint32_t>(data[0])
            | static_cast<std::uint32_t>(data[1]) << 8U
            | static_cast<std::uint32_t>(data[2]) << 16U
            | static_cast<std::uint32_t>(data[3]) << 24U;
        return true;
    }
    bool i32(std::int32_t * value) {
        std::uint32_t raw = 0;
        if (!u32(&raw)) return false;
        std::memcpy(value, &raw, sizeof(raw));
        return true;
    }
    bool u64(std::uint64_t * value) {
        const std::uint8_t * data = nullptr;
        if (!take(8, &data)) return false;
        *value = 0;
        for (std::uint32_t index = 0; index < 8; ++index) {
            *value |= static_cast<std::uint64_t>(data[index]) << (index * 8U);
        }
        return true;
    }
    bool i64(std::int64_t * value) {
        std::uint64_t raw = 0;
        if (!u64(&raw)) return false;
        std::memcpy(value, &raw, sizeof(raw));
        return true;
    }
    bool string(std::string * value) {
        std::uint16_t size = 0;
        const std::uint8_t * data = nullptr;
        if (!u16(&size) || size > kMaxString || !take(size, &data)) return false;
        value->assign(reinterpret_cast<const char *>(data), size);
        return true;
    }
    [[nodiscard]] bool done() const { return offset_ == bytes_.size(); }

private:
    const std::vector<std::uint8_t> & bytes_;
    std::size_t offset_ = 0;
};

inline void put_u16(std::vector<std::uint8_t> * out, std::uint16_t value) {
    out->push_back(static_cast<std::uint8_t>(value));
    out->push_back(static_cast<std::uint8_t>(value >> 8U));
}
inline void put_u32(std::vector<std::uint8_t> * out, std::uint32_t value) {
    for (std::uint32_t index = 0; index < 4; ++index) {
        out->push_back(static_cast<std::uint8_t>(value >> (index * 8U)));
    }
}
inline void put_i32(std::vector<std::uint8_t> * out, std::int32_t value) {
    std::uint32_t raw = 0;
    std::memcpy(&raw, &value, sizeof(raw));
    put_u32(out, raw);
}
inline void put_u64(std::vector<std::uint8_t> * out, std::uint64_t value) {
    for (std::uint32_t index = 0; index < 8; ++index) {
        out->push_back(static_cast<std::uint8_t>(value >> (index * 8U)));
    }
}
inline void put_i64(std::vector<std::uint8_t> * out, std::int64_t value) {
    std::uint64_t raw = 0;
    std::memcpy(&raw, &value, sizeof(raw));
    put_u64(out, raw);
}
inline bool put_string(std::vector<std::uint8_t> * out, const std::string & value) {
    if (value.size() > kMaxString || value.size() > std::numeric_limits<std::uint16_t>::max()) {
        return false;
    }
    put_u16(out, static_cast<std::uint16_t>(value.size()));
    out->insert(out->end(), value.begin(), value.end());
    return true;
}

} // namespace staged::llama_runtime::physical_wire
