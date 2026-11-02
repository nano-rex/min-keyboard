//! A Qwen2/Qwen3 transformer running on the CPU from a memory-mapped GGUF file.

use std::cell::RefCell;
use std::io;
use std::path::Path;

use crate::gguf::{Gguf, Mapping, TensorInfo, Value};
use crate::pool::{Pool, SyncPtr};
use crate::quant::{dequantize_row, Kind, Unpacked, Q8};
use crate::tokenizer::Tokenizer;

#[derive(Clone, Debug)]
pub struct Config {
    pub architecture: String,
    pub layers: usize,
    pub dim: usize,
    pub ffn: usize,
    pub heads: usize,
    pub kv_heads: usize,
    pub head_dim: usize,
    pub rope_base: f32,
    pub eps: f32,
    pub vocab: usize,
}

/// A weight matrix in the mapped file: `rows` rows of `columns` values.
#[derive(Clone, Copy, Debug)]
struct Weight {
    kind: Kind,
    rows: usize,
    columns: usize,
    offset: usize,
}

impl Weight {
    fn row<'a>(&self, file: &'a [u8], r: usize) -> &'a [u8] {
        let size = self.kind.row_bytes(self.columns);
        &file[self.offset + r * size..self.offset + (r + 1) * size]
    }
}

struct Layer {
    attn_norm: Vec<f32>,
    q: Weight,
    k: Weight,
    v: Weight,
    q_bias: Option<Vec<f32>>,
    k_bias: Option<Vec<f32>>,
    v_bias: Option<Vec<f32>>,
    q_norm: Option<Vec<f32>>,
    k_norm: Option<Vec<f32>>,
    output: Weight,
    ffn_norm: Vec<f32>,
    gate: Weight,
    up: Weight,
    down: Weight,
}

pub struct Model {
    map: Mapping,
    pub config: Config,
    pub tokenizer: Tokenizer,
    pub chat_template: String,
    pub name: String,
    embedding: Weight,
    output: Weight,
    output_norm: Vec<f32>,
    layers: Vec<Layer>,
    inv_freq: Vec<f32>,
}

/// Keys and values of the tokens seen so far, stored as bfloat16 to halve their memory.
pub struct Cache {
    keys: Vec<Vec<u16>>,
    values: Vec<Vec<u16>>,
    /// The tokens whose keys and values are stored, in order.
    pub tokens: Vec<u32>,
}

impl Cache {
    pub fn bytes(&self) -> usize {
        self.keys.iter().chain(&self.values).map(|v| v.capacity() * 2).sum()
    }

    /// Writes the first `len` tokens' keys and values to `path`, so a later load can skip them.
    pub fn save(&self, path: &Path, len: usize) -> io::Result<()> {
        let len = len.min(self.tokens.len());
        let kv_dim = self.keys.first().map_or(0, |k| k.len() / self.tokens.len().max(1));
        let mut out = Vec::with_capacity(16 + len * 4 + self.keys.len() * len * kv_dim * 4);
        out.extend_from_slice(CACHE_MAGIC);
        for v in [len, self.keys.len(), kv_dim] {
            out.extend_from_slice(&(v as u32).to_le_bytes());
        }
        self.tokens[..len].iter().for_each(|t| out.extend_from_slice(&t.to_le_bytes()));
        for buffer in self.keys.iter().chain(&self.values) {
            buffer[..len * kv_dim].iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        }
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, &out)?;
        std::fs::rename(&temporary, path)
    }

    /// Replaces the cache with the one saved at `path`, if that one holds exactly `tokens`.
    pub fn restore(&mut self, path: &Path, tokens: &[u32], kv_dim: usize) -> bool {
        let Ok(data) = std::fs::read(path) else { return false };
        let word = |i: usize| data.get(4 + i * 4..8 + i * 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
        let (Some(len), Some(layers), Some(dim)) = (word(0), word(1), word(2)) else { return false };
        let header = 16 + len * 4;
        if !data.starts_with(CACHE_MAGIC) || len != tokens.len() || layers != self.keys.len() || dim != kv_dim {
            return false;
        }
        if data.len() != header + 2 * layers * len * kv_dim * 2 {
            return false;
        }
        if (0..len).any(|i| word(3 + i) != Some(tokens[i] as usize)) {
            return false;
        }
        let mut values = data[header..].chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        for buffer in self.keys.iter_mut().chain(&mut self.values) {
            buffer.clear();
            buffer.extend(values.by_ref().take(len * kv_dim));
        }
        self.tokens = tokens.to_vec();
        true
    }

    /// Forgets every token from `len` on, for example draft tokens the model rejected.
    pub fn truncate(&mut self, len: usize, kv_dim: usize) {
        self.tokens.truncate(len);
        for buffer in self.keys.iter_mut().chain(&mut self.values) {
            buffer.truncate(len * kv_dim);
        }
    }
}

const CACHE_MAGIC: &[u8; 4] = b"MKV1";

fn to_bf16(v: f32) -> u16 {
    let bits = v.to_bits();
    ((bits + 0x7fff + ((bits >> 16) & 1)) >> 16) as u16
}

fn from_bf16(v: u16) -> f32 {
    f32::from_bits((v as u32) << 16)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn rms_norm(x: &mut [f32], weight: &[f32], eps: f32) {
    let mean = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    let scale = 1.0 / (mean + eps).sqrt();
    for (v, w) in x.iter_mut().zip(weight) {
        *v *= scale * w;
    }
}

thread_local! {
    static UNPACKED: RefCell<Unpacked> = RefCell::new(Unpacked::default());
    static SCORES: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

impl Model {
    pub fn load(path: &Path) -> io::Result<Model> {
        let map = Mapping::open(path)?;
        let gguf = Gguf::parse(map.bytes())?;
        let architecture = gguf.get("general.architecture").and_then(Value::as_str).unwrap_or("").to_string();
        if architecture != "qwen2" && architecture != "qwen3" {
            return Err(invalid(format!("Models of type \"{architecture}\" are not supported. Use a Qwen3 or Qwen2.5 model.")));
        }
        let int = |key: &str| -> io::Result<usize> {
            gguf.get(&format!("{architecture}.{key}"))
                .and_then(Value::as_int)
                .map(|v| v as usize)
                .ok_or_else(|| invalid(format!("The model file lacks {key}.")))
        };
        let float =
            |key: &str, default: f32| gguf.get(&format!("{architecture}.{key}")).and_then(Value::as_float).map_or(default, |v| v as f32);
        let dim = int("embedding_length")?;
        let heads = int("attention.head_count")?;
        let tokenizer = Tokenizer::from_gguf(&gguf)?;
        let config = Config {
            layers: int("block_count")?,
            dim,
            ffn: int("feed_forward_length")?,
            heads,
            kv_heads: int("attention.head_count_kv").unwrap_or(heads),
            head_dim: int("attention.key_length").unwrap_or(dim / heads.max(1)),
            rope_base: float("rope.freq_base", 10_000.0),
            eps: float("attention.layer_norm_rms_epsilon", 1e-6),
            vocab: tokenizer.vocab_size(),
            architecture,
        };
        if config.heads == 0 || config.kv_heads == 0 || !config.heads.is_multiple_of(config.kv_heads) || !config.head_dim.is_multiple_of(2)
        {
            return Err(invalid("The model's attention shape is not supported."));
        }

        let file = map.bytes();
        let info = |name: &str| -> io::Result<&TensorInfo> {
            gguf.tensors.get(name).ok_or_else(|| invalid(format!("The model lacks {name}, or stores it in an unsupported format.")))
        };
        let weight = |name: &str, rows: usize, columns: usize| -> io::Result<Weight> {
            let t = info(name)?;
            if t.dims.len() != 2 || t.dims[0] != columns || t.dims[1] != rows {
                return Err(invalid(format!("{name} has shape {:?}, expected [{columns}, {rows}]", t.dims)));
            }
            if !columns.is_multiple_of(crate::quant::QK_K) {
                return Err(invalid(format!("{name} has an unsupported width")));
            }
            Ok(Weight { kind: t.kind, rows, columns, offset: t.offset })
        };
        let vector = |name: &str, len: usize| -> io::Result<Vec<f32>> {
            let t = info(name)?;
            if t.dims.iter().product::<usize>() != len {
                return Err(invalid(format!("{name} has the wrong size")));
            }
            let mut out = vec![0.0; len];
            dequantize_row(t.kind, &file[t.offset..t.offset + t.size], &mut out);
            Ok(out)
        };
        let optional = |name: &str, len: usize| if gguf.tensors.contains_key(name) { vector(name, len).map(Some) } else { Ok(None) };

        let c = &config;
        let (q_dim, kv_dim) = (c.heads * c.head_dim, c.kv_heads * c.head_dim);
        let mut layers = Vec::with_capacity(c.layers);
        for i in 0..c.layers {
            let name = |part: &str| format!("blk.{i}.{part}");
            layers.push(Layer {
                attn_norm: vector(&name("attn_norm.weight"), c.dim)?,
                q: weight(&name("attn_q.weight"), q_dim, c.dim)?,
                k: weight(&name("attn_k.weight"), kv_dim, c.dim)?,
                v: weight(&name("attn_v.weight"), kv_dim, c.dim)?,
                q_bias: optional(&name("attn_q.bias"), q_dim)?,
                k_bias: optional(&name("attn_k.bias"), kv_dim)?,
                v_bias: optional(&name("attn_v.bias"), kv_dim)?,
                q_norm: optional(&name("attn_q_norm.weight"), c.head_dim)?,
                k_norm: optional(&name("attn_k_norm.weight"), c.head_dim)?,
                output: weight(&name("attn_output.weight"), c.dim, q_dim)?,
                ffn_norm: vector(&name("ffn_norm.weight"), c.dim)?,
                gate: weight(&name("ffn_gate.weight"), c.ffn, c.dim)?,
                up: weight(&name("ffn_up.weight"), c.ffn, c.dim)?,
                down: weight(&name("ffn_down.weight"), c.dim, c.ffn)?,
            });
        }
        let embedding = weight("token_embd.weight", c.vocab, c.dim)?;
        // Small models share the embedding matrix with the output layer.
        let output = if gguf.tensors.contains_key("output.weight") { weight("output.weight", c.vocab, c.dim)? } else { embedding };
        let output_norm = vector("output_norm.weight", c.dim)?;
        let half = c.head_dim / 2;
        let inv_freq = (0..half).map(|i| c.rope_base.powf(-(2.0 * i as f32) / c.head_dim as f32)).collect();
        let chat_template = gguf.get("tokenizer.chat_template").and_then(Value::as_str).unwrap_or("").to_string();
        let name = gguf.get("general.name").and_then(Value::as_str).unwrap_or("").to_string();
        drop(gguf);
        Ok(Model { map, config, tokenizer, chat_template, name, embedding, output, output_norm, layers, inv_freq })
    }

    pub fn new_cache(&self) -> Cache {
        Cache { keys: vec![Vec::new(); self.config.layers], values: vec![Vec::new(); self.config.layers], tokens: Vec::new() }
    }

    pub fn kv_dim(&self) -> usize {
        self.config.kv_heads * self.config.head_dim
    }

    /// `outputs[j][b * rows + r]` = row `r` of `weights[j]` · `x[b]`, for all weights at once.
    fn matmul(&self, pool: &Pool, weights: &[Weight], x: &[Q8], xf: &[f32], outputs: &mut [&mut [f32]]) {
        let file = self.map.bytes();
        let batch = x.len();
        let pointers: Vec<SyncPtr<f32>> = outputs.iter_mut().map(|o| SyncPtr(o.as_mut_ptr())).collect();
        for (w, o) in weights.iter().zip(outputs.iter()) {
            assert_eq!(o.len(), w.rows * batch);
        }
        // Rows of all weights, numbered one after another.
        let starts: Vec<usize> = weights.iter().scan(0, |acc, w| Some(std::mem::replace(acc, *acc + w.rows))).collect();
        let total: usize = weights.iter().map(|w| w.rows).sum();
        pool.for_chunks(total, 16, &|_, range| {
            UNPACKED.with(|cell| {
                let mut unpacked = cell.borrow_mut();
                for global in range {
                    let j = starts.partition_point(|&s| s <= global) - 1;
                    let (w, r) = (&weights[j], global - starts[j]);
                    unpacked.unpack(w.kind, w.row(file, r), w.columns);
                    for (b, xq) in x.iter().enumerate() {
                        let value = unpacked.dot(xq, &xf[b * w.columns..(b + 1) * w.columns]);
                        // SAFETY: each (b, r) of output j is written by exactly one thread.
                        unsafe { *pointers[j].get().add(b * w.rows + r) = value };
                    }
                }
            });
        });
    }

    fn quantize_rows(x: &[f32], width: usize) -> Vec<Q8> {
        x.chunks_exact(width).map(Q8::quantize).collect()
    }

    fn rope(&self, x: &mut [f32], position: usize) {
        let half = self.config.head_dim / 2;
        for head in x.chunks_exact_mut(self.config.head_dim) {
            for (i, &freq) in self.inv_freq.iter().enumerate() {
                let (sin, cos) = (position as f32 * freq).sin_cos();
                let (a, b) = (head[i], head[i + half]);
                head[i] = a * cos - b * sin;
                head[i + half] = a * sin + b * cos;
            }
        }
    }

    /// Runs `tokens` through the model after the cached ones and returns the last token's logits.
    pub fn forward(&self, pool: &Pool, cache: &mut Cache, tokens: &[u32]) -> Vec<f32> {
        let mut logits = self.forward_rows(pool, cache, tokens, false);
        logits.pop().unwrap_or_default()
    }

    /// Runs `tokens` like [`forward`](Model::forward) and returns the most likely next token after
    /// each of them, so several guessed tokens can be checked in one pass.
    pub fn greedy(&self, pool: &Pool, cache: &mut Cache, tokens: &[u32]) -> Vec<u32> {
        self.forward_rows(pool, cache, tokens, true).iter().map(|logits| argmax(logits)).collect()
    }

    /// The logits after the last token, or after every token when `all` is set.
    fn forward_rows(&self, pool: &Pool, cache: &mut Cache, tokens: &[u32], all: bool) -> Vec<Vec<f32>> {
        let c = &self.config;
        let file = self.map.bytes();
        let (batch, dim, hd) = (tokens.len(), c.dim, c.head_dim);
        let (q_dim, kv_dim) = (c.heads * hd, self.kv_dim());
        let start = cache.tokens.len();

        let mut x = vec![0.0f32; batch * dim];
        for (b, &token) in tokens.iter().enumerate() {
            dequantize_row(self.embedding.kind, self.embedding.row(file, token as usize), &mut x[b * dim..(b + 1) * dim]);
        }
        let mut xn = vec![0.0f32; batch * dim];
        let mut q = vec![0.0f32; batch * q_dim];
        let mut k = vec![0.0f32; batch * kv_dim];
        let mut v = vec![0.0f32; batch * kv_dim];
        let mut attn = vec![0.0f32; batch * q_dim];
        let mut proj = vec![0.0f32; batch * dim];
        let mut gate = vec![0.0f32; batch * c.ffn];
        let mut up = vec![0.0f32; batch * c.ffn];

        for (l, layer) in self.layers.iter().enumerate() {
            xn.copy_from_slice(&x);
            for row in xn.chunks_exact_mut(dim) {
                rms_norm(row, &layer.attn_norm, c.eps);
            }
            let xq = Self::quantize_rows(&xn, dim);
            self.matmul(pool, &[layer.q, layer.k, layer.v], &xq, &xn, &mut [&mut q, &mut k, &mut v]);
            for (buffer, bias, width) in [(&mut q, &layer.q_bias, q_dim), (&mut k, &layer.k_bias, kv_dim), (&mut v, &layer.v_bias, kv_dim)]
            {
                if let Some(bias) = bias {
                    for row in buffer.chunks_exact_mut(width) {
                        row.iter_mut().zip(bias).for_each(|(a, b)| *a += b);
                    }
                }
            }
            for b in 0..batch {
                let (qb, kb) = (&mut q[b * q_dim..(b + 1) * q_dim], &mut k[b * kv_dim..(b + 1) * kv_dim]);
                if let (Some(qn), Some(kn)) = (&layer.q_norm, &layer.k_norm) {
                    qb.chunks_exact_mut(hd).for_each(|h| rms_norm(h, qn, c.eps));
                    kb.chunks_exact_mut(hd).for_each(|h| rms_norm(h, kn, c.eps));
                }
                self.rope(qb, start + b);
                self.rope(kb, start + b);
            }
            cache.keys[l].extend(k.iter().map(|&v| to_bf16(v)));
            cache.values[l].extend(v.iter().map(|&v| to_bf16(v)));

            // Attention, one task per (token, head).
            let (keys, values) = (&cache.keys[l], &cache.values[l]);
            let group = c.heads / c.kv_heads;
            let scale = 1.0 / (hd as f32).sqrt();
            let out = SyncPtr(attn.as_mut_ptr());
            let q = &q;
            pool.for_chunks(batch * c.heads, 4, &|_, range| {
                SCORES.with(|cell| {
                    let mut scores = cell.borrow_mut();
                    for task in range {
                        let (b, h) = (task / c.heads, task % c.heads);
                        let visible = start + b + 1;
                        let query = &q[b * q_dim + h * hd..b * q_dim + (h + 1) * hd];
                        let kv_offset = (h / group) * hd;
                        scores.clear();
                        let mut max = f32::NEG_INFINITY;
                        for t in 0..visible {
                            let key = &keys[t * kv_dim + kv_offset..t * kv_dim + kv_offset + hd];
                            let s = query.iter().zip(key).map(|(a, &b)| a * from_bf16(b)).sum::<f32>() * scale;
                            max = max.max(s);
                            scores.push(s);
                        }
                        let mut sum = 0.0;
                        for s in scores.iter_mut() {
                            *s = (*s - max).exp();
                            sum += *s;
                        }
                        // SAFETY: each task writes its own head of its own token.
                        let result = unsafe { std::slice::from_raw_parts_mut(out.get().add(b * q_dim + h * hd), hd) };
                        result.fill(0.0);
                        for (t, &p) in scores.iter().enumerate() {
                            let value = &values[t * kv_dim + kv_offset..t * kv_dim + kv_offset + hd];
                            let p = p / sum;
                            result.iter_mut().zip(value).for_each(|(r, &v)| *r += p * from_bf16(v));
                        }
                    }
                });
            });

            let aq = Self::quantize_rows(&attn, q_dim);
            self.matmul(pool, &[layer.output], &aq, &attn, &mut [&mut proj]);
            x.iter_mut().zip(&proj).for_each(|(a, b)| *a += b);

            xn.copy_from_slice(&x);
            for row in xn.chunks_exact_mut(dim) {
                rms_norm(row, &layer.ffn_norm, c.eps);
            }
            let xq = Self::quantize_rows(&xn, dim);
            self.matmul(pool, &[layer.gate, layer.up], &xq, &xn, &mut [&mut gate, &mut up]);
            for (g, u) in gate.iter_mut().zip(&up) {
                *g = *g / (1.0 + (-*g).exp()) * u;
            }
            let hq = Self::quantize_rows(&gate, c.ffn);
            self.matmul(pool, &[layer.down], &hq, &gate, &mut [&mut proj]);
            x.iter_mut().zip(&proj).for_each(|(a, b)| *a += b);
        }
        cache.tokens.extend_from_slice(tokens);

        let mut rows = if all { x } else { x[(batch - 1) * dim..].to_vec() };
        for row in rows.chunks_exact_mut(dim) {
            rms_norm(row, &self.output_norm, c.eps);
        }
        let quantized = Self::quantize_rows(&rows, dim);
        let mut logits = vec![0.0f32; quantized.len() * c.vocab];
        self.matmul(pool, &[self.output], &quantized, &rows, &mut [&mut logits]);
        logits.chunks_exact(c.vocab).map(<[f32]>::to_vec).collect()
    }

    /// Feeds a prompt, reusing whatever beginning of it is already in the cache, and returns the
    /// logits after its last token. `stop` is checked between batches.
    pub fn prompt(&self, pool: &Pool, cache: &mut Cache, tokens: &[u32], stop: &dyn Fn() -> bool) -> Option<Vec<f32>> {
        const BATCH: usize = 64;
        let common = cache.tokens.iter().zip(tokens).take_while(|(a, b)| a == b).count().min(tokens.len().saturating_sub(1));
        cache.truncate(common, self.kv_dim());
        let mut logits = None;
        for chunk in tokens[common..].chunks(BATCH) {
            if stop() {
                return None;
            }
            logits = Some(self.forward(pool, cache, chunk));
        }
        logits
    }
}

pub fn argmax(logits: &[f32]) -> u32 {
    logits.iter().enumerate().fold((0, f32::NEG_INFINITY), |best, (i, &v)| if v > best.1 { (i, v) } else { best }).0 as u32
}
