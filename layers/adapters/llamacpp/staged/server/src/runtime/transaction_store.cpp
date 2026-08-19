#include "transaction_store.hpp"

#include <atomic>
#include <fstream>
#include <functional>
#include <system_error>
#include <thread>
#include <vector>

#include "state_store.hpp"

#ifdef _WIN32
#include <windows.h>
#else
#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>
#endif

namespace staged::runtime {
namespace {

std::atomic<std::uint64_t> temp_serial{0};

std::string hex_operation_id(const std::string &value) {
    static constexpr char hex[] = "0123456789abcdef";
    std::string result;
    result.reserve(value.size() * 2);
    for (const auto byte : value) {
        const auto ch = static_cast<unsigned char>(byte);
        result.push_back(hex[ch >> 4U]);
        result.push_back(hex[ch & 0x0fU]);
    }
    return result;
}

bool sync_file(const std::filesystem::path &path, std::string *error) {
#ifdef _WIN32
    const auto handle = CreateFileW(path.c_str(), GENERIC_READ | GENERIC_WRITE,
                                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                                    nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (handle == INVALID_HANDLE_VALUE || !FlushFileBuffers(handle)) {
        if (handle != INVALID_HANDLE_VALUE) CloseHandle(handle);
        if (error != nullptr) *error = "transaction receipt FlushFileBuffers failed";
        return false;
    }
    CloseHandle(handle);
    return true;
#else
    const auto fd = ::open(path.c_str(), O_RDONLY);
    if (fd < 0 || ::fsync(fd) != 0) {
        if (fd >= 0) ::close(fd);
        if (error != nullptr) *error = "transaction receipt fsync failed";
        return false;
    }
    ::close(fd);
    return true;
#endif
}

bool atomic_write(const std::filesystem::path &path,
                  const std::vector<std::uint8_t> &bytes, std::string *error) {
    std::error_code ec;
    std::filesystem::create_directories(path.parent_path(), ec);
    if (ec) {
        if (error != nullptr) *error = "transaction receipt directory creation failed";
        return false;
    }
    const auto serial = temp_serial.fetch_add(1, std::memory_order_relaxed);
#ifdef _WIN32
    const auto process_id = static_cast<unsigned long>(GetCurrentProcessId());
#else
    const auto process_id = static_cast<unsigned long>(::getpid());
#endif
    const auto thread_id = std::hash<std::thread::id>{}(std::this_thread::get_id());
    const auto temp = path.string() + ".tmp." + std::to_string(process_id)
        + "." + std::to_string(thread_id) + "." + std::to_string(serial);
    {
        std::ofstream output(temp, std::ios::binary | std::ios::trunc);
        if (!output) {
            if (error != nullptr) *error = "transaction receipt temp open failed";
            return false;
        }
        output.write(reinterpret_cast<const char *>(bytes.data()),
                     static_cast<std::streamsize>(bytes.size()));
        output.flush();
        if (!output) {
            if (error != nullptr) *error = "transaction receipt temp write failed";
            std::filesystem::remove(temp, ec);
            return false;
        }
    }
    if (!sync_file(temp, error)) {
        std::filesystem::remove(temp, ec);
        return false;
    }
#ifdef _WIN32
    bool moved = false;
    for (unsigned attempt = 0; attempt < 32; ++attempt) {
        if (MoveFileExW(std::filesystem::path(temp).c_str(), path.c_str(),
                        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
            moved = true;
            break;
        }
        const auto status = GetLastError();
        if (status != ERROR_ACCESS_DENIED && status != ERROR_SHARING_VIOLATION) break;
        Sleep(1);
    }
    if (!moved) {
        if (error != nullptr) *error = "transaction receipt replace failed";
        std::filesystem::remove(temp, ec);
        return false;
    }
#else
    std::filesystem::rename(temp, path, ec);
    if (ec) {
        if (error != nullptr) *error = "transaction receipt rename failed";
        std::filesystem::remove(temp, ec);
        return false;
    }
    const auto directory = ::open(path.parent_path().c_str(), O_RDONLY | O_DIRECTORY);
    if (directory < 0 || ::fsync(directory) != 0) {
        if (directory >= 0) ::close(directory);
        if (error != nullptr) *error = "transaction receipt parent directory fsync failed";
        return false;
    }
    ::close(directory);
#endif
    return true;
}

bool same_identity(const protocol::KvPayload &request, const protocol::KvReceipt &receipt) {
    return request.operation_id == receipt.operation_id
        && request.sequence_id == receipt.sequence_id
        && request.cache_key == receipt.cache_key
        && request.model_identity == receipt.model_identity
        && request.stage_begin == receipt.stage_begin
        && request.stage_end == receipt.stage_end
        && (request.flags == 0 || request.flags == receipt.kind);
}

} // namespace

TransactionStore::TransactionStore(std::filesystem::path root)
    : root_(std::move(root)) {}

TransactionStore::Lease::Lease(Lease &&other) noexcept
    : handle_(other.handle_), path_(std::move(other.path_)) {
    other.handle_ = invalid_handle;
}

TransactionStore::Lease &TransactionStore::Lease::operator=(Lease &&other) noexcept {
    if (this != &other) {
        release();
        handle_ = other.handle_;
        path_ = std::move(other.path_);
        other.handle_ = invalid_handle;
    }
    return *this;
}

TransactionStore::Lease::~Lease() {
    release();
}

void TransactionStore::Lease::release() noexcept {
    if (handle_ == invalid_handle) return;
#ifdef _WIN32
    OVERLAPPED overlapped{};
    UnlockFileEx(reinterpret_cast<HANDLE>(handle_), 0, MAXDWORD, MAXDWORD, &overlapped);
    CloseHandle(reinterpret_cast<HANDLE>(handle_));
#else
    (void)::flock(static_cast<int>(handle_), LOCK_UN);
    ::close(static_cast<int>(handle_));
#endif
    handle_ = invalid_handle;
}

std::filesystem::path TransactionStore::path_for(const std::string &operation_id,
                                                  std::string *error) const {
    if (root_.empty() || operation_id.empty() || operation_id.size() > 4096) {
        if (error != nullptr) *error = "invalid transaction receipt identity";
        return {};
    }
    return root_ / ".p4-transactions" / (hex_operation_id(operation_id) + ".receipt");
}

TransactionStore::Lease TransactionStore::acquire(const std::string &operation_id,
                                                   std::string *error) const {
    const auto receipt = path_for(operation_id, error);
    if (receipt.empty()) return {};
    auto lock = receipt;
    lock.replace_extension(".lock");
    std::error_code ec;
    std::filesystem::create_directories(lock.parent_path(), ec);
    if (ec) {
        if (error != nullptr) *error = "transaction lease directory creation failed";
        return {};
    }
#ifdef _WIN32
    const auto handle = CreateFileW(lock.c_str(), GENERIC_READ | GENERIC_WRITE,
                                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                                    nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (handle == INVALID_HANDLE_VALUE) {
        if (error != nullptr) *error = "transaction lease open failed";
        return {};
    }
    OVERLAPPED overlapped{};
    if (!LockFileEx(handle, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                    0, MAXDWORD, MAXDWORD, &overlapped)) {
        CloseHandle(handle);
        if (error != nullptr) *error = "transaction lease busy";
        return {};
    }
    return Lease(reinterpret_cast<std::intptr_t>(handle), lock);
#else
    const auto descriptor = ::open(lock.c_str(), O_CREAT | O_RDWR, 0666);
    if (descriptor < 0) {
        if (error != nullptr) *error = "transaction lease open failed";
        return {};
    }
    if (::flock(descriptor, LOCK_EX | LOCK_NB) != 0) {
        ::close(descriptor);
        if (error != nullptr) *error = "transaction lease busy";
        return {};
    }
    return Lease(static_cast<std::intptr_t>(descriptor), lock);
#endif
}

bool TransactionStore::prepare(const protocol::KvPayload &request,
                               protocol::KvReceipt *receipt, std::string *error) const {
    if (receipt == nullptr || request.operation_id.empty()
        || request.flags < protocol::kKvPersist || request.flags > protocol::kKvDiscard) {
        if (error != nullptr) *error = "invalid transaction prepare request";
        return false;
    }
    protocol::KvReceipt existing;
    std::string read_error;
    if (read(request, &existing, &read_error)) {
        if (!same_identity(request, existing)) {
            if (error != nullptr) *error = "transaction operation identity mismatch";
            return false;
        }
        *receipt = existing;
        return true;
    }
    if (read_error != "transaction receipt absent") {
        if (error != nullptr) *error = read_error;
        return false;
    }
    protocol::KvReceipt created{
        request.operation_id, request.sequence_id, request.cache_key, request.model_identity,
        request.stage_begin, request.stage_end, request.flags,
        protocol::KvReceiptState::Prepared, 0, std::string(64, '0'), "prepared"};
    if (!write(created, error)) return false;
    *receipt = std::move(created);
    return true;
}

bool TransactionStore::read(const protocol::KvPayload &request,
                            protocol::KvReceipt *receipt, std::string *error) const {
    const auto path = path_for(request.operation_id, error);
    if (path.empty()) return false;
    std::ifstream input(path, std::ios::binary);
    if (!input) {
        if (error != nullptr) *error = "transaction receipt absent";
        return false;
    }
    std::vector<std::uint8_t> bytes;
    char byte = 0;
    while (input.get(byte)) {
        bytes.push_back(static_cast<std::uint8_t>(static_cast<unsigned char>(byte)));
    }
    try {
        *receipt = protocol::KvReceipt::decode(bytes, protocol::ProtocolLimits{});
    } catch (const protocol::ProtocolError &exception) {
        if (error != nullptr) *error = std::string("transaction receipt corrupt: ") + exception.what();
        return false;
    }
    if (!same_identity(request, *receipt)) {
        if (error != nullptr) *error = "transaction receipt identity mismatch";
        return false;
    }
    return true;
}

bool TransactionStore::write(const protocol::KvReceipt &receipt, std::string *error) const {
    const auto path = path_for(receipt.operation_id, error);
    if (path.empty()) return false;
    try {
        return atomic_write(path, receipt.encode(protocol::ProtocolLimits{}), error);
    } catch (const protocol::ProtocolError &exception) {
        if (error != nullptr) *error = exception.what();
        return false;
    }
}

bool TransactionStore::erase(const protocol::KvPayload &request, std::string *error) const {
    const auto path = path_for(request.operation_id, error);
    if (path.empty()) return false;
    std::error_code ec;
    std::filesystem::remove(path, ec);
    if (ec) {
        if (error != nullptr) *error = "transaction receipt removal failed";
        return false;
    }
    return true;
}

} // namespace staged::runtime
