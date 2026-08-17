use super::profile::{FileProfile, ModelProfile, TensorProfile};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_HEADER_BYTES: usize = 256 * 1024 * 1024;

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or("GGUF offset overflow")?;
        if end > self.bytes.len() {
            return Err("truncated GGUF header".into());
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }
    fn string(&mut self) -> Result<String, String> {
        let length = usize::try_from(self.u64()?).map_err(|_| "GGUF string is too large")?;
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| "GGUF string is not UTF-8".into())
    }
}

#[derive(Default)]
struct Parsed {
    version: u32,
    metadata: Vec<(String, String)>,
    tensors: Vec<TensorProfile>,
    data_offset: u64,
    file_size: u64,
}

pub fn inspect_artifact(reference: &str) -> Result<String, String> {
    let paths = resolve_paths(reference)?;
    if paths.is_empty() {
        return Err("model artifact contains no GGUF files".into());
    }
    let mut parsed = Vec::with_capacity(paths.len());
    let mut files = Vec::with_capacity(paths.len());
    for path in &paths {
        let file_size = fs::metadata(path)
            .map_err(|e| format!("cannot stat {}: {e}", path.display()))?
            .len();
        let bytes = read_header(path)?;
        let item = parse(path, &bytes, file_size)?;
        files.push(FileProfile {
            name: path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("artifact")
                .to_owned(),
            bytes: item.file_size,
            complete: item.tensors.iter().all(|tensor| tensor.bytes.is_some()),
        });
        parsed.push(item);
    }
    let first = parsed
        .first()
        .ok_or("model artifact contains no GGUF files")?;
    let architecture = metadata(&first.metadata, "general.architecture");
    let prefix = architecture.as_deref().map(|v| format!("{v}."));
    let key = |suffix: &str| prefix.as_deref().map(|p| format!("{p}{suffix}"));
    let all_tensors: Vec<TensorProfile> = parsed.iter().flat_map(|v| v.tensors.clone()).collect();
    let mut layer_bytes = vec![
        0_u64;
        key("block_count")
            .and_then(|k| metadata_u64(&first.metadata, &k))
            .unwrap_or(0) as usize
    ];
    let mut boundary_bytes = 0_u64;
    for tensor in &all_tensors {
        let bytes = tensor.bytes.unwrap_or(0);
        if let Some(layer) = tensor
            .name
            .strip_prefix("blk.")
            .and_then(|v| v.split('.').next())
            .and_then(|v| v.parse::<usize>().ok())
        {
            if let Some(slot) = layer_bytes.get_mut(layer) {
                *slot = slot.saturating_add(bytes);
            } else {
                boundary_bytes = boundary_bytes.saturating_add(bytes);
            }
        } else {
            boundary_bytes = boundary_bytes.saturating_add(bytes);
        }
    }
    let fingerprint = fingerprint(&parsed, &files, &all_tensors);
    let profile = ModelProfile {
        schema: 1,
        artifact: reference.to_owned(),
        fingerprint,
        files,
        architecture,
        layers: key("block_count").and_then(|k| metadata_u64(&first.metadata, &k)),
        embedding: key("embedding_length").and_then(|k| metadata_u64(&first.metadata, &k)),
        attention_heads: key("attention.head_count")
            .and_then(|k| metadata_u64(&first.metadata, &k)),
        attention_heads_kv: key("attention.head_count_kv")
            .and_then(|k| metadata_u64(&first.metadata, &k)),
        context: key("context_length").and_then(|k| metadata_u64(&first.metadata, &k)),
        expert_count: key("expert_count").and_then(|k| metadata_u64(&first.metadata, &k)),
        tensors: all_tensors,
        layer_bytes,
        boundary_bytes,
    };
    serde_json::to_string(&profile).map_err(|e| format!("cannot encode model profile: {e}"))
}

fn resolve_paths(reference: &str) -> Result<Vec<PathBuf>, String> {
    let root = std::env::var_os("P4_MODEL_DIR")
        .or_else(|| std::env::var_os("LLAMA_MODEL_DIR"))
        .ok_or("P4_MODEL_DIR or LLAMA_MODEL_DIR is not configured")?;
    let root = fs::canonicalize(root).map_err(|e| format!("model root is unavailable: {e}"))?;
    let requested = Path::new(reference);
    if requested.is_absolute() || reference.contains("..") {
        return Err("artifact must be a relative model reference".into());
    }
    let candidate = root.join(requested);
    let metadata = fs::metadata(&candidate).map_err(|e| format!("artifact is unavailable: {e}"))?;
    let mut paths = if metadata.is_dir() {
        let mut values = fs::read_dir(&candidate)
            .map_err(|e| format!("cannot enumerate artifact: {e}"))?
            .filter_map(Result::ok)
            .map(|v| v.path())
            .filter(|v| {
                v.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
            })
            .collect::<Vec<_>>();
        values.sort();
        values
    } else {
        vec![candidate]
    };
    for path in &mut paths {
        let canonical =
            fs::canonicalize(&*path).map_err(|e| format!("artifact is unavailable: {e}"))?;
        if !canonical.starts_with(&root) {
            return Err("artifact escapes model root".into());
        }
        *path = canonical;
    }
    Ok(paths)
}

fn read_header(path: &Path) -> Result<Vec<u8>, String> {
    let mut file =
        fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let length = fs::metadata(path)
        .map_err(|e| format!("cannot stat {}: {e}", path.display()))?
        .len()
        .min(MAX_HEADER_BYTES as u64) as usize;
    let mut bytes = vec![0_u8; length];
    let length = file
        .read(&mut bytes)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    bytes.truncate(length);
    Ok(bytes)
}

fn parse(path: &Path, bytes: &[u8], file_size: u64) -> Result<Parsed, String> {
    let mut cursor = Cursor::new(bytes);
    if cursor.take(4)? != b"GGUF" {
        return Err(format!("{} is not GGUF", path.display()));
    }
    let version = cursor.u32()?;
    if version != 2 && version != 3 {
        return Err(format!("unsupported GGUF version {version}"));
    }
    let tensor_count = usize::try_from(cursor.u64()?).map_err(|_| "too many GGUF tensors")?;
    let metadata_count = usize::try_from(cursor.u64()?).map_err(|_| "too much GGUF metadata")?;
    let mut metadata = Vec::with_capacity(metadata_count.min(64));
    for _ in 0..metadata_count {
        let key = cursor.string()?;
        let value_type = cursor.u32()?;
        let value = read_value(&mut cursor, value_type)?;
        if retain(&key) {
            metadata.push((key, value));
        }
    }
    let tensor_start = cursor.offset;
    let mut tensors = Vec::with_capacity(tensor_count);
    for _ in 0..tensor_count {
        let name = cursor.string()?;
        let dimensions = (0..cursor.u32()?)
            .map(|_| cursor.u64())
            .collect::<Result<Vec<_>, _>>()?;
        let dtype = cursor.u32()?;
        let offset = cursor.u64()?;
        tensors.push(TensorProfile {
            name,
            dimensions,
            dtype,
            offset,
            bytes: None,
        });
    }
    let alignment = metadata_u64(&metadata, "general.alignment").unwrap_or(32);
    let data_offset = align(
        u64::try_from(cursor.offset).map_err(|_| "GGUF header is too large")?,
        alignment,
    );
    if data_offset > file_size {
        return Err("GGUF tensor data starts beyond the file".into());
    }
    let mut tensors = tensors;
    for index in 0..tensors.len() {
        let next = tensors
            .get(index + 1)
            .map(|v| v.offset)
            .unwrap_or_else(|| file_size.saturating_sub(data_offset));
        let bytes = next.checked_sub(tensors[index].offset);
        if let Some(length) = bytes {
            if data_offset
                .saturating_add(tensors[index].offset)
                .saturating_add(length)
                > file_size
            {
                return Err(format!(
                    "tensor {} exceeds the GGUF file",
                    tensors[index].name
                ));
            }
        }
        tensors[index].bytes = bytes;
    }
    let _ = tensor_start;
    Ok(Parsed {
        version,
        metadata,
        tensors,
        data_offset,
        file_size,
    })
}

fn read_value(cursor: &mut Cursor<'_>, kind: u32) -> Result<String, String> {
    match kind {
        0 => Ok(cursor.take(1)?[0].to_string()),
        1 => Ok((cursor.take(1)?[0] as i8).to_string()),
        2 => Ok(u16::from_le_bytes(cursor.take(2)?.try_into().expect("two bytes")).to_string()),
        3 => Ok(
            (u16::from_le_bytes(cursor.take(2)?.try_into().expect("two bytes")) as i16).to_string(),
        ),
        4 => Ok(cursor.u32()?.to_string()),
        5 => Ok((cursor.u32()? as i32).to_string()),
        6 => Ok(f32::from_le_bytes(cursor.take(4)?.try_into().expect("four bytes")).to_string()),
        7 => Ok((cursor.take(1)?[0] != 0).to_string()),
        8 => cursor.string(),
        10 => Ok(cursor.u64()?.to_string()),
        11 => Ok((cursor.u64()? as i64).to_string()),
        12 => Ok(f64::from_le_bytes(cursor.take(8)?.try_into().expect("eight bytes")).to_string()),
        9 => {
            let element = cursor.u32()?;
            let count = cursor.u64()?;
            if count > 1_000_000 {
                return Err("GGUF metadata array exceeds parser limit".into());
            }
            for _ in 0..count {
                let _ = read_value(cursor, element)?;
            }
            Ok(format!("array:{count}"))
        }
        other => Err(format!("unsupported GGUF metadata value type {other}")),
    }
}

fn retain(key: &str) -> bool {
    key == "general.architecture"
        || key == "general.alignment"
        || key.contains("block_count")
        || key.contains("embedding_length")
        || key.contains("attention.head_count")
        || key.contains("context_length")
        || key.contains("expert_count")
}
fn metadata<'a>(values: &'a [(String, String)], key: &str) -> Option<String> {
    values
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}
fn metadata_u64(values: &[(String, String)], key: &str) -> Option<u64> {
    metadata(values, key)?.parse().ok()
}
fn align(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        value
    } else {
        value.div_ceil(alignment) * alignment
    }
}
fn fingerprint(parsed: &[Parsed], files: &[FileProfile], tensors: &[TensorProfile]) -> String {
    let mut hash = 14695981039346656037_u64;
    for item in parsed {
        hash ^= item.version as u64;
        hash = hash.wrapping_mul(1099511628211);
        hash ^= item.data_offset;
        hash = hash.wrapping_mul(1099511628211);
        for (name, value) in &item.metadata {
            for byte in name.as_bytes().iter().chain(value.as_bytes()) {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(1099511628211);
            }
        }
    }
    for file in files {
        for byte in file.name.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(1099511628211);
        }
        hash ^= file.bytes;
        hash = hash.wrapping_mul(1099511628211);
    }
    for tensor in tensors {
        for byte in tensor.name.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(1099511628211);
        }
        hash ^= tensor.offset;
        hash = hash.wrapping_mul(1099511628211);
        hash ^= tensor.bytes.unwrap_or(0);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("fnv1a64-{hash:016x}")
}

#[cfg(test)]
#[path = "gguf_tests.rs"]
mod tests;
