#include "state_store.hpp"
#include "state_store_checksum.hpp"

#include <array>
#include <atomic>
#include <cstdio>
#include <fstream>
#include <functional>
#include <thread>

#ifdef _WIN32
#include <Windows.h>
#include <io.h>
#include <process.h>
#else
#include <fcntl.h>
#include <unistd.h>
#endif

namespace staged::runtime {
namespace {

constexpr std::array<std::uint8_t, 8> kMagic{'L','C','P','K','V','0','1',0};
constexpr std::uint32_t kVersion = 2;
constexpr std::size_t kDigestBytes = 32;
constexpr std::size_t kFixedHeaderBytes = 100;
std::atomic<std::uint64_t> kTemporarySerial{0};

std::string fail_text(const char *message, std::string *error);

std::uint64_t process_id() {
#ifdef _WIN32
    return static_cast<std::uint64_t>(_getpid());
#else
    return static_cast<std::uint64_t>(::getpid());
#endif
}

std::filesystem::path temporary_path(const std::filesystem::path &path) {
    const auto process = std::hash<std::thread::id>{}(std::this_thread::get_id());
    return path.string() + ".tmp." + std::to_string(process_id()) + "."
        + std::to_string(process) + "."
        + std::to_string(kTemporarySerial.fetch_add(1, std::memory_order_relaxed));
}

bool flush_file(FILE *file, std::string *error) {
    if (std::fflush(file) != 0) {
        fail_text("cannot flush KV temporary file", error);
        return false;
    }
#ifdef _WIN32
    if (_commit(_fileno(file)) != 0) {
        fail_text("cannot commit KV temporary file", error);
        return false;
    }
#else
    if (::fsync(::fileno(file)) != 0) {
        fail_text("cannot sync KV temporary file", error);
        return false;
    }
#endif
    return true;
}

bool publish_file(const std::filesystem::path &temporary,
                  const std::filesystem::path &path, std::string *error) {
#ifdef _WIN32
    for (unsigned attempt = 0; attempt < 32; ++attempt) {
        if (MoveFileExW(temporary.wstring().c_str(), path.wstring().c_str(),
                        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
            return true;
        }
        const auto status = GetLastError();
        if (status != ERROR_ACCESS_DENIED && status != ERROR_SHARING_VIOLATION) {
            break;
        }
        Sleep(1);
    }
    fail_text("cannot atomically publish KV state file", error);
    return false;
#else
    std::error_code ec;
    std::filesystem::rename(temporary, path, ec);
    if (ec) {
        fail_text("cannot atomically publish KV state file", error);
        return false;
    }
    const auto directory = ::open(path.parent_path().c_str(), O_RDONLY | O_DIRECTORY);
    if (directory < 0) {
        fail_text("cannot open KV state directory for sync", error);
        return false;
    }
    const auto synced = ::fsync(directory) == 0;
    ::close(directory);
    if (!synced) {
        fail_text("cannot sync KV state directory", error);
        return false;
    }
    return true;
#endif
}

void put_u32(std::vector<std::uint8_t> &out, std::uint32_t value) {
    for (unsigned shift = 0; shift < 32; shift += 8) {
        out.push_back(static_cast<std::uint8_t>(value >> shift));
    }
}

void put_i32(std::vector<std::uint8_t> &out, std::int32_t value) {
    put_u32(out, static_cast<std::uint32_t>(value));
}

void put_u64(std::vector<std::uint8_t> &out, std::uint64_t value) {
    for (unsigned shift = 0; shift < 64; shift += 8) {
        out.push_back(static_cast<std::uint8_t>(value >> shift));
    }
}

bool take_u32(const std::vector<std::uint8_t> &bytes, std::size_t &offset,
              std::uint32_t *value) {
    if (bytes.size() - offset < 4) return false;
    *value = static_cast<std::uint32_t>(bytes[offset])
        | (static_cast<std::uint32_t>(bytes[offset + 1]) << 8U)
        | (static_cast<std::uint32_t>(bytes[offset + 2]) << 16U)
        | (static_cast<std::uint32_t>(bytes[offset + 3]) << 24U);
    offset += 4;
    return true;
}

bool take_u64(const std::vector<std::uint8_t> &bytes, std::size_t &offset,
              std::uint64_t *value) {
    if (bytes.size() - offset < 8) return false;
    *value = 0;
    for (unsigned shift = 0; shift < 64; shift += 8) {
        *value |= static_cast<std::uint64_t>(bytes[offset++]) << shift;
    }
    return true;
}


bool safe_key(const std::string &key) {
    if (key.empty() || key.size() > 256) return false;
    for (const auto ch : key) {
        if (!((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z')
              || (ch >= '0' && ch <= '9') || ch == '.' || ch == '_' || ch == '-')) {
            return false;
        }
    }
    return true;
}

bool manifest_complete(const protocol::KvPayload &request) {
    return !request.build_identity.empty() && !request.runtime_identity.empty()
        && !request.context_identity.empty() && !request.kv_format.empty();
}

std::string fail_text(const char *message, std::string *error) {
    if (error != nullptr) *error = message;
    return message;
}

std::array<std::uint8_t, kDigestBytes> digest_bytes(const std::string &digest) {
    std::array<std::uint8_t, kDigestBytes> bytes{};
    for (std::size_t index = 0; index < bytes.size(); ++index) {
        bytes[index] = static_cast<std::uint8_t>(
            std::stoul(digest.substr(index * 2, 2), nullptr, 16));
    }
    return bytes;
}

std::vector<std::uint8_t> make_file(const protocol::KvPayload &request,
                                    const std::vector<std::uint8_t> &state,
                                    const std::string &digest) {
    std::vector<std::uint8_t> file;
    file.insert(file.end(), kMagic.begin(), kMagic.end());
    put_u32(file, kVersion);
    put_u32(file, request.flags);
    put_i32(file, request.stage_begin);
    put_i32(file, request.stage_end);
    put_u64(file, state.size());
    put_u32(file, static_cast<std::uint32_t>(request.sequence_id.size()));
    put_u32(file, static_cast<std::uint32_t>(request.model_identity.size()));
    put_u32(file, static_cast<std::uint32_t>(request.cache_key.size()));
    put_u32(file, static_cast<std::uint32_t>(request.build_identity.size()));
    put_u32(file, static_cast<std::uint32_t>(request.runtime_identity.size()));
    put_u32(file, static_cast<std::uint32_t>(request.context_identity.size()));
    put_u32(file, static_cast<std::uint32_t>(request.kv_format.size()));
    put_u64(file, request.token_position);
    for (std::size_t i = 0; i < digest.size(); i += 2) {
        file.push_back(static_cast<std::uint8_t>(std::stoul(digest.substr(i, 2), nullptr, 16)));
    }
    file.insert(file.end(), request.sequence_id.begin(), request.sequence_id.end());
    file.insert(file.end(), request.model_identity.begin(), request.model_identity.end());
    file.insert(file.end(), request.cache_key.begin(), request.cache_key.end());
    file.insert(file.end(), request.build_identity.begin(), request.build_identity.end());
    file.insert(file.end(), request.runtime_identity.begin(), request.runtime_identity.end());
    file.insert(file.end(), request.context_identity.begin(), request.context_identity.end());
    file.insert(file.end(), request.kv_format.begin(), request.kv_format.end());
    file.insert(file.end(), state.begin(), state.end());
    return file;
}

} // namespace

StateStore::StateStore(std::filesystem::path root)
    : root_(std::move(root)) {}

std::filesystem::path StateStore::path_for(const std::string &cache_key,
                                            std::string *error) const {
    if (root_.empty()) {
        if (error != nullptr) *error = "KV store root is not configured";
        return {};
    }
    if (!safe_key(cache_key)) {
        if (error != nullptr) *error = "KV cache key is not a safe file name";
        return {};
    }
    return root_ / (cache_key + ".lkv");
}

bool StateStore::save(const protocol::KvPayload &request,
                      const std::vector<std::uint8_t> &state,
                      StateFileInfo *info, std::string *error) const {
    const auto path = path_for(request.cache_key, error);
    if (path.empty()) return false;
    if (!manifest_complete(request)) {
        fail_text("KV state manifest is incomplete", error);
        return false;
    }
    std::error_code ec;
    std::filesystem::create_directories(root_, ec);
    if (ec) { fail_text("cannot create KV store directory", error); return false; }
    const auto digest = checksum(state);
    if (!request.expected_checksum.empty() && request.expected_checksum != digest) {
        fail_text("KV state checksum does not match request", error);
        return false;
    }
    const auto file = make_file(request, state, digest);
    const auto temporary = temporary_path(path);
    FILE *out = std::fopen(temporary.string().c_str(), "wb");
    if (out == nullptr) {
        fail_text("cannot open KV temporary file", error);
        return false;
    }
    const auto written = file.empty() ? 0U : std::fwrite(file.data(), 1, file.size(), out);
    const auto valid = written == file.size() && flush_file(out, error);
    const auto closed = std::fclose(out) == 0;
    if (!valid || !closed) {
        std::filesystem::remove(temporary, ec);
        if (valid && !closed) fail_text("cannot close KV temporary file", error);
        return false;
    }
    if (!publish_file(temporary, path, error)) {
        std::filesystem::remove(temporary, ec);
        return false;
    }
    if (info != nullptr) {
        info->bytes = static_cast<std::uint64_t>(state.size());
        info->checksum = digest;
        info->build_identity = request.build_identity;
        info->runtime_identity = request.runtime_identity;
        info->context_identity = request.context_identity;
        info->kv_format = request.kv_format;
        info->token_position = request.token_position;
    }
    return true;
}

bool StateStore::load(const protocol::KvPayload &request,
                      std::vector<std::uint8_t> *state,
                      StateFileInfo *info, std::string *error) const {
    if (state == nullptr) { fail_text("KV state output is null", error); return false; }
    const auto path = path_for(request.cache_key, error);
    if (path.empty()) return false;
    if (!manifest_complete(request)) {
        fail_text("KV state manifest is incomplete", error);
        return false;
    }
    std::ifstream in(path, std::ios::binary | std::ios::ate);
    if (!in) { fail_text("KV state file is not found", error); return false; }
    const auto size = in.tellg();
    if (size < 0 || static_cast<std::uint64_t>(size) > 128ULL * 1024ULL * 1024ULL) {
        fail_text("KV state file is too large", error); return false;
    }
    std::vector<std::uint8_t> file(static_cast<std::size_t>(size));
    in.seekg(0); in.read(reinterpret_cast<char *>(file.data()), static_cast<std::streamsize>(file.size()));
    if (!in && !file.empty()) { fail_text("cannot read KV state file", error); return false; }
    if (file.size() < kFixedHeaderBytes || !std::equal(kMagic.begin(), kMagic.end(), file.begin())) {
        fail_text("KV state file header is invalid", error); return false;
    }
    std::size_t offset = 8; std::uint32_t version=0, flags=0, seq_len=0, model_len=0, key_len=0;
    std::uint32_t build_len=0, runtime_len=0, context_len=0, format_len=0;
    std::uint64_t state_len=0, token_position=0;
    std::uint32_t begin=0, end=0;
    if (!take_u32(file, offset, &version) || !take_u32(file, offset, &flags)
        || !take_u32(file, offset, &begin) || !take_u32(file, offset, &end)
        || !take_u64(file, offset, &state_len) || !take_u32(file, offset, &seq_len)
        || !take_u32(file, offset, &model_len) || !take_u32(file, offset, &key_len)
        || !take_u32(file, offset, &build_len) || !take_u32(file, offset, &runtime_len)
        || !take_u32(file, offset, &context_len) || !take_u32(file, offset, &format_len)
        || !take_u64(file, offset, &token_position)
        || file.size() - offset < kDigestBytes) {
        fail_text("KV state header is truncated", error); return false;
    }
    const auto *stored_digest = file.data() + offset; offset += kDigestBytes;
    if (version != kVersion || flags != request.flags
        || static_cast<std::int32_t>(begin) != request.stage_begin
        || static_cast<std::int32_t>(end) != request.stage_end
        || file.size() - offset < static_cast<std::size_t>(seq_len) + model_len + key_len
            + build_len + runtime_len + context_len + format_len
        || state_len > file.size()) {
        fail_text("KV state metadata does not match request", error); return false;
    }
    const std::string sequence(reinterpret_cast<const char *>(file.data()+offset), seq_len); offset += seq_len;
    const std::string model(reinterpret_cast<const char *>(file.data()+offset), model_len); offset += model_len;
    const std::string key(reinterpret_cast<const char *>(file.data()+offset), key_len); offset += key_len;
    const std::string build(reinterpret_cast<const char *>(file.data()+offset), build_len); offset += build_len;
    const std::string runtime(reinterpret_cast<const char *>(file.data()+offset), runtime_len); offset += runtime_len;
    const std::string context(reinterpret_cast<const char *>(file.data()+offset), context_len); offset += context_len;
    const std::string format(reinterpret_cast<const char *>(file.data()+offset), format_len); offset += format_len;
    if (sequence != request.sequence_id || model != request.model_identity || key != request.cache_key
        || build != request.build_identity || runtime != request.runtime_identity
        || context != request.context_identity || format != request.kv_format
        || (request.token_position != UINT64_MAX
            && token_position != request.token_position)
        || state_len != file.size() - offset) {
        fail_text("KV state manifest does not match request", error); return false;
    }
    state->assign(file.begin() + static_cast<std::ptrdiff_t>(offset), file.end());
    const auto digest = checksum(*state);
    const auto standard_digest = digest_bytes(digest);
    auto accepted_checksum = digest;
    if (!std::equal(standard_digest.begin(), standard_digest.end(), stored_digest)) {
        // Version 2 records did not name their digest algorithm. Accept only
        // the exact digest produced by the pre-fix implementation, never an
        // arbitrary mismatch, so legacy state remains recoverable without
        // weakening corruption detection. A later explicit KV_SAVE rewrites
        // this state atomically using the standard digest above.
        const auto legacy = detail::legacy_checksum_hex(*state);
        const auto legacy_digest = digest_bytes(legacy);
        if (!std::equal(legacy_digest.begin(), legacy_digest.end(), stored_digest)) {
            fail_text("KV state checksum is invalid", error); return false;
        }
        accepted_checksum = legacy;
    }
    if (!request.expected_checksum.empty() && request.expected_checksum != accepted_checksum) { fail_text("KV state checksum does not match request", error); return false; }
    if (info != nullptr) {
        info->bytes = state_len;
        info->checksum = accepted_checksum;
        info->build_identity = build;
        info->runtime_identity = runtime;
        info->context_identity = context;
        info->kv_format = format;
        info->token_position = token_position;
    }
    return true;
}

bool StateStore::drop(const protocol::KvPayload &request, StateFileInfo *info,
                      std::string *error) const {
    std::vector<std::uint8_t> ignored;
    StateFileInfo found;
    if (!load(request, &ignored, &found, error)) return false;
    const auto path = path_for(request.cache_key, error);
    if (path.empty()) return false;
    std::error_code ec;
    std::filesystem::remove(path, ec);
    if (ec) { fail_text("cannot remove KV state file", error); return false; }
    if (info != nullptr) *info = {0, found.checksum};
    return true;
}

std::string StateStore::checksum(const std::vector<std::uint8_t> &bytes) {
    return detail::checksum_hex(bytes);
}

} // namespace staged::runtime
