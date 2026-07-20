#pragma once

#include <cstdint>

#ifndef LINKCPP_RING_BUILD_ID
#define LINKCPP_RING_BUILD_ID "unknown"
#endif

inline constexpr uint8_t LINKCPP_RING_WIRE_VERSION = 1;
// ABI 5: a 1-byte connection role preamble precedes the hello frame so a NAT'd
// stage can dial both neighbours outbound (see send_role/recv_role).
// ABI 6: stage startup carries whether rank-local KV is offloaded to the GPU.
inline constexpr uint32_t LINKCPP_RING_ADAPTER_ABI = 6;
inline constexpr const char * LINKCPP_RING_PROTOCOL_NAME = "linkcpp-stage-v1";
inline constexpr const char * LINKCPP_RING_BUILD_ID_STRING = LINKCPP_RING_BUILD_ID;

constexpr uint64_t linkcpp_ring_build_fingerprint(const char * value) {
    uint64_t hash = UINT64_C(14695981039346656037);
    while (*value) {
        hash ^= static_cast<uint8_t>(*value++);
        hash *= UINT64_C(1099511628211);
    }
    return hash;
}

inline constexpr uint64_t LINKCPP_RING_BUILD_FINGERPRINT =
    linkcpp_ring_build_fingerprint(LINKCPP_RING_BUILD_ID_STRING);
