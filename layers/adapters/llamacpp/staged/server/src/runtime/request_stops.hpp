#pragma once

#include <cstddef>
#include <string>
#include <vector>

namespace staged::llama_runtime {

struct StopFilterResult final {
    std::string text;
    bool stopped = false;
};

bool parse_request_stops(const std::string & raw,
                         std::vector<std::string> * stops,
                         std::string * error);

StopFilterResult filter_request_stops(const std::string & generated,
                                      std::size_t emitted,
                                      const std::vector<std::string> & stops,
                                      bool end_of_generation);

} // namespace staged::llama_runtime
