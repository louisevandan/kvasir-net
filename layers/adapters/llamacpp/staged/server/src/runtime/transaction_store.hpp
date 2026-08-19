#pragma once

#include <filesystem>
#include <cstdint>
#include <string>

#include "protocol.hpp"

namespace staged::runtime {

// Durable intent/receipt journal for one stage server.  It deliberately uses
// one file per operation so recovery never has to rewrite an unrelated
// operation.  A Committing receipt is ambiguous after a crash and is exposed
// as Inconsistent; the store never guesses whether the KV file was mutated.
class TransactionStore final {
public:
    class Lease final {
    public:
        Lease() = default;
        Lease(const Lease &) = delete;
        Lease &operator=(const Lease &) = delete;
        Lease(Lease &&other) noexcept;
        Lease &operator=(Lease &&other) noexcept;
        ~Lease();

        [[nodiscard]] bool held() const noexcept { return handle_ != invalid_handle; }

    private:
        friend class TransactionStore;
        explicit Lease(std::intptr_t handle, std::filesystem::path path)
            : handle_(handle), path_(std::move(path)) {}
        void release() noexcept;
        static constexpr std::intptr_t invalid_handle = -1;
        std::intptr_t handle_ = invalid_handle;
        std::filesystem::path path_;
    };

    explicit TransactionStore(std::filesystem::path root);

    [[nodiscard]] bool available() const noexcept { return !root_.empty(); }
    [[nodiscard]] bool prepare(const protocol::KvPayload &, protocol::KvReceipt *,
                               std::string *error = nullptr) const;
    [[nodiscard]] bool read(const protocol::KvPayload &, protocol::KvReceipt *,
                             std::string *error = nullptr) const;
    [[nodiscard]] bool write(const protocol::KvReceipt &, std::string *error = nullptr) const;
    [[nodiscard]] bool erase(const protocol::KvPayload &, std::string *error = nullptr) const;
    [[nodiscard]] Lease acquire(const std::string &operation_id,
                                std::string *error = nullptr) const;

private:
    [[nodiscard]] std::filesystem::path path_for(const std::string &operation_id,
                                                  std::string *error) const;
    std::filesystem::path root_;
};

} // namespace staged::runtime
