#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace staged::runtime::detail {

std::string checksum_hex(const std::vector<std::uint8_t> &bytes);

} // namespace staged::runtime::detail
