#include "ggml.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"

#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <iostream>
#include <vector>

void run_ggml_reserve_size_tests() {
    ggml_backend_load_all();
    auto * cpu = ggml_backend_dev_by_type(GGML_BACKEND_DEVICE_TYPE_CPU);
    assert(cpu != nullptr);
    auto * backend = ggml_backend_dev_init(cpu, nullptr);
    assert(backend != nullptr);
    for (const int copies : {1, 2, 3}) {
        const ggml_init_params params{32 * ggml_tensor_overhead() + ggml_graph_overhead_custom(32, false), nullptr, true};
        auto * context = ggml_init(params);
        assert(context != nullptr);
        auto * left = ggml_new_tensor_1d(context, GGML_TYPE_F32, 257);
        auto * right = ggml_new_tensor_1d(context, GGML_TYPE_F32, 257);
        ggml_set_input(left);
        ggml_set_input(right);
        auto * sum = ggml_add(context, left, right);
        ggml_set_output(sum);
        auto * graph = ggml_new_graph_custom(context, 32, false);
        ggml_build_forward_expand(graph, sum);
        std::vector<ggml_backend_buffer_type_t> types(copies, ggml_backend_dev_buffer_type(cpu));
        auto * allocator = ggml_gallocr_new_n(types.data(), copies);
        std::vector<int> nodes(ggml_graph_n_nodes(graph), copies - 1);
        // This graph has the two explicitly created input leaves.
        const std::vector<int> leaves{0, copies - 1};
        std::vector<size_t> measured(copies, 0);
        ggml_gallocr_reserve_n_size(allocator, graph, nodes.data(), leaves.data(), measured.data());
        assert(left->data == nullptr && right->data == nullptr && sum->data == nullptr);
        for (int i = 0; i < copies; ++i) assert(ggml_gallocr_get_buffer_size(allocator, i) == 0);
        assert(ggml_gallocr_reserve_n(allocator, graph, nodes.data(), leaves.data()));
        for (int i = 0; i < copies; ++i) {
            const auto actual = ggml_gallocr_get_buffer_size(allocator, i);
            std::cout << "RESERVE_SIZE copies=" << copies << " index=" << i
                      << " planned=" << measured[i] << " actual=" << actual << std::endl;
            assert(measured[i] == actual);
            assert((actual > 0) == (i == 0));
        }
        assert(ggml_gallocr_alloc_graph(allocator, graph));
        std::vector<float> a(257), b(257, 2.0f), output(257);
        for (int i = 0; i < 257; ++i) a[i] = static_cast<float>(i);
        ggml_backend_tensor_set(left, a.data(), 0, a.size() * sizeof(float));
        ggml_backend_tensor_set(right, b.data(), 0, b.size() * sizeof(float));
        assert(ggml_backend_graph_compute(backend, graph) == GGML_STATUS_SUCCESS);
        ggml_backend_tensor_get(sum, output.data(), 0, output.size() * sizeof(float));
        for (int i = 0; i < 257; ++i) assert(output[i] == static_cast<float>(i + 2));
        ggml_gallocr_free(allocator);
        ggml_free(context);
    }
    ggml_backend_free(backend);
}
