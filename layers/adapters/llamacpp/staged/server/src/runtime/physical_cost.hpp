#pragma once

#include "physical_wire.hpp"
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#ifdef _WIN32
#include <process.h>
#else
#include <unistd.h>
#endif

namespace staged::llama_runtime::cost {

using Clock = std::chrono::steady_clock;
inline long long micros(Clock::time_point a, Clock::time_point b) {
    return std::chrono::duration_cast<std::chrono::microseconds>(b - a).count();
}
inline bool enabled() {
    static const bool value = [] {
        const auto * p = std::getenv("P4_STAGED_TRACE_COST");
        return p != nullptr && std::strcmp(p, "1") == 0;
    }();
    return value;
}
inline long process() {
#ifdef _WIN32
    return _getpid();
#else
    return getpid();
#endif
}
class Call;
class Native;
inline thread_local Call * active_call = nullptr;
inline thread_local Native * active_native = nullptr;
inline std::atomic<std::uint64_t> next_call{1};

class Call final {
    Call * previous_ = active_call;
    Clock::time_point started_{};
    bool ok_ = false;
public:
    const bool traced = enabled();
    std::uint64_t id = 0;
    long long parse_us = 0, match_us = 0, sample_us = 0, encode_us = 0;
    explicit Call(std::size_t input_bytes) {
        if (!traced) return;
        id = next_call.fetch_add(1, std::memory_order_relaxed);
        active_call = this;
        started_ = Clock::now();
        std::fprintf(stderr, "P4_COST_CALL_BEGIN pid=%ld call=%llu input_bytes=%zu\n",
            process(), static_cast<unsigned long long>(id), input_bytes);
    }
    void finish(const std::vector<RoutedPhysicalExecution> & output) {
        if (!traced) return;
        for (const auto & capsule : output) {
            if (capsule.owners.empty()) continue;
            const auto & owner = capsule.owners.front();
            std::string session;
            constexpr char hex[] = "0123456789abcdef";
            for (unsigned char c : owner.session_id) {
                session += hex[c >> 4]; session += hex[c & 15];
            }
            std::fprintf(stderr, "P4_COST_BIND pid=%ld call=%llu execution=%llu load=%llu session=%s rows=%zu\n",
                process(), static_cast<unsigned long long>(id),
                static_cast<unsigned long long>(capsule.execution_id),
                static_cast<unsigned long long>(owner.load_generation), session.c_str(), capsule.owners.size());
        }
        ok_ = true;
    }
    ~Call() {
        if (!traced) return;
        std::fprintf(stderr, "P4_COST_CALL_END pid=%ld call=%llu ok=%d total_us=%lld parse_us=%lld match_us=%lld sample_us=%lld encode_us=%lld\n",
            process(), static_cast<unsigned long long>(id), ok_ ? 1 : 0,
            micros(started_, Clock::now()), parse_us, match_us, sample_us, encode_us);
        active_call = previous_;
    }
};

class Native final {
    Native * previous_ = active_native;
    Call * call_ = active_call;
    Clock::time_point started_{}, decoding_{}, decoded_{};
    const void * context_;
    bool began_ = false, decoded_ok_ = false, ok_ = false;
public:
    long long capture_before = 0, capture_after = 0;
    std::size_t capture_bytes = 0;
    explicit Native(const void * context) : context_(context) {
        if (!call_) return;
        active_native = this; started_ = Clock::now();
    }
    bool decoded() const { return decoded_ok_; }
    void begin_decode(std::size_t rows) {
        if (!call_) return;
        std::fprintf(stderr, "P4_COST_CONTEXT pid=%ld call=%llu ctx=%llx rows=%zu\n",
            process(), static_cast<unsigned long long>(call_->id),
            static_cast<unsigned long long>(reinterpret_cast<std::uintptr_t>(context_)), rows);
        decoding_ = Clock::now(); began_ = true;
    }
    void end_decode() {
        if (call_) { decoded_ = Clock::now(); decoded_ok_ = true; }
    }
    void finish() { ok_ = true; }
    ~Native() {
        if (!call_) return;
        const auto end = Clock::now();
        std::fprintf(stderr, "P4_COST_NATIVE pid=%ld call=%llu ctx=%llx ok=%d total_us=%lld setup_us=%lld decode_us=%lld capture_us=%lld post_us=%lld capture_bytes=%zu\n",
            process(), static_cast<unsigned long long>(call_->id),
            static_cast<unsigned long long>(reinterpret_cast<std::uintptr_t>(context_)),
            ok_ && began_ && decoded_ok_ ? 1 : 0, micros(started_, end),
            began_ ? micros(started_, decoding_) : -1,
            decoded_ok_ ? micros(decoding_, decoded_) - capture_before : -1,
            capture_before + capture_after,
            decoded_ok_ ? micros(decoded_, end) - capture_after : -1, capture_bytes);
        active_native = previous_;
    }
};

class Capture final {
    Native * native_ = active_native;
    Clock::time_point started_{};
public:
    std::size_t bytes = 0;
    Capture() { if (native_) started_ = Clock::now(); }
    ~Capture() {
        if (!native_) return;
        const auto elapsed = micros(started_, Clock::now());
        (native_->decoded() ? native_->capture_after : native_->capture_before) += elapsed;
        native_->capture_bytes += bytes;
    }
};

} // namespace staged::llama_runtime::cost
