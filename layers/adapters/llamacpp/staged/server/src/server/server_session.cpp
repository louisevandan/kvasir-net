#include "server.hpp"

#include <filesystem>
#include <utility>

namespace staged::server {

Session::Session(
        Capabilities capabilities
#ifdef P4_STAGED_WITH_LLAMA
        , llama_runtime::StageRuntime * llama_runtime
#endif
        , HopExecutor hop_executor
        , std::filesystem::path transaction_root)
    : transaction_store_(std::move(transaction_root))
    , capabilities_(capabilities)
#ifdef P4_STAGED_WITH_LLAMA
    , llama_runtime_(llama_runtime)
#endif
    , hop_executor_(std::move(hop_executor)) {
}

bool Session::feed_plan(const std::vector<std::uint8_t> & bytes, std::string * error) {
    const auto result = runtime_.stdin_plan().feed(bytes.data(), bytes.size());
    if (!result.ok()) {
        if (error != nullptr) *error = "startup plan rejected";
        return false;
    }
    return true;
}

protocol::Frame Session::status(
        protocol::Operation operation, const std::string & message) const {
    return protocol::Frame::make(
        operation, std::vector<std::uint8_t>(message.begin(), message.end()));
}

protocol::Frame Session::error(const std::string & message) const {
    return status(protocol::Operation::Error, message);
}

} // namespace staged::server
