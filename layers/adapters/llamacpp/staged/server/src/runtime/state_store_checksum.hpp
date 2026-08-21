#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace staged::runtime::detail {

std::string checksum_hex(const std::vector<std::uint8_t> &bytes);

// Retained only to verify v2 records written before the standard SHA-256
// correction. New records must always use checksum_hex().
std::string legacy_checksum_hex(const std::vector<std::uint8_t> &bytes);

} // namespace staged::runtime::detail
