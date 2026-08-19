#pragma once

#include <string>

#include "protocol.hpp"

namespace staged::runtime {

class KvBridge {
public:
    virtual ~KvBridge() = default;
    [[nodiscard]] virtual bool save(const protocol::KvPayload &, protocol::KvResult *,
                                    std::string *) = 0;
    [[nodiscard]] virtual bool restore(const protocol::KvPayload &, protocol::KvResult *,
                                       std::string *) = 0;
    [[nodiscard]] virtual bool drop(const protocol::KvPayload &, protocol::KvResult *,
                                    std::string *) = 0;
};

} // namespace staged::runtime
