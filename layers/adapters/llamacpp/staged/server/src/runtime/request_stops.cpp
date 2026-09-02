#include "request_stops.hpp"

#include "compat/p4_llama_compat.hpp"

#include <algorithm>
#include <limits>

#include <nlohmann/json.hpp>

namespace staged::llama_runtime {

namespace {

constexpr std::size_t kMaxStops = 128;
constexpr std::size_t kMaxStopBytes = 64 * 1024;

bool fail(std::string * error, const std::string & detail) {
    if (error != nullptr) *error = detail;
    return false;
}

} // namespace

bool parse_request_stops(const std::string & raw,
                         std::vector<std::string> * stops,
                         std::string * error) {
    if (stops == nullptr) return fail(error, "request stop output is null");
    stops->clear();
    if (raw.empty()) return true;
    nlohmann::ordered_json values;
    try {
        values = nlohmann::ordered_json::parse(raw);
    } catch (const std::exception & exception) {
        return fail(error, std::string("invalid request options JSON: ") + exception.what());
    }
    if (!values.is_object() || !values.contains("stop")) return true;
    const auto & value = values.at("stop");
    if (value.is_string()) {
        stops->push_back(value.get<std::string>());
    } else if (value.is_array()) {
        for (const auto & item : value) {
            if (!item.is_string()) return fail(error, "request stop entries must be strings");
            stops->push_back(item.get<std::string>());
        }
    } else {
        return fail(error, "request stop must be a string or string array");
    }
    stops->erase(std::remove_if(stops->begin(), stops->end(),
                               [](const auto & stop) { return stop.empty(); }), stops->end());
    std::size_t bytes = 0;
    for (const auto & stop : *stops) {
        if (stop.size() > kMaxStopBytes || bytes > kMaxStopBytes - stop.size()) {
            return fail(error, "request stop strings exceed the byte limit");
        }
        bytes += stop.size();
    }
    if (stops->size() > kMaxStops || bytes > kMaxStopBytes) {
        return fail(error, "request stop strings exceed the configured limit");
    }
    return true;
}

StopFilterResult filter_request_stops(const std::string & generated,
                                      std::size_t emitted,
                                      const std::vector<std::string> & stops,
                                      bool end_of_generation) {
    emitted = std::min(emitted, generated.size());
    std::size_t end = generated.size();
    bool stopped = false;
    for (const auto & stop : stops) {
        const auto found = generated.find(stop, emitted);
        if (found != std::string::npos && found < end) {
            end = found;
            stopped = true;
        }
    }
    if (!stopped && !end_of_generation) {
        for (const auto & stop : stops) {
            const auto partial = p4_llama_compat::find_partial_stop(generated, stop);
            if (partial != std::string::npos) end = std::min(end, partial);
        }
    }
    return {generated.substr(emitted, end - emitted), stopped};
}

} // namespace staged::llama_runtime
