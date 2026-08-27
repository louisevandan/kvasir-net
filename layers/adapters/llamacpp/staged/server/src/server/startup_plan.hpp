#pragma once

#include <cstddef>
#include <cstdint>

namespace staged::server {

bool read_stdin_exact(std::uint8_t * destination, std::size_t size);
std::uint32_t read_u32_le(const std::uint8_t * bytes);

} // namespace staged::server
