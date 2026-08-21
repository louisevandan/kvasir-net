#include "state_store.hpp"
#include "state_store_checksum.hpp"

#include <cassert>
#include <chrono>
#include <atomic>
#include <fstream>
#include <mutex>
#include <thread>
#include <vector>

using staged::protocol::KvPayload;
using staged::runtime::StateStore;

namespace {

constexpr std::size_t kDigestOffset = 68;
constexpr std::size_t kDigestBytes = 32;

KvPayload request() {
    KvPayload value;
    value.sequence_id = "sequence-1";
    value.cache_key = "deployment-1";
    value.model_identity = "model-fingerprint";
    value.stage_begin = 2;
    value.stage_end = 8;
    value.build_identity = "llama-system-test";
    value.runtime_identity = "staged-llama-kv-manifest-v1";
    value.context_identity = "n_ctx=512;n_ctx_seq=512;n_batch=64;n_ubatch=64;n_seq_max=1;kv_unified=0";
    value.kv_format = "K=1;V=1;flags=0";
    value.token_position = 7;
    return value;
}

std::vector<std::uint8_t> read_file(const std::filesystem::path &path) {
    std::ifstream input(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>()};
}

void write_file(const std::filesystem::path &path, const std::vector<std::uint8_t> &bytes) {
    std::ofstream output(path, std::ios::binary | std::ios::trunc);
    output.write(reinterpret_cast<const char *>(bytes.data()),
                 static_cast<std::streamsize>(bytes.size()));
}

std::string persisted_digest(const std::vector<std::uint8_t> &file) {
    static constexpr char hex[] = "0123456789abcdef";
    assert(file.size() >= kDigestOffset + kDigestBytes);
    std::string digest;
    digest.reserve(kDigestBytes * 2);
    for (std::size_t index = 0; index < kDigestBytes; ++index) {
        const auto byte = file[kDigestOffset + index];
        digest.push_back(hex[byte >> 4U]);
        digest.push_back(hex[byte & 0x0fU]);
    }
    return digest;
}

void replace_digest(std::vector<std::uint8_t> *file, const std::string &digest) {
    assert(file->size() >= kDigestOffset + kDigestBytes);
    assert(digest.size() == kDigestBytes * 2);
    for (std::size_t index = 0; index < kDigestBytes; ++index) {
        (*file)[kDigestOffset + index] = static_cast<std::uint8_t>(
            std::stoul(digest.substr(index * 2, 2), nullptr, 16));
    }
}

} // namespace

int main() {
    // Known-answer vectors (FIPS 180-4 Appendix B / widely published test
    // vectors) pinning StateStore::checksum to standard SHA-256. A prior
    // build had 3 of 64 round constants transcribed off by one hex digit,
    // which produced a self-consistent but non-standard digest -- these
    // vectors are the only thing that can catch that class of bug, since
    // save/load only ever compare the digest against itself.
    assert(StateStore::checksum({}) ==
           "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert(StateStore::checksum({'a', 'b', 'c'}) ==
           "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    {
        const std::vector<std::uint8_t> million_a(1000000, 'a');
        assert(StateStore::checksum(million_a) ==
               "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    const auto root = std::filesystem::temp_directory_path() /
        ("p4-staged-state-store-" + std::to_string(
            std::chrono::steady_clock::now().time_since_epoch().count()));
    StateStore store(root);
    const auto payload = request();
    const std::vector<std::uint8_t> original{'o', 'p', 'a', 'q', 'u', 'e'};
    staged::runtime::StateFileInfo saved;
    std::string error;
    assert(store.save(payload, original, &saved, &error));
    assert(saved.bytes == original.size());
    assert(saved.checksum.size() == 64);

    std::vector<std::uint8_t> restored;
    staged::runtime::StateFileInfo loaded;
    assert(store.load(payload, &restored, &loaded, &error));
    assert(restored == original);
    assert(loaded.checksum == saved.checksum);

    // A v2 record did not label its digest algorithm. Simulate an actual file
    // written by the pre-fix implementation by replacing only its persisted
    // digest, then prove that load accepts exactly that legacy digest and no
    // other malformed value.
    const auto path = store.path_for(payload.cache_key);
    const auto standard_file = read_file(path);
    assert(persisted_digest(standard_file) == saved.checksum);
    const auto legacy_checksum = staged::runtime::detail::legacy_checksum_hex(original);
    assert(legacy_checksum != saved.checksum);
    auto legacy_file = standard_file;
    replace_digest(&legacy_file, legacy_checksum);
    write_file(path, legacy_file);
    auto legacy_request = payload;
    legacy_request.expected_checksum = legacy_checksum;
    assert(store.load(legacy_request, &restored, &loaded, &error));
    assert(restored == original);
    assert(loaded.checksum == legacy_checksum);
    legacy_file[kDigestOffset] ^= 0x01U;
    write_file(path, legacy_file);
    assert(!store.load(legacy_request, &restored, nullptr, &error));
    assert(error == "KV state checksum is invalid");

    // Newly persisted records always retain standard SHA-256 and reject a
    // legacy receipt checksum.
    write_file(path, standard_file);
    assert(store.load(payload, &restored, &loaded, &error));
    assert(loaded.checksum == saved.checksum);
    assert(!store.load(legacy_request, &restored, nullptr, &error));

    auto wrong = payload;
    wrong.model_identity = "other-model";
    assert(!store.load(wrong, &restored, nullptr, &error));
    auto wrong_context = payload;
    wrong_context.context_identity = "n_ctx=1024";
    assert(!store.load(wrong_context, &restored, nullptr, &error));
    auto wrong_build = payload;
    wrong_build.build_identity = "different-llama-build";
    assert(!store.load(wrong_build, &restored, nullptr, &error));
    auto wrong_runtime = payload;
    wrong_runtime.runtime_identity = "staged-llama-kv-manifest-v2";
    assert(!store.load(wrong_runtime, &restored, nullptr, &error));
    auto wrong_format = payload;
    wrong_format.kv_format = "K=2;V=2;flags=0";
    assert(!store.load(wrong_format, &restored, nullptr, &error));
    auto wrong_position = payload;
    wrong_position.token_position = 8;
    assert(!store.load(wrong_position, &restored, nullptr, &error));
    assert(!store.path_for("../escape", &error).empty() == false);

    {
        std::fstream file(store.path_for(payload.cache_key),
                          std::ios::in | std::ios::out | std::ios::binary);
        file.seekp(-1, std::ios::end);
        char corrupted = '\0';
        file.write(&corrupted, 1);
    }
    assert(!store.load(payload, &restored, nullptr, &error));

    // Re-save a valid record before testing the destructive command.
    assert(store.save(payload, original, nullptr, &error));
    assert(store.drop(payload, nullptr, &error));
    assert(!store.load(payload, &restored, nullptr, &error));

    // Concurrent writers must use distinct temporary names and leave one
    // complete, checksum-valid record rather than a partial or missing file.
    std::vector<std::thread> writers;
    std::atomic<int> writer_failures{0};
    std::string first_writer_error;
    std::mutex writer_error_mutex;
    for (int index = 0; index < 8; ++index) {
        writers.emplace_back([&store, payload, index, &writer_failures, &first_writer_error, &writer_error_mutex] {
            const std::vector<std::uint8_t> state{
                static_cast<std::uint8_t>('0' + index), 'w', 'r', 'i', 't', 'e'};
            std::string writer_error;
            if (!store.save(payload, state, nullptr, &writer_error)) {
                writer_failures.fetch_add(1);
                std::lock_guard<std::mutex> lock(writer_error_mutex);
                if (first_writer_error.empty()) first_writer_error = writer_error;
            }
        });
    }
    for (auto &writer : writers) writer.join();
    if (writer_failures.load() != 0) return 1;
    const auto concurrent_loaded = store.load(payload, &restored, nullptr, &error);
    if (!concurrent_loaded) return 1;
    assert(restored.size() == original.size());
    for (const auto &entry : std::filesystem::directory_iterator(root)) {
        assert(entry.path().filename().string().find(".tmp.") == std::string::npos);
    }
    std::error_code ignored;
    std::filesystem::remove_all(root, ignored);
    return 0;
}
