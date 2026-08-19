#include "state_store.hpp"

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

} // namespace

int main() {
    assert(StateStore::checksum({}) ==
           "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");

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
