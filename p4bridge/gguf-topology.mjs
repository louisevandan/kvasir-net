#!/usr/bin/env node
/**
 * Read a GGUF's header and say what of it can be handed to a remote worker.
 *
 * ## Why this exists
 *
 * The participation market was handing out assignments that cannot be served.
 * It knew `n_layer = 45` and `n_expert = 288` and assumed every layer held
 * experts, so it offered layer 0 — which in Step 3.7 is one of three leading
 * dense blocks with no expert tensors at all. A device would have accepted the
 * window, asked for the shard, and found nothing. Nobody would have learned
 * that until the download path existed.
 *
 * Those two numbers were hand-written constants in the load plan. That is the
 * shape of the bug: MoE topology is per-model, and this model alone carries
 * `leading_dense_block_count = 3`, `moe_every_n_layers = 1`, a separate shared
 * expert (`expert_shared_feed_forward_length`) and an expert FFN width
 * (1280) unrelated to the dense one (11264). A model where MoE appears every
 * second layer is a legal GGUF and would break a constant that happened to fit
 * this one. So nothing here is declared; it is all read from the file, and
 * anything that cannot be read is an error rather than a default.
 *
 * ## What it emits
 *
 * One JSON record, small enough to live in the catalog (42 layers × 3 tensors
 * is a few KB), carrying both things the network needs:
 *
 *   expert_layers  the layers that actually hold routed experts, so the market
 *                  only offers windows that exist.
 *   expert_slice   per layer, per tensor: absolute file offset, bytes per
 *                  expert, and the ggml type — the byte map a range server
 *                  needs to cut experts [begin, end) out of the file.
 *
 * ## Why a shard is a plain byte range
 *
 * In these tensors the expert index is the LAST dimension in GGUF's
 * fastest-first ordering, which makes it the slowest-varying one in memory, so
 * one expert's weights are contiguous. And each expert's slice is a whole
 * number of quantisation blocks — this asserts that rather than assuming it —
 * so a cut never lands inside a block. Serving a shard is therefore a read of
 * `offset + begin*per_expert` for `(end-begin)*per_expert` bytes. No
 * re-quantising, no repacking, nothing to get subtly wrong.
 *
 * Usage:
 *   node gguf-topology.mjs <model.gguf> [--id <catalog id>] > topology.json
 *
 * It reads only the header. The tensor data — 122 GB for Step 3.7 — is never
 * touched.
 */
import { open } from 'node:fs/promises';

/**
 * ggml type → [block size in elements, bytes per block].
 *
 * Deliberately not defaulted. An unknown type here means a quantisation this
 * was never checked against, and guessing a size would produce byte offsets
 * that look plausible and address the wrong weights.
 */
const GGML_TYPES = {
  0: ['F32', 1, 4], 1: ['F16', 1, 2],
  2: ['Q4_0', 32, 18], 3: ['Q4_1', 32, 20],
  6: ['Q5_0', 32, 22], 7: ['Q5_1', 32, 24],
  8: ['Q8_0', 32, 34], 9: ['Q8_1', 32, 40],
  10: ['Q2_K', 256, 84], 11: ['Q3_K', 256, 110], 12: ['Q4_K', 256, 144],
  13: ['Q5_K', 256, 176], 14: ['Q6_K', 256, 210], 15: ['Q8_K', 256, 292],
  16: ['IQ2_XXS', 256, 66], 17: ['IQ2_XS', 256, 74], 18: ['IQ3_XXS', 256, 98],
  19: ['IQ1_S', 256, 50], 20: ['IQ4_NL', 32, 18], 21: ['IQ3_S', 256, 110],
  22: ['IQ2_S', 256, 82], 23: ['IQ4_XS', 256, 136],
  24: ['I8', 1, 1], 25: ['I16', 1, 2], 26: ['I32', 1, 4], 27: ['I64', 1, 8],
  28: ['F64', 1, 8], 29: ['IQ1_M', 256, 56], 30: ['BF16', 1, 2],
};

/** A cursor over the header bytes, so the parse reads as the format does. */
class Cursor {
  constructor(buf) { this.buf = buf; this.at = 0; }
  take(n) {
    if (this.at + n > this.buf.length) throw new Error('header ended mid-value');
    const out = this.buf.subarray(this.at, this.at + n);
    this.at += n;
    return out;
  }
  u8() { return this.take(1)[0]; }
  i8() { return this.take(1).readInt8(0); }
  u16() { return this.take(2).readUInt16LE(0); }
  i16() { return this.take(2).readInt16LE(0); }
  u32() { return this.take(4).readUInt32LE(0); }
  i32() { return this.take(4).readInt32LE(0); }
  f32() { return this.take(4).readFloatLE(0); }
  f64() { return this.take(8).readDoubleLE(0); }
  bool() { return this.take(1)[0] !== 0; }
  // Offsets and dimensions are u64 in the format. Number is exact to 2^53,
  // which is four petabytes of file — past anything that will be served here,
  // and BigInt everywhere would make the arithmetic below unreadable.
  u64() { return Number(this.take(8).readBigUInt64LE(0)); }
  i64() { return Number(this.take(8).readBigInt64LE(0)); }
  str() { return this.take(this.u64()).toString('utf8'); }
  value(type) {
    switch (type) {
      case 0: return this.u8();
      case 1: return this.i8();
      case 2: return this.u16();
      case 3: return this.i16();
      case 4: return this.u32();
      case 5: return this.i32();
      case 6: return this.f32();
      case 7: return this.bool();
      case 8: return this.str();
      case 9: {
        const elem = this.u32();
        const n = this.u64();
        const out = [];
        for (let i = 0; i < n; i += 1) out.push(this.value(elem));
        return out;
      }
      case 10: return this.u64();
      case 11: return this.i64();
      case 12: return this.f64();
      default: throw new Error(`unknown metadata value type ${type}`);
    }
  }
}

/**
 * Read enough of the file to cover the header, growing if the tensor directory
 * turns out to be longer than the first guess. Step 3.7's is about 5 MB.
 */
async function readHeader(path) {
  const fh = await open(path, 'r');
  try {
    let size = 8 << 20;
    for (;;) {
      const buf = Buffer.alloc(size);
      const { bytesRead } = await fh.read(buf, 0, size, 0);
      const slice = buf.subarray(0, bytesRead);
      try {
        return { ...parseHeader(slice), path };
      } catch (error) {
        if (!/header ended mid-value/.test(error.message) || bytesRead < size) throw error;
        size *= 4;
        if (size > (512 << 20)) throw new Error('tensor directory is implausibly large');
      }
    }
  } finally {
    await fh.close();
  }
}

function parseHeader(buf) {
  const c = new Cursor(buf);
  if (c.take(4).toString('latin1') !== 'GGUF') throw new Error('not a GGUF file');
  const version = c.u32();
  const tensorCount = c.u64();
  const kvCount = c.u64();

  const metadata = {};
  for (let i = 0; i < kvCount; i += 1) {
    const key = c.str();
    metadata[key] = c.value(c.u32());
  }

  const tensors = [];
  for (let i = 0; i < tensorCount; i += 1) {
    const name = c.str();
    const dims = [];
    const nDims = c.u32();
    for (let d = 0; d < nDims; d += 1) dims.push(c.u64());
    const type = c.u32();
    const offset = c.u64();
    tensors.push({ name, dims, type, offset });
  }

  // Tensor data begins at the next alignment boundary after the header.
  const alignment = metadata['general.alignment'] ?? 32;
  const dataStart = Math.ceil(c.at / alignment) * alignment;
  return { version, metadata, tensors, alignment, dataStart };
}

function tensorBytes({ dims, type, name }) {
  const known = GGML_TYPES[type];
  if (!known) throw new Error(`${name}: unsupported ggml type ${type}`);
  const [, blockElems, blockBytes] = known;
  const elems = dims.reduce((a, b) => a * b, 1);
  if (elems % blockElems !== 0) {
    throw new Error(`${name}: ${elems} elements is not a whole number of ${known[0]} blocks`);
  }
  return (elems / blockElems) * blockBytes;
}

/** Everything the network needs to know about one model's expert layout. */
export function topology(header, id) {
  const { metadata, tensors, dataStart } = header;
  const arch = metadata['general.architecture'];
  const get = (suffix) => metadata[`${arch}.${suffix}`];

  const EXPERT_TENSOR = /^blk\.(\d+)\.(ffn_(?:down|gate|up)_exps)\.weight$/;
  const nExpert = get('expert_count');
  if (!nExpert) throw new Error(`${arch} declares no expert_count — not a MoE model`);

  const slice = {};
  for (const t of tensors) {
    const m = EXPERT_TENSOR.exec(t.name);
    if (!m) continue;
    const layer = Number(m[1]);
    const which = m[2];
    // The expert index is the last dimension, so one expert is contiguous.
    if (t.dims[t.dims.length - 1] !== nExpert) {
      throw new Error(`${t.name}: last dimension ${t.dims.at(-1)} is not the expert count ${nExpert}`);
    }
    const total = tensorBytes(t);
    if (total % nExpert !== 0) {
      // Would mean an expert's slice ends inside a quantisation block, and a
      // range cut there hands the worker half a block it cannot decode.
      throw new Error(`${t.name}: ${total} bytes does not divide into ${nExpert} experts`);
    }
    (slice[layer] ??= {})[which] = {
      offset: dataStart + t.offset,
      per_expert: total / nExpert,
      type: t.type,
      type_name: GGML_TYPES[t.type][0],
      dims: t.dims,
    };
  }

  const layers = Object.keys(slice).map(Number).sort((a, b) => a - b);
  if (!layers.length) throw new Error('no routed-expert tensors found');

  // A layer missing one of the three is not servable: a worker needs gate, up
  // and down to compute an expert at all. Better to refuse the layer than to
  // hand out a window that produces silence two subsystems later.
  const want = ['ffn_gate_exps', 'ffn_up_exps', 'ffn_down_exps'];
  const complete = layers.filter((l) => want.every((w) => slice[l][w]));
  const partial = layers.filter((l) => !want.every((w) => slice[l][w]));

  const perExpert = want.reduce((sum, w) => sum + slice[complete[0]][w].per_expert, 0);

  return {
    id: id ?? null,
    architecture: arch,
    source: header.path,
    n_embd: get('embedding_length') ?? null,
    n_layer: get('block_count') ?? null,
    n_expert: nExpert,
    n_expert_used: get('expert_used_count') ?? null,
    // Recorded because they are the reason a constant would have been wrong,
    // not because anything reads them yet.
    leading_dense_block_count: get('leading_dense_block_count') ?? 0,
    moe_every_n_layers: get('moe_every_n_layers') ?? 1,
    expert_ffn_length: get('expert_feed_forward_length') ?? null,
    shared_expert_ffn_length: get('expert_shared_feed_forward_length') ?? null,
    /** The only layers a volunteer may be given. */
    expert_layers: complete,
    incomplete_expert_layers: partial,
    /** Bytes one expert costs across all three tensors, in one layer. */
    bytes_per_expert: perExpert,
    expert_tensors: want,
    expert_slice: slice,
  };
}

async function main() {
  const args = process.argv.slice(2);
  const path = args.find((a) => !a.startsWith('--'));
  if (!path) {
    console.error('usage: gguf-topology.mjs <model.gguf> [--id <catalog id>]');
    process.exit(2);
  }
  const idFlag = args.indexOf('--id');
  const id = idFlag >= 0 ? args[idFlag + 1] : null;
  const header = await readHeader(path);
  const record = topology(header, id);
  process.stdout.write(`${JSON.stringify(record, null, 2)}\n`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => { console.error(`gguf-topology: ${error.message}`); process.exit(1); });
}
