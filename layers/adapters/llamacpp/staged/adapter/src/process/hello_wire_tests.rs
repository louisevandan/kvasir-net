//! The HELLO string is a wire format, so it is tested as one.
//!
//! The negative tests elsewhere build a `ReadyInfo` by hand and compare the
//! structs. That proves the comparison and nothing about the parse - which is
//! where the defect was: a value containing the field separator reached the
//! adapter truncated, and the run reported a backend inventory of `CUDA[CUDA0]`
//! for a stage that had said `CUDA[CUDA0];CPU[CPU]`.

use super::core::decode_hello;

fn hello(capabilities: &str) -> Vec<u8> {
    let revision = super::super::PROTOCOL_REVISION;
    let mut body = vec![revision as u8, (revision >> 8) as u8];
    body.extend_from_slice(capabilities.as_bytes());
    body
}

const BASE: &str = "READY;llama_runtime=1;hop=1;kv=1;transactions=1;physical_batch=1\
;n_ctx=2048;n_batch=512;n_ubatch=512;n_seq_max=4;physical_result_payload_bytes=1048576\
;physical_result_tensor_count=2;max_physical_result_bytes=33554432;max_atomic_sequences=4";

#[test]
fn physical_identity_revision_is_never_inferred_from_physical_batch() {
    assert_eq!(
        decode_hello(&hello(BASE))
            .unwrap()
            .physical_identity_revision,
        0
    );
    assert_eq!(
        decode_hello(&hello(&format!("{BASE};physical_identity_revision=1")))
            .unwrap()
            .physical_identity_revision,
        1
    );
    assert!(
        decode_hello(&hello(&format!(
            "{BASE};physical_identity_revision=invalid"
        )))
        .is_err()
    );
}

#[test]
fn physical_result_bound_is_required_and_parsed_exactly() {
    let ready = decode_hello(&hello(BASE)).unwrap();
    assert_eq!(ready.physical_result_payload_bytes, 1_048_576);
    assert_eq!(ready.physical_result_tensor_count, 2);
    assert_eq!(ready.max_physical_result_bytes, 33_554_432);
    assert!(
        decode_hello(&hello(
            &BASE.replace(";max_physical_result_bytes=33554432", "")
        ))
        .is_err()
    );
    assert!(
        decode_hello(&hello(
            "READY;n_ctx=1;n_batch=1;n_ubatch=1;n_seq_max=1;max_atomic_sequences=1"
        ))
        .is_err()
    );
}

#[test]
fn a_multi_registry_inventory_survives_the_wire() {
    // The 2026-09-02 defect, in the form the stage server actually sent it.
    let text = format!(
        "{BASE};upstream=557614e02;patch_set=00e66c6b;backend_inventory=CPU[CPU]|CUDA[CUDA0]"
    );
    let ready = decode_hello(&hello(&text)).expect("decode");
    assert_eq!(ready.backend_inventory, "CPU[CPU]|CUDA[CUDA0]");
}

#[test]
fn a_field_after_the_inventory_is_still_its_own_field() {
    let text = format!(
        "{BASE};backend_inventory=CPU[CPU]|CUDA[CUDA0,CUDA1];upstream=557614e02;patch_set=00e66c6b"
    );
    let ready = decode_hello(&hello(&text)).expect("decode");
    assert_eq!(ready.backend_inventory, "CPU[CPU]|CUDA[CUDA0,CUDA1]");
    assert_eq!(ready.upstream_commit, "557614e02");
    assert_eq!(ready.patch_set, "00e66c6b");
}

#[test]
fn an_escaped_separator_does_not_split_the_value() {
    // A backend whose name contains the field separator is escaped by the
    // server; the adapter must read it as one value rather than two fields.
    let text = format!("{BASE};backend_inventory=od%3Bd[dev];upstream=x;patch_set=y");
    let ready = decode_hello(&hello(&text)).expect("decode");
    assert_eq!(ready.backend_inventory, "od%3Bd[dev]");
    assert_eq!(ready.upstream_commit, "x");
}

#[test]
fn a_stage_that_does_not_report_an_inventory_reads_as_unknown() {
    let text = format!("{BASE};upstream=557614e02;patch_set=00e66c6b");
    let ready = decode_hello(&hello(&text)).expect("decode");
    assert_eq!(ready.backend_inventory, "unknown");
}

#[test]
fn stage_wire_identity_is_read_verbatim_and_never_inferred_from_the_backend() {
    let abi = format!("p4pb4le64:{}:types=0/1/4,1/1/2,2/32/18", "a".repeat(64));
    let ready = decode_hello(&hello(&format!(
        "{BASE};backend_inventory=MTL[MTL0];stage_wire_abi={abi};upstream=pin"
    )))
    .unwrap();
    assert_eq!(ready.stage_wire_abi, abi);
    assert_eq!(ready.upstream_commit, "pin");
    assert_eq!(
        decode_hello(&hello(BASE)).unwrap().stage_wire_abi,
        "unknown"
    );
}
