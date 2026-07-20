#pragma once

#include "llama.h"

#include <memory>
#include <string>

struct linkcpp_ring_options {
    int layer_begin = -1;
    int layer_end = -1;
    int listen_port = 0;
    std::string next_endpoint;
    // NAT traversal: if a neighbour is behind NAT it dials us, so we accept the
    // edge we would normally dial. dial_prev_endpoint dials the predecessor
    // instead of accepting it; accept_next accepts the successor instead of
    // dialing it. Default (empty/false) is the original wiring.
    std::string dial_prev_endpoint;
    bool accept_next = false;

    bool enabled() const {
        return layer_begin >= 0 || layer_end >= 0 || listen_port != 0 || !next_endpoint.empty();
    }
    bool valid() const {
        return layer_begin == 0 && layer_end > 0 && listen_port > 0 && listen_port <= 65535
            && !next_endpoint.empty();
    }
};

class linkcpp_ring_executor {
public:
    explicit linkcpp_ring_executor(linkcpp_ring_options options);
    ~linkcpp_ring_executor();

    bool configure(const std::string & model_path);

private:
    struct impl;
    std::unique_ptr<impl> pimpl;
};
