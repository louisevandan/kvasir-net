#include "protocol.hpp"

#include <array>
#include <algorithm>
#include <limits>
#include <utility>

namespace staged::protocol {
#include "protocol_frame.inc"
#include "protocol_hop.inc"
#include "protocol_kv.inc"

} // namespace staged::protocol
