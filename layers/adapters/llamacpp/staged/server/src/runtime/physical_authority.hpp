#pragma once

#include <cstddef>
#include <cstdint>
#include <map>
#include <string>
#include <utility>
#include <vector>

// Adapter-native authority: no llama/ggml type or backend-specific rule.
// This is in-memory safety, not restart recovery or physical-execution dedup.
namespace staged::runtime {

// Canonical request identity is session + one NUL + request. All individual
// names are nonempty, valid UTF-8, and contain no NUL themselves.
bool canonical_physical_request_identity(const std::string & session,
    const std::string & key, const std::string & request);

struct PhysicalIdentity final {
    std::uint64_t load_generation = 0;
    std::uint64_t incarnation = 0;
    std::uint32_t slot = 0;
    std::string session;
    std::string key;
    bool operator==(const PhysicalIdentity & other) const;
};

struct PhysicalAdmission final {
    PhysicalIdentity identity;
    bool begins_request = false; // Prefill at position zero, never merely a free slot.
};

struct PhysicalControlIdentity final {
    PhysicalIdentity identity;
    std::uint64_t operation_id = 0;
    std::size_t prefix_bytes = 0;
};

bool decode_physical_control_identity(const std::vector<std::uint8_t> & bytes,
    PhysicalControlIdentity * result, std::string * error);

class PhysicalAuthority final {
public:
    struct RowsPlan { std::map<std::uint32_t, PhysicalIdentity> acquired; };
    enum class ControlDecision { Rejected, Execute, Replay };
    bool bind(std::uint64_t generation, std::uint32_t capacity, std::string * error);
    bool bound() const noexcept { return generation_ != 0; }
    bool fenced() const noexcept { return fenced_; }
    void fence() noexcept { fenced_ = true; }
    bool prepare_rows(const std::vector<PhysicalAdmission> & rows,
        RowsPlan * plan, std::string * error) const;
    void commit_rows(const RowsPlan & plan);
    ControlDecision prepare_control(const PhysicalControlIdentity & control,
        bool release, const std::vector<std::uint8_t> & request,
        std::vector<std::uint8_t> * cached, std::string * error) const;
    void commit_control(const PhysicalControlIdentity & control, bool release,
        const std::vector<std::uint8_t> & request,
        const std::vector<std::uint8_t> & response);

    // A full budget rejects before execution. Watermarks are never evicted to
    // make space: eviction would turn an old incarnation into a new request.
    static constexpr std::size_t max_watermarks = 65536;
    static constexpr std::size_t max_control_bytes = 1024 * 1024;
    static constexpr std::size_t max_receipt_bytes = 64 * 1024 * 1024;

private:
    struct Slot {
        PhysicalIdentity identity;
        bool released = false;
        bool last_release = false;
        std::uint64_t last_operation = 0;
        std::vector<std::uint8_t> request;
        std::vector<std::uint8_t> response;
    };
    std::uint64_t generation_ = 0;
    std::uint32_t capacity_ = 0;
    bool fenced_ = false;
    std::size_t receipt_bytes_ = 0;
    std::map<std::uint32_t, Slot> slots_;
    std::map<std::pair<std::string, std::uint32_t>, std::uint64_t> retired_;
};

} // namespace staged::runtime
