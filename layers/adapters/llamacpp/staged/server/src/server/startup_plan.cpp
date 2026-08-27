#include "startup_plan.hpp"

#include <iostream>

namespace staged::server {

bool read_stdin_exact(std::uint8_t * destination, std::size_t size) {
    std::size_t offset = 0;
    while (offset < size && std::cin.good()) {
        std::cin.read(reinterpret_cast<char *>(destination + offset),
                      static_cast<std::streamsize>(size - offset));
        const auto count = static_cast<std::size_t>(std::cin.gcount());
        offset += count;
        if (count == 0) break;
    }
    return offset == size;
}

std::uint32_t read_u32_le(const std::uint8_t * bytes) {
    return static_cast<std::uint32_t>(bytes[0]) |
           (static_cast<std::uint32_t>(bytes[1]) << 8U) |
           (static_cast<std::uint32_t>(bytes[2]) << 16U) |
           (static_cast<std::uint32_t>(bytes[3]) << 24U);
}

} // namespace staged::server
