#include "ggml.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"

#include <cassert>
#include <cmath>
#include <cstdio>
#include <vector>

static void run_case(ggml_backend_t gpu, ggml_backend_t cpu, int count) {
    constexpr int width = 128;
    const ggml_init_params params = {4 * 1024 * 1024, nullptr, true};
    auto * weights = ggml_init(params);
    auto * context = ggml_init(params);
    assert(weights && context);
    std::vector<ggml_tensor *> inputs;
    for (int i = 0; i < count; ++i) {
        inputs.push_back(ggml_new_tensor_1d(weights, GGML_TYPE_F32, width));
        ggml_format_name(inputs.back(), "input_%d", i);
    }
    auto buffer = ggml_backend_alloc_ctx_tensors(weights, cpu);
    assert(buffer);
    assert(!ggml_backend_dev_supports_buft(ggml_backend_get_device(gpu),
                                         ggml_backend_buffer_get_type(buffer)));
    for (int i = 0; i < count; ++i) {
        std::vector<float> values(width, float(i + 1));
        ggml_backend_tensor_set(inputs[i], values.data(), 0, width * sizeof(float));
    }
    ggml_backend_t backends[] = {gpu, cpu};
    auto sched = ggml_backend_sched_new(backends, nullptr, 2, 1024, false, true);
    assert(sched);
    auto add = [&](ggml_tensor * left, ggml_tensor * right) {
        auto * node = ggml_add(context, left, right);
        ggml_backend_sched_set_tensor_backend(sched, node, gpu);
        return node;
    };
    auto * sum = inputs[0];
    for (int i = 1; i < count - 2; ++i) sum = add(sum, inputs[i]);
    // At count=31/61 one operation crosses the old 30/60 input capacity with two new sources.
    auto * pair = add(inputs[count - 2], inputs[count - 1]);
    sum = add(sum, pair);
    auto * graph = ggml_new_graph_custom(context, 1024, false);
    ggml_build_forward_expand(graph, sum);
    assert(ggml_backend_sched_alloc_graph(sched, graph));
    assert(ggml_backend_sched_graph_compute(sched, graph) == GGML_STATUS_SUCCESS);
    assert(ggml_backend_sched_get_n_splits(sched) == 1);
    assert(ggml_backend_sched_get_tensor_backend(sched, sum) == gpu);
    std::vector<float> actual(width);
    ggml_backend_tensor_get(sum, actual.data(), 0, width * sizeof(float));
    const float expected = float(count * (count + 1) / 2);
    for (float value : actual) assert(std::isfinite(value) && value == expected);
    for (int i = 0; i < count; ++i) {
        ggml_backend_tensor_get(inputs[i], actual.data(), 0, width * sizeof(float));
        for (float value : actual) assert(value == float(i + 1));
    }
    std::printf("PASS inputs=%d backend=%s splits=1 expected=%g\n", count,
                ggml_backend_name(gpu), double(expected));
    ggml_backend_sched_free(sched);
    ggml_backend_buffer_free(buffer);
    ggml_free(context);
    ggml_free(weights);
}

int main() {
    ggml_backend_load_all();
    auto gpu = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_GPU, nullptr);
    if (!gpu) gpu = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_IGPU, nullptr);
    auto cpu = ggml_backend_init_by_type(GGML_BACKEND_DEVICE_TYPE_CPU, nullptr);
    if (!gpu || !cpu) {
        std::fprintf(stderr, "FAIL: this conformance test requires a GPU and CPU backend\n");
        return 2;
    }
    for (int count : {29, 30, 31, 59, 60, 61}) run_case(gpu, cpu, count);
    ggml_backend_free(gpu);
    ggml_backend_free(cpu);
}
