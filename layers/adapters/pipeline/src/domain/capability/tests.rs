use super::*;

#[test]
fn extracts_the_native_pipeline_contract_without_exposing_host_specific_paths() {
    let value = descriptor_from_identity(&json!({
        "pipeline": { "protocol": "linker-stage-v1", "adapter_abi": 16, "capability_bits": 8081, "build_id": "build-a" },
        "runtime_contract": { "name": "linker-pipeline-runtime", "major": 1, "revision": 1 }
    }));
    assert_eq!(value["available"], true);
    assert_eq!(value["protocol"], "linker-stage-v1");
    assert_eq!(value["adapter_abi"], 16);
    assert_eq!(value["capability_bits"], 8081);
    assert_eq!(value["build_id"], "build-a");
}

#[test]
fn rejects_a_runtime_identity_without_a_complete_pipeline_contract() {
    let value = descriptor_from_identity(&json!({ "pipeline": { "protocol": "linker-stage-v1" } }));
    assert_eq!(value["available"], false);
}
