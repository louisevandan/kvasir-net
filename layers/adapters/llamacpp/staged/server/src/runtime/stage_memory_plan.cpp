#include "stage_memory_plan.hpp"

#include "ggml-backend.h"
#include "llama-cpp.h"
#include "compat/p4_llama_compat.hpp"

#include <algorithm>
#include <exception>
#include <limits>
#include <map>
#include <set>
#include <sstream>
#include <utility>

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#elif defined(__APPLE__)
#include <mach/mach.h>
#include <sys/sysctl.h>
#elif defined(__linux__)
#include <sys/sysinfo.h>
#endif

namespace {

using staged::llama_runtime::LoadConfig;
using staged::llama_runtime::StageMemoryEntry;
using staged::llama_runtime::StageMemoryPlan;

bool has_mtp(const p4_llama_compat::LlamaPlan & params) {
    return params.requests_draft_mtp();
}

StageMemoryEntry device_entry(ggml_backend_dev_t device, std::size_t index) {
    StageMemoryEntry result;
    result.scope = "device";
    result.index = static_cast<std::int32_t>(index);
    result.name = ggml_backend_dev_name(device);
    result.description = ggml_backend_dev_description(device);
    std::size_t free = 0;
    std::size_t total = 0;
    ggml_backend_dev_memory(device, &free, &total);
    result.free = static_cast<std::int64_t>(free);
    result.total = static_cast<std::int64_t>(total);
    return result;
}

StageMemoryEntry host_entry() {
    StageMemoryEntry result;
    result.scope = "host";
    result.name = "host";
    result.description = "host memory";
    std::uint64_t free = 0;
    std::uint64_t total = 0;
#ifdef _WIN32
    MEMORYSTATUSEX status{};
    status.dwLength = sizeof(status);
    if (GlobalMemoryStatusEx(&status)) {
        free = status.ullAvailPhys;
        total = status.ullTotalPhys;
    }
#elif defined(__APPLE__)
    std::uint64_t physical = 0;
    std::size_t physical_size = sizeof(physical);
    vm_statistics64_data_t statistics{};
    mach_msg_type_number_t statistics_size = HOST_VM_INFO64_COUNT;
    vm_size_t page_size = 0;
    if (sysctlbyname("hw.memsize", &physical, &physical_size, nullptr, 0) == 0 &&
        host_page_size(mach_host_self(), &page_size) == KERN_SUCCESS &&
        host_statistics64(mach_host_self(), HOST_VM_INFO64,
            reinterpret_cast<host_info64_t>(&statistics), &statistics_size) == KERN_SUCCESS) {
        total = physical;
        free = static_cast<std::uint64_t>(page_size) *
            (statistics.free_count + statistics.inactive_count + statistics.speculative_count);
    }
#elif defined(__linux__)
    struct sysinfo status{};
    if (sysinfo(&status) == 0) {
        const auto unit = static_cast<std::uint64_t>(status.mem_unit);
        total = static_cast<std::uint64_t>(status.totalram) * unit;
        // free + buffer is deliberately conservative. Unlike llama.cpp's
        // CPU backend Unix fallback, it never labels all physical RAM free.
        free = (static_cast<std::uint64_t>(status.freeram) +
                static_cast<std::uint64_t>(status.bufferram)) * unit;
    }
#endif
    if (total == 0) {
        auto * cpu = ggml_backend_dev_by_type(GGML_BACKEND_DEVICE_TYPE_CPU);
        if (cpu != nullptr) {
            std::size_t backend_free = 0;
            std::size_t backend_total = 0;
            ggml_backend_dev_memory(cpu, &backend_free, &backend_total);
            free = backend_free;
            total = backend_total;
        }
    }
    result.free = static_cast<std::int64_t>(free);
    result.total = static_cast<std::int64_t>(total);
    return result;
}

bool individual_current_free_fits(const StageMemoryPlan & plan) {
    return std::all_of(plan.entries.begin(), plan.entries.end(), [](const auto & entry) {
        return entry.free >= 0 && entry.required() <= static_cast<std::uint64_t>(entry.free);
    });
}

bool valid_memory_topology(
        const staged::llama_runtime::MemoryTopology & topology,
        std::size_t device_count,
        std::string * error) {
    using staged::llama_runtime::MemoryTopologyKind;
    if (topology.kind == MemoryTopologyKind::Discrete) {
        if (topology.host_shared_devices.empty()) return true;
    } else if (topology.kind == MemoryTopologyKind::HostShared &&
               !topology.host_shared_devices.empty() &&
               std::set<std::int32_t>(topology.host_shared_devices.begin(),
                                      topology.host_shared_devices.end()).size() ==
                   topology.host_shared_devices.size() &&
               std::all_of(topology.host_shared_devices.begin(),
                           topology.host_shared_devices.end(),
                           [device_count](std::int32_t index) {
                               return index >= 0 &&
                                   static_cast<std::size_t>(index) < device_count;
                           })) {
        return true;
    }
    if (error != nullptr) {
        *error = "memory topology is missing or references an unavailable backend device";
    }
    return false;
}

bool add_breakdown(
        const llama_model * model,
        const llama_context * context,
        bool include_model,
        StageMemoryPlan * result,
        std::string * error) {
    const auto breakdown = p4_llama_compat::memory_breakdown(context);
    const std::size_t device_count = p4_llama_compat::model_device_count(model);
    for (const auto & measured : breakdown) {
        StageMemoryEntry * entry = &result->entries.back();
        if (!staged::llama_runtime::stage_buffer_uses_host_memory(measured.buffer_type)) {
            auto * device = ggml_backend_buft_get_device(measured.buffer_type);
            entry = nullptr;
            for (std::size_t index = 0; index < device_count; ++index) {
                if (device == p4_llama_compat::model_device(model, index)) {
                    entry = &result->entries[index];
                    break;
                }
            }
            if (entry == nullptr) {
                if (error != nullptr) {
                    *error = "memory breakdown referenced an unknown backend device: buffer=";
                    *error += ggml_backend_buft_name(measured.buffer_type);
                    *error += " device=";
                    *error += device != nullptr ? ggml_backend_dev_name(device) : "none";
                }
                return false;
            }
        }
        if (include_model) entry->model += measured.model;
        entry->context += measured.context;
        entry->compute += measured.compute;
    }
    return true;
}

std::string json_escape(const std::string & value) {
    std::ostringstream out;
    for (const unsigned char ch : value) {
        switch (ch) {
            case '\\': out << "\\\\"; break;
            case '"': out << "\\\""; break;
            case '\n': out << "\\n"; break;
            case '\r': out << "\\r"; break;
            case '\t': out << "\\t"; break;
            default:
                if (ch < 0x20) {
                    constexpr char hex[] = "0123456789abcdef";
                    out << "\\u00" << hex[ch >> 4] << hex[ch & 0x0f];
                } else {
                    out << static_cast<char>(ch);
                }
        }
    }
    return out.str();
}

} // namespace

namespace staged::llama_runtime {

bool stage_buffer_uses_host_memory(ggml_backend_buffer_type_t buffer_type) {
    if (ggml_backend_buft_is_host(buffer_type)) return true;
    const auto device = ggml_backend_buft_get_device(buffer_type);
    return device != nullptr && ggml_backend_dev_type(device) == GGML_BACKEND_DEVICE_TYPE_CPU;
}

llama_model_params make_stage_model_params(
        p4_llama_compat::LlamaPlan & params,
        const LoadConfig & config) {
    params.set_model_path(config.model_path);
    auto result = params.to_model_params();
    result.linkcpp_layer_begin = config.layer_begin;
    result.linkcpp_layer_end = config.layer_end;
    result.linkcpp_kv_gpu_layer_start = config.kv_gpu_layer_start;
    result.linkcpp_kv_gpu_layer_end = config.kv_gpu_layer_end;
    return result;
}

llama_context_params make_stage_context_params(const p4_llama_compat::LlamaPlan & params) {
    return params.to_context_params();
}

bool inspect_stage_memory_with_initialized_backend(
        const p4_llama_compat::LlamaPlan & plan,
        const LoadConfig & config,
        StageMemoryPlan * result,
        std::string * error) {
    // A measurement pass rewrites the model path, so it works on its own copy.
    p4_llama_compat::LlamaPlan params = plan.clone();
    if (result == nullptr || config.model_path.empty() || config.layer_begin < 0 ||
        config.layer_end <= config.layer_begin) {
        if (error != nullptr) *error = "invalid stage memory-plan configuration";
        return false;
    }
    try {
        auto model_params = make_stage_model_params(params, config);
        model_params.no_alloc = true;
        model_params.load_mode = LLAMA_LOAD_MODE_NONE;
        llama_model_ptr model(llama_model_load_from_file(
            config.model_path.c_str(), model_params));
        if (model == nullptr) {
            if (error != nullptr) *error = "llama.cpp failed to create the no-alloc model plan";
            return false;
        }
        const auto context_params = make_stage_context_params(params);
        llama_context_ptr context(llama_init_from_model(model.get(), context_params));
        if (context == nullptr) {
            if (error != nullptr) *error = "llama.cpp failed to create the no-alloc context plan";
            return false;
        }
        llama_context_ptr speculative_context;
        const bool mtp_tail = has_mtp(params) &&
            config.layer_end >= llama_model_n_layer(model.get());
        if (mtp_tail) {
            if (llama_model_n_layer_nextn(model.get()) <= 0) {
                if (error != nullptr) {
                    *error = "draft-mtp requested but the model reports zero NextN/MTP layers";
                }
                return false;
            }
            auto mtp_params = params.speculative_plan();
            auto mtp_context_params = make_stage_context_params(mtp_params);
            mtp_context_params.ctx_type = LLAMA_CONTEXT_TYPE_MTP;
            mtp_context_params.n_rs_seq = 0;
            mtp_context_params.ctx_other = context.get();
            speculative_context.reset(llama_init_from_model(
                model.get(), mtp_context_params));
            if (speculative_context == nullptr) {
                if (error != nullptr) *error = "llama.cpp failed to create the no-alloc MTP context plan";
                return false;
            }
        }
        return measure_stage_memory(
            model.get(), context.get(), speculative_context.get(),
            config.memory_topology, params.kv_unified(), result, error) &&
            measure_stage_layer_devices(model.get(), config, result, error);
    } catch (const std::exception & exception) {
        if (error != nullptr) *error = std::string("llama.cpp memory planning failed: ") + exception.what();
        return false;
    }
}

bool inspect_stage_memory(
        const p4_llama_compat::LlamaPlan & plan,
        const LoadConfig & config,
        StageMemoryPlan * result,
        std::string * error) {
    llama_backend_init();
    ggml_backend_load_all();
    const bool ok = inspect_stage_memory_with_initialized_backend(
        plan, config, result, error);
    llama_backend_free();
    return ok;
}

bool measure_stage_memory(
        const llama_model * model,
        const llama_context * context,
        const llama_context * speculative_context,
        const MemoryTopology & memory_topology,
        bool kv_unified,
        StageMemoryPlan * result,
        std::string * error) {
    if (model == nullptr || context == nullptr || result == nullptr) {
        if (error != nullptr) *error = "cannot measure an unloaded stage";
        return false;
    }
    const std::size_t device_count = p4_llama_compat::model_device_count(model);
    if (!valid_memory_topology(memory_topology, device_count, error)) return false;
    result->memory_topology = memory_topology;
    result->execution_shape = StageExecutionShape{
        llama_n_ctx(context), llama_n_ctx_seq(context), llama_n_batch(context),
        llama_n_ubatch(context), llama_n_seq_max(context), kv_unified};
    result->entries.clear();
    result->entries.reserve(device_count + 1);
    for (std::size_t index = 0; index < device_count; ++index) {
        auto * device = p4_llama_compat::model_device(model, index);
        result->entries.push_back(device_entry(device, index));
    }
    result->entries.push_back(host_entry());
    if (!add_breakdown(model, context, true, result, error)) return false;
    if (speculative_context != nullptr &&
        !add_breakdown(model, speculative_context, false, result, error)) return false;
    result->complete = true;
    result->fits_current_free = stage_memory_plan_fits_current_free(*result);
    return true;
}

bool stage_memory_plan_fits_current_free(const StageMemoryPlan & plan) {
    if (!individual_current_free_fits(plan)) return false;
    if (plan.memory_topology.kind == MemoryTopologyKind::Discrete) return true;
    if (plan.memory_topology.kind != MemoryTopologyKind::HostShared) return false;
    const auto host = std::find_if(plan.entries.begin(), plan.entries.end(),
        [](const StageMemoryEntry & entry) { return entry.scope == "host"; });
    if (host == plan.entries.end() || host->free < 0) return false;
    std::uint64_t shared_required = host->required();
    for (const auto device_index : plan.memory_topology.host_shared_devices) {
        const auto device = std::find_if(plan.entries.begin(), plan.entries.end(),
            [device_index](const StageMemoryEntry & entry) {
                return entry.scope == "device" && entry.index == device_index;
            });
        if (device == plan.entries.end() ||
            device->required() > std::numeric_limits<std::uint64_t>::max() - shared_required) {
            return false;
        }
        shared_required += device->required();
    }
    return shared_required <= static_cast<std::uint64_t>(host->free);
}

bool same_stage_memory_allocation(
        const StageMemoryPlan & planned,
        const StageMemoryPlan & actual,
        std::string * error) {
    if (planned.layer_device_query_supported != actual.layer_device_query_supported ||
        planned.layer_device_expectations_checked != actual.layer_device_expectations_checked ||
        planned.layer_default_devices != actual.layer_default_devices) {
        if (error) *error = "planned and actual default layer devices differ";
        return false;
    }
    if (planned.entries.size() != actual.entries.size()) {
        if (error != nullptr) *error = "planned and actual memory device counts differ";
        return false;
    }
    if (!(planned.memory_topology == actual.memory_topology)) {
        if (error != nullptr) *error = "planned and actual memory topologies differ";
        return false;
    }
    if (!(planned.execution_shape == actual.execution_shape)) {
        if (error != nullptr) *error = "planned and actual execution shapes differ";
        return false;
    }
    for (std::size_t index = 0; index < planned.entries.size(); ++index) {
        const auto & lhs = planned.entries[index];
        const auto & rhs = actual.entries[index];
        if (lhs.scope != rhs.scope || lhs.name != rhs.name || lhs.model != rhs.model ||
            lhs.context != rhs.context || lhs.compute != rhs.compute) {
            if (error != nullptr) {
                *error = "planned and actual memory differ at entry " + std::to_string(index);
            }
            return false;
        }
    }
    return true;
}

std::string serialize_stage_memory_plan(const StageMemoryPlan & plan) {
    std::ostringstream out;
    const char * topology = plan.memory_topology.kind == MemoryTopologyKind::Discrete
        ? "discrete"
        : (plan.memory_topology.kind == MemoryTopologyKind::HostShared
               ? "host-shared" : "unspecified");
    out << "{\"schema\":2,\"memory_topology\":{\"mode\":\"" << topology
        << "\",\"host_shared_devices\":[";
    for (std::size_t index = 0;
         index < plan.memory_topology.host_shared_devices.size(); ++index) {
        if (index != 0) out << ',';
        out << plan.memory_topology.host_shared_devices[index];
    }
    out << "]},\"execution_shape\":{\"n_ctx\":" << plan.execution_shape.n_ctx
        << ",\"n_ctx_seq\":" << plan.execution_shape.n_ctx_seq
        << ",\"n_batch\":" << plan.execution_shape.n_batch
        << ",\"n_ubatch\":" << plan.execution_shape.n_ubatch
        << ",\"n_seq_max\":" << plan.execution_shape.n_seq_max
        << ",\"kv_unified\":" << (plan.execution_shape.kv_unified ? "true" : "false")
        << "},\"complete\":" << (plan.complete ? "true" : "false")
        << ",\"fits_current_free\":" << (plan.fits_current_free ? "true" : "false")
        << ",\"layer_device_query_supported\":" << (plan.layer_device_query_supported ? "true" : "false")
        << ",\"layer_device_expectations_checked\":" << (plan.layer_device_expectations_checked ? "true" : "false")
        << ",\"layer_default_devices\":[";
    for (std::size_t i = 0; i < plan.layer_default_devices.size(); ++i) {
        if (i) out << ',';
        const auto & layer = plan.layer_default_devices[i];
        out << "{\"layer\":" << layer.layer << ",\"device\":\"" << json_escape(layer.device) << "\"}";
    }
    out << "],\"entries\":[";
    for (std::size_t index = 0; index < plan.entries.size(); ++index) {
        if (index != 0) out << ',';
        const auto & entry = plan.entries[index];
        out << "{\"scope\":\"" << json_escape(entry.scope)
            << "\",\"index\":" << entry.index
            << ",\"name\":\"" << json_escape(entry.name)
            << "\",\"description\":\"" << json_escape(entry.description)
            << "\",\"free\":" << entry.free
            << ",\"total\":" << entry.total
            << ",\"model\":" << entry.model
            << ",\"context\":" << entry.context
            << ",\"compute\":" << entry.compute
            << ",\"required\":" << entry.required() << '}';
    }
    out << "]}";
    return out.str();
}

} // namespace staged::llama_runtime
