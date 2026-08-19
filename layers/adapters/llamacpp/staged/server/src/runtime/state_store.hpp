#pragma once

#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

#include "protocol.hpp"

namespace staged::runtime {

struct StateFileInfo {
    std::uint64_t bytes = 0;
    std::string checksum;
    std::string build_identity;
    std::string runtime_identity;
    std::string context_identity;
    std::string kv_format;
    std::uint64_t token_position = 0;
};

// Stores llama state blobs as opaque bytes. The store owns the file format and
// validates all metadata before a blob becomes visible to the runtime.
class StateStore final {
public:
    explicit StateStore(std::filesystem::path root);

    [[nodiscard]] bool available() const noexcept { return !root_.empty(); }
    [[nodiscard]] std::filesystem::path path_for(const std::string &cache_key,
                                                   std::string *error = nullptr) const;

    [[nodiscard]] bool save(const protocol::KvPayload &request,
                            const std::vector<std::uint8_t> &state,
                            StateFileInfo *info = nullptr,
                            std::string *error = nullptr) const;
    [[nodiscard]] bool load(const protocol::KvPayload &request,
                            std::vector<std::uint8_t> *state,
                            StateFileInfo *info = nullptr,
                            std::string *error = nullptr) const;
    [[nodiscard]] bool drop(const protocol::KvPayload &request,
                            StateFileInfo *info = nullptr,
                            std::string *error = nullptr) const;

    [[nodiscard]] static std::string checksum(
        const std::vector<std::uint8_t> &bytes);

private:
    std::filesystem::path root_;
};

} // namespace staged::runtime
