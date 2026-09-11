#include "stage_memory_plan.hpp"

#include <algorithm>
#include <utility>

namespace staged::llama_runtime {

bool validate_layer_device_expectations(
        const std::vector<LayerDeviceExpectation> & expectations,
        std::int32_t begin, std::int32_t end, std::string * error) {
    if (expectations.empty()) return true;
    auto sorted = expectations;
    std::sort(sorted.begin(), sorted.end(), [](const auto & a, const auto & b) {
        return a.begin < b.begin;
    });
    auto next = begin;
    bool valid = begin >= 0 && end > begin;
    for (const auto & range : sorted) {
        valid = valid && range.begin == next && range.end > range.begin &&
            range.end <= end && !range.device.empty() &&
            std::none_of(range.device.begin(), range.device.end(), [](unsigned char c) {
                return c < 0x20 || c == 0x7f;
            });
        next = range.end;
    }
    if (valid && next == end) return true;
    if (error) *error = "--expect-layer-device ranges must cover the active cut exactly, without gaps or overlaps";
    return false;
}

bool validate_stage_layer_devices(
        const LoadConfig & config, const StageMemoryPlan & measured, std::string * error) {
    if (!validate_layer_device_expectations(config.layer_device_expectations,
            config.layer_begin, config.layer_end, error)) return false;
    if (config.layer_device_expectations.empty()) return true;
    if (!measured.layer_device_query_supported) {
        if (error) *error = "layer device expectations require a native with layer-device query support";
        return false;
    }
    const auto extent = static_cast<std::size_t>(config.layer_end - config.layer_begin);
    if (measured.layer_default_devices.size() != extent) {
        if (error) *error = "layer device evidence does not cover the declared repeating-layer cut";
        return false;
    }
    for (std::size_t i = 0; i < extent; ++i) {
        const auto & actual = measured.layer_default_devices[i];
        if (actual.layer != config.layer_begin + static_cast<std::int32_t>(i)) {
            if (error) *error = "layer device evidence has missing, repeated or unordered layers";
            return false;
        }
        const auto expected = std::find_if(config.layer_device_expectations.begin(),
            config.layer_device_expectations.end(), [&](const auto & range) {
                return range.begin <= actual.layer && actual.layer < range.end;
            });
        if (expected == config.layer_device_expectations.end() || expected->device != actual.device) {
            if (error) *error = "layer device mismatch: layer=" + std::to_string(actual.layer) +
                " expected=" + (expected == config.layer_device_expectations.end() ? "missing" : expected->device) +
                " actual=" + actual.device;
            return false;
        }
    }
    return true;
}

bool measure_stage_layer_devices(
        const llama_model * model, const LoadConfig & config,
        StageMemoryPlan * result, std::string * error) {
    if (!model || !result) {
        if (error) *error = "cannot inspect layer devices of an unloaded stage";
        return false;
    }
    if (!validate_layer_device_expectations(config.layer_device_expectations,
            config.layer_begin, config.layer_end, error)) return false;
    result->layer_device_query_supported = p4_llama_compat::has_layer_device_query();
    result->layer_device_expectations_checked = false;
    result->layer_default_devices.clear();
    if (result->layer_device_query_supported) {
        const auto end = std::min(config.layer_end, llama_model_n_layer(model));
        for (auto layer = config.layer_begin; layer < end; ++layer) {
            auto * device = p4_llama_compat::model_layer_device(model, layer);
            if (!device) {
                if (error) *error = "native could not report default device for layer=" + std::to_string(layer);
                return false;
            }
            result->layer_default_devices.push_back({layer, ggml_backend_dev_name(device)});
        }
    }
    if (!validate_stage_layer_devices(config, *result, error)) return false;
    result->layer_device_expectations_checked = !config.layer_device_expectations.empty();
    return true;
}

} // namespace staged::llama_runtime
