// Reads a GGUF file's metadata and tensor table without loading weights.
//
// The catalogue needs the numbers that decide a load - layer count, expert
// count, embedding width, trained context, and how many bytes the routed
// experts hold - before any stage server is started, and for models whose
// shards total hundreds of gigabytes. Only the header of each shard is read.
//
// This is OUTER knowledge. P4 carries a plan string it does not interpret.

import fs from 'node:fs';

// ggml type -> [block byte size, elements per block]
const GGML_TYPE_SIZE = {
  0: [4, 1], 1: [2, 1], 2: [18, 32], 3: [20, 32], 6: [22, 32], 7: [24, 32],
  8: [34, 32], 9: [36, 32], 10: [84, 256], 11: [110, 256], 12: [144, 256],
  13: [176, 256], 14: [210, 256], 15: [292, 256], 16: [66, 256], 17: [74, 256],
  18: [98, 256], 19: [50, 256], 20: [18, 32], 21: [110, 256], 22: [82, 256],
  23: [136, 256], 24: [1, 1], 25: [2, 1], 26: [4, 1], 27: [8, 1], 28: [8, 1],
  29: [56, 256], 30: [2, 1], 39: [17, 32],
};

// The metadata block is a few MiB even with a 250k-token vocabulary, and these
// files live on a network share, so read a small prefix and grow only when the
// parser runs off its end. Reading a fixed large prefix of every shard turned a
// catalogue pass into minutes per model.
const FIRST_READ = 4 * 1024 * 1024;
const MAX_READ = 512 * 1024 * 1024;

export function readGgufHeader(file, options = {}) {
  let length = FIRST_READ;
  for (;;) {
    try {
      return parseHeader(file, length, options);
    } catch (error) {
      if (error?.code !== 'GGUF_TRUNCATED' || length >= MAX_READ) throw error;
      length = Math.min(length * 8, MAX_READ);
    }
  }
}

function parseHeader(file, length, { keepTokens = false } = {}) {
  const handle = fs.openSync(file, 'r');
  let buf;
  let size;
  let read;
  try {
    size = fs.fstatSync(handle).size;
    buf = Buffer.alloc(Math.min(size, length));
    read = fs.readSync(handle, buf, 0, buf.length, 0);
  } finally {
    fs.closeSync(handle);
  }
  const truncated = read < size;
  const short = () => {
    const error = new Error(`GGUF metadata exceeds the ${read} byte prefix read from ${file}`);
    error.code = 'GGUF_TRUNCATED';
    return error;
  };
  const need = (bytes) => {
    if (offset + bytes > read) throw truncated ? short() : new Error(`corrupt GGUF header in ${file}`);
  };
  if (buf.toString('ascii', 0, 4) !== 'GGUF') throw new Error(`not a GGUF file: ${file}`);

  let offset = 4;
  const u32 = () => { need(4); const v = buf.readUInt32LE(offset); offset += 4; return v; };
  const u64 = () => { need(8); const v = Number(buf.readBigUInt64LE(offset)); offset += 8; return v; };
  const str = () => { const n = u64(); need(n); const s = buf.toString('utf8', offset, offset + n); offset += n; return s; };
  const scalar = (type) => {
    switch (type) {
      case 0: need(1); return buf.readUInt8(offset++);
      case 1: need(1); return buf.readInt8(offset++);
      case 2: { need(2); const v = buf.readUInt16LE(offset); offset += 2; return v; }
      case 3: { need(2); const v = buf.readInt16LE(offset); offset += 2; return v; }
      case 4: return u32();
      case 5: { need(4); const v = buf.readInt32LE(offset); offset += 4; return v; }
      case 6: { need(4); const v = buf.readFloatLE(offset); offset += 4; return v; }
      case 7: need(1); return buf.readUInt8(offset++) !== 0;
      case 8: return str();
      case 10: return u64();
      case 11: { need(8); const v = Number(buf.readBigInt64LE(offset)); offset += 8; return v; }
      case 12: { need(8); const v = buf.readDoubleLE(offset); offset += 8; return v; }
      default: throw new Error(`unsupported GGUF value type ${type}`);
    }
  };

  const version = u32();
  const tensorCount = u64();
  const kvCount = u64();
  const kv = {};
  const tokens = [];
  for (let i = 0; i < kvCount; i += 1) {
    const key = str();
    const type = u32();
    if (type === 9) {
      const elementType = u32();
      const n = u64();
      const sample = [];
      for (let j = 0; j < n; j += 1) {
        const value = scalar(elementType);
        if (sample.length < 8) sample.push(value);
        if (keepTokens && key === 'tokenizer.ggml.tokens') tokens.push(value);
      }
      kv[key] = { array: n, sample };
    } else {
      kv[key] = scalar(type);
    }
  }

  const tensors = [];
  for (let i = 0; i < tensorCount; i += 1) {
    const name = str();
    const dims = u32();
    const ne = [];
    for (let d = 0; d < dims; d += 1) ne.push(u64());
    const type = u32();
    const dataOffset = u64(); // offset within the tensor data section
    const elements = ne.reduce((a, b) => a * b, 1);
    const spec = GGML_TYPE_SIZE[type];
    tensors.push({ name, type, ne, dataOffset, bytes: spec ? (elements / spec[1]) * spec[0] : null });
  }

  const alignment = kv['general.alignment'] ?? 32;
  if (!Number.isSafeInteger(alignment) || alignment < 1) throw new Error(`invalid GGUF alignment in ${file}`);
  return { file, version, kv, tensors, tokens, fileSize: size,
    dataStart: tensorCount === 0 ? size : Math.ceil(offset / alignment) * alignment };
}

/// Sums one logical model's shards into the numbers a load decision needs.
export function describeModel(shards) {
  let kv = null;
  let fileBytes = 0;
  let total = 0;
  let experts = 0;
  let blockOther = 0;
  let outside = 0;
  let unknown = 0;
  for (const shard of shards) {
    const header = readGgufHeader(shard);
    if (!kv) kv = header.kv;
    fileBytes += header.fileSize;
    for (const tensor of header.tensors) {
      if (tensor.bytes == null) { unknown += 1; continue; }
      total += tensor.bytes;
      if (/ffn_(up|down|gate)_exps/.test(tensor.name)) experts += tensor.bytes;
      else if (/^blk\./.test(tensor.name)) blockOther += tensor.bytes;
      else outside += tensor.bytes;
    }
  }
  const arch = kv['general.architecture'];
  const get = (suffix) => kv[`${arch}.${suffix}`];
  const plain = (value) => (value && typeof value === 'object' && 'array' in value ? null : value);
  const blocks = plain(get('block_count'));
  const nextn = plain(get('nextn_predict_layers')) ?? 0;
  return {
    architecture: arch,
    name: kv['general.name'] ?? null,
    size_label: kv['general.size_label'] ?? null,
    shards: shards.length,
    file_bytes: fileBytes,
    tensor_bytes: total,
    expert_bytes: experts,
    other_block_bytes: blockOther,
    non_block_bytes: outside,
    unknown_type_tensors: unknown,
    block_count: blocks,
    nextn_predict_layers: nextn,
    // llama.cpp's n_layer() excludes the NextN/MTP blocks, and a stage cut
    // that ends past it trips an assert inside the graph builder.
    trunk_layers: blocks == null ? null : blocks - nextn,
    expert_count: plain(get('expert_count')) ?? null,
    expert_used_count: plain(get('expert_used_count')) ?? null,
    embedding_length: plain(get('embedding_length')) ?? null,
    context_length: plain(get('context_length')) ?? null,
    head_count_kv: get('attention.head_count_kv') ?? null,
    key_length: plain(get('attention.key_length')) ?? null,
    value_length: plain(get('attention.value_length')) ?? null,
    full_attention_interval: plain(get('full_attention_interval')) ?? null,
    eos_token_id: plain(kv['tokenizer.ggml.eos_token_id']) ?? null,
    bos_token_id: plain(kv['tokenizer.ggml.bos_token_id']) ?? null,
    chat_template_markers: chatTemplateMarkers(kv['tokenizer.chat_template']),
  };
}

const MARKERS = ['<|im_start|>', '<|im_end|>', '<|turn>', '<turn|>', '<start_of_turn>',
  '[INST]', '<|User|>', '<|Assistant|>', '<think>', '</think>', '<|channel>'];

function chatTemplateMarkers(template) {
  if (typeof template !== 'string') return [];
  return MARKERS.filter((marker) => template.includes(marker));
}
