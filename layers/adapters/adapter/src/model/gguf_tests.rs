use super::*;
use std::path::PathBuf;

fn u32(value: u32, out: &mut Vec<u8>) {
    out.extend(value.to_le_bytes());
}
fn u64(value: u64, out: &mut Vec<u8>) {
    out.extend(value.to_le_bytes());
}
fn text(value: &str, out: &mut Vec<u8>) {
    u64(value.len() as u64, out);
    out.extend(value.as_bytes());
}
fn emit_metadata(key: &str, kind: u32, value: &[u8], out: &mut Vec<u8>) {
    text(key, out);
    u32(kind, out);
    out.extend(value);
}

#[test]
fn parses_model_shape_and_tensor_inventory_without_model_data() {
    let mut bytes = b"GGUF".to_vec();
    u32(3, &mut bytes);
    u64(2, &mut bytes);
    u64(6, &mut bytes);
    let mut architecture = Vec::new();
    text("llama", &mut architecture);
    emit_metadata("general.architecture", 8, &architecture, &mut bytes);
    emit_metadata("general.alignment", 4, &32_u32.to_le_bytes(), &mut bytes);
    emit_metadata("llama.block_count", 4, &2_u32.to_le_bytes(), &mut bytes);
    emit_metadata(
        "llama.embedding_length",
        4,
        &4096_u32.to_le_bytes(),
        &mut bytes,
    );
    emit_metadata(
        "llama.attention.head_count",
        4,
        &32_u32.to_le_bytes(),
        &mut bytes,
    );
    emit_metadata(
        "llama.context_length",
        4,
        &8192_u32.to_le_bytes(),
        &mut bytes,
    );
    text("blk.0.attn_q", &mut bytes);
    u32(1, &mut bytes);
    u64(32, &mut bytes);
    u32(0, &mut bytes);
    u64(0, &mut bytes);
    text("output", &mut bytes);
    u32(1, &mut bytes);
    u64(32, &mut bytes);
    u32(1, &mut bytes);
    u64(128, &mut bytes);
    let parsed = parse(Path::new("fixture.gguf"), &bytes, 1024).expect("valid fixture");
    assert_eq!(
        metadata(&parsed.metadata, "general.architecture").as_deref(),
        Some("llama")
    );
    assert_eq!(metadata_u64(&parsed.metadata, "llama.block_count"), Some(2));
    assert_eq!(parsed.tensors.len(), 2);
    assert_eq!(parsed.tensors[0].bytes, Some(128));
    assert!(parsed.tensors[1].bytes.is_some());
}

#[test]
fn refuses_non_gguf_and_truncated_headers() {
    assert!(parse(Path::new("bad"), b"not-gguf", 8).is_err());
    assert!(parse(Path::new("short"), b"GGUF", 4).is_err());
}

#[test]
fn inspects_a_real_model_when_the_test_root_is_configured() {
    let Some(root) = std::env::var_os("P4_MODEL_DIR") else {
        return;
    };
    let root = PathBuf::from(root);
    let Some(file) = std::fs::read_dir(root).ok().and_then(|entries| {
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
            })
    }) else {
        return;
    };
    let name = file
        .file_name()
        .and_then(|value| value.to_str())
        .expect("UTF-8 model name");
    let profile: serde_json::Value =
        serde_json::from_str(&inspect_artifact(name).expect("real GGUF profile"))
            .expect("profile JSON");
    assert_eq!(profile["schema"], 1);
    assert!(
        profile["fingerprint"]
            .as_str()
            .is_some_and(|value| value.starts_with("fnv1a64-"))
    );
    assert!(
        profile["tensors"]
            .as_array()
            .is_some_and(|values| !values.is_empty())
    );
}
