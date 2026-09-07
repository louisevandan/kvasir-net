#include "physical_authority.hpp"
#include "utf8_text.hpp"

#include <algorithm>
#include <limits>

namespace staged::runtime {
namespace {
bool reject(const char * text, std::string * error) {
    if (error != nullptr) *error = text;
    return false;
}
bool valid_identity(const PhysicalIdentity & value) {
    return value.load_generation != 0 && value.incarnation != 0
        && !value.session.empty() && value.session.size() <= 4096
        && value.key.size() <= 4096 && value.key.size() > value.session.size()
        && canonical_physical_request_identity(value.session, value.key,
            value.key.substr(value.session.size() + 1));
}
} // namespace

bool canonical_physical_request_identity(const std::string & session,
        const std::string & key, const std::string & request) {
    return !session.empty() && !request.empty()
        && session.find('\0') == std::string::npos
        && request.find('\0') == std::string::npos
        && valid_utf8_text(session) && valid_utf8_text(request)
        && key.size() == session.size() + 1 + request.size()
        && key.compare(0, session.size(), session) == 0
        && key[session.size()] == '\0'
        && key.compare(session.size() + 1, request.size(), request) == 0;
}

bool PhysicalIdentity::operator==(const PhysicalIdentity & other) const {
    return load_generation == other.load_generation && incarnation == other.incarnation
        && slot == other.slot && session == other.session && key == other.key;
}

bool decode_physical_control_identity(const std::vector<std::uint8_t> & bytes,
        PhysicalControlIdentity * result, std::string * error) {
    if (result == nullptr || bytes.size() < 44
        || bytes.size() > PhysicalAuthority::max_control_bytes
        || bytes[0] != 'P' || bytes[1] != '4' || bytes[2] != 'I' || bytes[3] != 'D'
        || bytes[4] != 1 || bytes[5] != 0 || bytes[6] != 0 || bytes[7] != 0) {
        return reject("invalid identity-bound control header", error);
    }
    auto u32 = [&](std::size_t offset) {
        std::uint32_t value = 0;
        for (unsigned i = 0; i < 4; ++i) value |= std::uint32_t(bytes[offset + i]) << (i * 8U);
        return value;
    };
    auto u64 = [&](std::size_t offset) {
        std::uint64_t value = 0;
        for (unsigned i = 0; i < 8; ++i) value |= std::uint64_t(bytes[offset + i]) << (i * 8U);
        return value;
    };
    PhysicalControlIdentity parsed;
    parsed.identity.load_generation = u64(8);
    parsed.identity.incarnation = u64(16);
    parsed.operation_id = u64(24);
    parsed.identity.slot = u32(32);
    std::size_t offset = 36;
    auto string = [&](std::string * value) {
        if (bytes.size() - offset < 4) return false;
        const auto size = u32(offset);
        offset += 4;
        if (size == 0 || size > 4096 || size > bytes.size() - offset) return false;
        value->assign(reinterpret_cast<const char *>(bytes.data() + offset), size);
        offset += size;
        return true;
    };
    if (!string(&parsed.identity.session) || !string(&parsed.identity.key)
        || !valid_identity(parsed.identity) || parsed.operation_id == 0
        || parsed.identity.slot > std::uint32_t(std::numeric_limits<std::int32_t>::max())) {
        return reject("invalid physical control identity", error);
    }
    parsed.prefix_bytes = offset;
    *result = std::move(parsed);
    return true;
}

bool PhysicalAuthority::bind(std::uint64_t generation, std::uint32_t capacity,
        std::string * error) {
    if (generation == 0 || capacity == 0 || (bound() && generation_ != generation)) {
        return reject("physical load binding is invalid or conflicting", error);
    }
    if (bound()) return true; // Never clear authority/receipts on a repeated bind.
    generation_ = generation;
    capacity_ = capacity;
    return true;
}

bool PhysicalAuthority::prepare_rows(const std::vector<PhysicalAdmission> & rows,
        RowsPlan * plan, std::string * error) const {
    if (!bound() || fenced_ || rows.empty() || plan == nullptr) {
        return reject("physical authority is unbound, fenced, or empty", error);
    }
    RowsPlan candidate;
    std::size_t new_entries = 0;
    for (const auto & row : rows) {
        const auto & id = row.identity;
        if (!valid_identity(id) || id.load_generation != generation_ || id.slot >= capacity_) {
            return reject("physical row has stale or invalid identity", error);
        }
        const auto pending = candidate.acquired.find(id.slot);
        if (pending != candidate.acquired.end()) {
            if (!(pending->second == id)) return reject("physical batch aliases a slot owner", error);
            continue;
        }
        const auto current = slots_.find(id.slot);
        if (current != slots_.end() && !current->second.released) {
            if (!(current->second.identity == id)) return reject("physical slot belongs to another request", error);
            continue;
        }
        const auto retired = retired_.find({id.session, id.slot});
        if (!row.begins_request || (retired != retired_.end() && id.incarnation <= retired->second)) {
            return reject("physical request is retired or lacks its initial prefill", error);
        }
        // Reserve the watermark entry at acquisition, before KV can exist.
        // This bounds all historical session/slot identities without eviction.
        if (retired == retired_.end() && retired_.size() + new_entries >= max_watermarks) {
            return reject("physical incarnation watermark budget is full", error);
        }
        if (retired == retired_.end()) ++new_entries;
        candidate.acquired.emplace(id.slot, id);
    }
    *plan = std::move(candidate);
    return true;
}

void PhysicalAuthority::commit_rows(const RowsPlan & plan) {
    for (const auto & entry : plan.acquired) {
        auto & slot = slots_[entry.first];
        receipt_bytes_ -= slot.request.size() + slot.response.size();
        slot = Slot{};
        slot.identity = entry.second;
        retired_.emplace(std::make_pair(entry.second.session, entry.second.slot), 0);
    }
}

PhysicalAuthority::ControlDecision PhysicalAuthority::prepare_control(
        const PhysicalControlIdentity & control, bool release,
        const std::vector<std::uint8_t> & request, std::vector<std::uint8_t> * cached,
        std::string * error) const {
    auto fail = [&](const char * text) { reject(text, error); return ControlDecision::Rejected; };
    const auto & id = control.identity;
    if (!bound() || fenced_ || !valid_identity(id) || id.load_generation != generation_
        || control.operation_id == 0 || request.size() > max_control_bytes || cached == nullptr) {
        return fail("physical control authority is stale, unbound or fenced");
    }
    const auto found = slots_.find(id.slot);
    if (found == slots_.end() || !(found->second.identity == id)) {
        return fail("physical control does not own this slot incarnation");
    }
    const auto & slot = found->second;
    if (control.operation_id == slot.last_operation) {
        if (slot.last_release != release || slot.request != request) {
            return fail("physical control operation conflicts with its receipt");
        }
        *cached = slot.response;
        return ControlDecision::Replay;
    }
    if (slot.released || control.operation_id < slot.last_operation) {
        return fail("physical control operation or incarnation is retired");
    }
    // Reserve worst-case response space before the native side effect. A
    // reply is constrained to max_control_bytes by its caller.
    const auto previous = slot.request.size() + slot.response.size();
    if (receipt_bytes_ - previous + request.size() + max_control_bytes > max_receipt_bytes) {
        return fail("physical control receipt budget is full");
    }
    return ControlDecision::Execute;
}

void PhysicalAuthority::commit_control(const PhysicalControlIdentity & control,
        bool release, const std::vector<std::uint8_t> & request,
        const std::vector<std::uint8_t> & response) {
    auto & slot = slots_.at(control.identity.slot);
    receipt_bytes_ -= slot.request.size() + slot.response.size();
    slot.last_operation = control.operation_id;
    slot.last_release = release;
    slot.request = request;
    slot.response = response;
    receipt_bytes_ += request.size() + response.size();
    if (release) {
        slot.released = true;
        auto & retired = retired_.at({control.identity.session, control.identity.slot});
        retired = std::max(retired, control.identity.incarnation);
    }
}

} // namespace staged::runtime
