#include "request_stops.hpp"

#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <iostream>

int main() {
    using staged::llama_runtime::filter_request_stops;
    using staged::llama_runtime::parse_request_stops;

    std::vector<std::string> stops;
    std::string error;
    assert(parse_request_stops(R"({"temperature":0,"stop":[" Question","<END>"]})",
                               &stops, &error));
    assert(stops.size() == 2);

    const auto partial = filter_request_stops("The answer is Paris. Quest", 0, stops, false);
    assert(partial.text == "The answer is Paris.");
    assert(!partial.stopped);

    const auto complete = filter_request_stops("The answer is Paris. Question:",
                                               partial.text.size(), stops, false);
    assert(complete.text.empty());
    assert(complete.stopped);

    const auto eos_flush = filter_request_stops("The answer is Paris. Quest",
                                                partial.text.size(), stops, true);
    assert(eos_flush.text == " Quest");
    assert(!eos_flush.stopped);

    assert(!parse_request_stops(R"({"stop":[1]})", &stops, &error));
    std::cout << "REQUEST_STOPS_OK\n";
}
