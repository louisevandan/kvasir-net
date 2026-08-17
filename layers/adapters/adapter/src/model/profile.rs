use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TensorProfile {
    pub name: String,
    pub dimensions: Vec<u64>,
    pub dtype: u32,
    pub offset: u64,
    pub bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ModelProfile {
    pub schema: u32,
    pub artifact: String,
    pub fingerprint: String,
    pub files: Vec<FileProfile>,
    pub architecture: Option<String>,
    pub layers: Option<u64>,
    pub embedding: Option<u64>,
    pub attention_heads: Option<u64>,
    pub attention_heads_kv: Option<u64>,
    pub context: Option<u64>,
    pub expert_count: Option<u64>,
    pub tensors: Vec<TensorProfile>,
    pub layer_bytes: Vec<u64>,
    pub boundary_bytes: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FileProfile {
    pub name: String,
    pub bytes: u64,
    pub complete: bool,
}
