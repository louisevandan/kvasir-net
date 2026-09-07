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
;n_ctx=2048;n_batch=512;n_ubatch=512;n_seq_max=4;max_atomic_sequences=4";

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
