//! The weight formats of GGUF files and the matrix-vector products over them.
//!
//! Activations are quantized to int8 with one scale per 256 values ([`Q8`]) and each weight row is
//! unpacked to int8 values with integer scales ([`Unpacked`]), so the inner loops of every format
//! are integer dot products of 16 values. Block layouts follow ggml's `ggml-common.h`.

pub const QK_K: usize = 256;

/// The integer type unpacked weights and quantized activations are stored as. Products of two
/// lanes accumulate into 32 bits (`pmaddwd` on x86, `smlal` on ARM).
pub type Lane = i16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    F32,
    F16,
    BF16,
    Q4_0,
    Q8_0,
    Q4K,
    Q5K,
    Q6K,
}

impl Kind {
    pub fn from_ggml(id: u32) -> Option<Kind> {
        Some(match id {
            0 => Kind::F32,
            1 => Kind::F16,
            2 => Kind::Q4_0,
            8 => Kind::Q8_0,
            12 => Kind::Q4K,
            13 => Kind::Q5K,
            14 => Kind::Q6K,
            30 => Kind::BF16,
            _ => return None,
        })
    }

    /// Values per block.
    pub fn block_elements(self) -> usize {
        match self {
            Kind::F32 | Kind::F16 | Kind::BF16 => 1,
            Kind::Q4_0 | Kind::Q8_0 => 32,
            Kind::Q4K | Kind::Q5K | Kind::Q6K => QK_K,
        }
    }

    pub fn block_bytes(self) -> usize {
        match self {
            Kind::F32 => 4,
            Kind::F16 | Kind::BF16 => 2,
            Kind::Q4_0 => 18,
            Kind::Q8_0 => 34,
            Kind::Q4K => 144,
            Kind::Q5K => 176,
            Kind::Q6K => 210,
        }
    }

    pub fn row_bytes(self, columns: usize) -> usize {
        columns / self.block_elements() * self.block_bytes()
    }
}

/// IEEE half precision to single precision.
pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) as u32) << 31;
    let exponent = ((bits >> 10) & 0x1f) as u32;
    let mantissa = (bits & 0x3ff) as u32;
    let value = match (exponent, mantissa) {
        (0, 0) => sign,
        (0, m) => {
            // Subnormal: normalise the mantissa.
            let shift = m.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | (((m << shift) & 0x3ff) << 13)
        }
        (0x1f, m) => sign | 0x7f80_0000 | (m << 13),
        (e, m) => sign | ((e + 112) << 23) | (m << 13),
    };
    f32::from_bits(value)
}

fn half(bytes: &[u8], at: usize) -> f32 {
    f16_to_f32(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

/// A vector quantized to int8 with one scale per 256 values (like ggml's Q8_K), and the sum of
/// each group of 16 quantized values for the formats that store an offset.
#[derive(Default, Clone)]
pub struct Q8 {
    pub qs: Vec<Lane>,
    pub scales: Vec<f32>,
    pub sums: Vec<i32>,
}

impl Q8 {
    pub fn quantize(x: &[f32]) -> Q8 {
        let mut q = Q8::default();
        q.set(x);
        q
    }

    pub fn set(&mut self, x: &[f32]) {
        assert!(x.len().is_multiple_of(QK_K), "vector length must be a multiple of 256");
        self.qs.resize(x.len(), 0);
        self.scales.resize(x.len() / QK_K, 0.0);
        self.sums.resize(x.len() / 16, 0);
        for (b, block) in x.chunks_exact(QK_K).enumerate() {
            let max = block.iter().fold(0f32, |m, v| m.max(v.abs()));
            let scale = max / 127.0;
            let inverse = if scale > 0.0 { 1.0 / scale } else { 0.0 };
            let qs = &mut self.qs[b * QK_K..(b + 1) * QK_K];
            for (q, &v) in qs.iter_mut().zip(block) {
                *q = (v * inverse).round() as i8 as Lane;
            }
            self.scales[b] = scale;
            for (g, sum) in self.sums[b * 16..(b + 1) * 16].iter_mut().enumerate() {
                *sum = qs[g * 16..g * 16 + 16].iter().map(|&q| q as i32).sum();
            }
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
enum Layout {
    /// `weight[i] = d[i / 256] * q[i] - dmin[i / 256] * mins[i / 16]` (K-quants), where `q`
    /// already includes the integer scale of its group of 16.
    #[default]
    Super,
    /// `weight[i] = block_d[i / 32] * q[i]` (Q4_0, Q8_0).
    Block32,
    /// Unquantized; the values are in `float`.
    Float,
}

/// One weight row unpacked to int8 values and integer scales, for [`Unpacked::dot`].
#[derive(Default)]
pub struct Unpacked {
    pub q: Vec<Lane>,
    scales: [Lane; 16],
    mins: Vec<i32>,
    d: Vec<f32>,
    dmin: Vec<f32>,
    block_d: Vec<f32>,
    float: Vec<f32>,
    layout: Layout,
}

/// The 6-bit scale and min of sub-block `j` in a Q4_K or Q5_K block.
fn scale_min_k4(j: usize, q: &[u8]) -> (u8, u8) {
    if j < 4 {
        (q[j] & 63, q[j + 4] & 63)
    } else {
        ((q[j + 4] & 0xf) | ((q[j - 4] >> 6) << 4), (q[j + 4] >> 4) | ((q[j] >> 6) << 4))
    }
}

impl Unpacked {
    pub fn unpack(&mut self, kind: Kind, row: &[u8], columns: usize) {
        if matches!(kind, Kind::F32 | Kind::F16 | Kind::BF16) {
            self.layout = Layout::Float;
            self.float.resize(columns, 0.0);
            dequantize_row(kind, row, &mut self.float);
            return;
        }
        self.q.resize(columns, 0);
        if matches!(kind, Kind::Q4_0 | Kind::Q8_0) {
            self.layout = Layout::Block32;
            self.block_d.resize(columns / 32, 0.0);
            for ((block, q), d) in row.chunks_exact(kind.block_bytes()).zip(self.q.chunks_exact_mut(32)).zip(&mut self.block_d) {
                *d = half(block, 0);
                if kind == Kind::Q8_0 {
                    for (q, &v) in q.iter_mut().zip(&block[2..34]) {
                        *q = v as i8 as Lane;
                    }
                } else {
                    let (lo, hi) = q.split_at_mut(16);
                    for ((lo, hi), &v) in lo.iter_mut().zip(hi).zip(&block[2..18]) {
                        *lo = ((v & 0xf) as i8 - 8) as Lane;
                        *hi = ((v >> 4) as i8 - 8) as Lane;
                    }
                }
            }
            return;
        }
        self.layout = Layout::Super;
        let blocks = columns / QK_K;
        self.mins.resize(blocks * 16, 0);
        self.d.resize(blocks, 0.0);
        self.dmin.resize(blocks, 0.0);
        for (b, block) in row.chunks_exact(kind.block_bytes()).enumerate() {
            let q = &mut self.q[b * QK_K..(b + 1) * QK_K];
            let (scales, mins) = (&mut self.scales, &mut self.mins[b * 16..(b + 1) * 16]);
            match kind {
                Kind::Q4K | Kind::Q5K => {
                    self.d[b] = half(block, 0);
                    self.dmin[b] = half(block, 2);
                    for sub in 0..8 {
                        let (sc, m) = scale_min_k4(sub, &block[4..16]);
                        scales[sub * 2..sub * 2 + 2].fill(sc as Lane);
                        mins[sub * 2..sub * 2 + 2].fill(m as i32);
                    }
                    if kind == Kind::Q4K {
                        unpack_q4k(block[16..144].try_into().unwrap(), q);
                    } else {
                        unpack_q5k(block[16..48].try_into().unwrap(), block[48..176].try_into().unwrap(), q);
                    }
                }
                Kind::Q6K => {
                    self.d[b] = half(block, 208);
                    self.dmin[b] = 0.0;
                    for (scale, &s) in scales.iter_mut().zip(&block[192..208]) {
                        *scale = s as i8 as Lane;
                    }
                    mins.fill(0);
                    unpack_q6k(block, q);
                }
                _ => unreachable!(),
            }
            // Fold the group scales into the values: |q| <= 32 and |scale| <= 127, so they fit.
            for (group, &scale) in q.chunks_exact_mut(16).zip(scales.iter()) {
                group.iter_mut().for_each(|v| *v *= scale);
            }
        }
    }

    /// The weight at `i`, for tests and embedding lookups.
    fn value(&self, i: usize) -> f32 {
        match self.layout {
            Layout::Float => self.float[i],
            Layout::Block32 => self.block_d[i / 32] * self.q[i] as f32,
            Layout::Super => {
                let (b, g) = (i / QK_K, i / 16);
                self.d[b] * self.q[i] as f32 - self.dmin[b] * self.mins[g] as f32
            }
        }
    }

    /// Dot product with a quantized vector (`x`) or, for float rows, with the original (`xf`).
    pub fn dot(&self, x: &Q8, xf: &[f32]) -> f32 {
        let mut total = 0f32;
        match self.layout {
            Layout::Float => return self.float.iter().zip(xf).map(|(a, b)| a * b).sum(),
            Layout::Block32 => {
                for (b, (q, xq)) in self.q.chunks_exact(QK_K).zip(x.qs.chunks_exact(QK_K)).enumerate() {
                    let mut sum = 0f32;
                    for (k, (q, xq)) in q.chunks_exact(32).zip(xq.chunks_exact(32)).enumerate() {
                        let (lo, hi) = (dot16(&q[..16], &xq[..16]), dot16(&q[16..], &xq[16..]));
                        sum += self.block_d[b * 8 + k] * (lo + hi) as f32;
                    }
                    total += x.scales[b] * sum;
                }
            }
            Layout::Super => {
                for (b, (q, xq)) in self.q.chunks_exact(QK_K).zip(x.qs.chunks_exact(QK_K)).enumerate() {
                    let sum = dot256(q.try_into().unwrap(), xq.try_into().unwrap());
                    let (mins, sums) = (&self.mins[b * 16..b * 16 + 16], &x.sums[b * 16..b * 16 + 16]);
                    let offset: i32 = mins.iter().zip(sums).map(|(m, s)| m * s).sum();
                    total += x.scales[b] * (self.d[b] * sum as f32 - self.dmin[b] * offset as f32);
                }
            }
        }
        total
    }
}

/// The integer dot product of a block. |a| <= 32 * 127 and |b| <= 127, so 256 products fit in i32.
#[inline(always)]
fn dot256(a: &[Lane; QK_K], b: &[Lane; QK_K]) -> i32 {
    // Pairs of products summed together match x86's `pmaddwd`.
    let mut lanes = [0i32; 8];
    for (a, b) in a.chunks_exact(16).zip(b.chunks_exact(16)) {
        for i in 0..8 {
            lanes[i] += a[i] as i32 * b[i] as i32 + a[i + 8] as i32 * b[i + 8] as i32;
        }
    }
    lanes.iter().sum()
}

/// The integer dot product of 16 values.
#[inline(always)]
fn dot16(a: &[Lane], b: &[Lane]) -> i32 {
    let (a, b): (&[Lane; 16], &[Lane; 16]) = (a.try_into().unwrap(), b.try_into().unwrap());
    let mut pairs = [0i32; 8];
    for i in 0..8 {
        pairs[i] = a[i] as i32 * b[i] as i32 + a[i + 8] as i32 * b[i + 8] as i32;
    }
    pairs.iter().sum()
}

fn unpack_q4k(qs: &[u8; 128], q: &mut [Lane]) {
    for j in 0..4 {
        let (out, ql) = (&mut q[j * 64..j * 64 + 64], &qs[j * 32..j * 32 + 32]);
        let (lo, hi) = out.split_at_mut(32);
        for l in 0..32 {
            lo[l] = (ql[l] & 0xf) as Lane;
            hi[l] = (ql[l] >> 4) as Lane;
        }
    }
}

fn unpack_q5k(qh: &[u8; 32], qs: &[u8; 128], q: &mut [Lane]) {
    for j in 0..4 {
        let (out, ql) = (&mut q[j * 64..j * 64 + 64], &qs[j * 32..j * 32 + 32]);
        let (lo, hi) = out.split_at_mut(32);
        for l in 0..32 {
            lo[l] = ((ql[l] & 0xf) | (((qh[l] >> (2 * j)) & 1) << 4)) as Lane;
            hi[l] = ((ql[l] >> 4) | (((qh[l] >> (2 * j + 1)) & 1) << 4)) as Lane;
        }
    }
}

fn unpack_q6k(block: &[u8], q: &mut [Lane]) {
    for n in 0..2 {
        let ql: &[u8; 64] = block[n * 64..n * 64 + 64].try_into().unwrap();
        let qh: &[u8; 32] = block[128 + n * 32..128 + n * 32 + 32].try_into().unwrap();
        let out = &mut q[n * 128..n * 128 + 128];
        for l in 0..32 {
            out[l] = (((ql[l] & 0xf) | ((qh[l] & 3) << 4)) as i8 - 32) as Lane;
            out[l + 32] = (((ql[l + 32] & 0xf) | (((qh[l] >> 2) & 3) << 4)) as i8 - 32) as Lane;
            out[l + 64] = (((ql[l] >> 4) | (((qh[l] >> 4) & 3) << 4)) as i8 - 32) as Lane;
            out[l + 96] = (((ql[l + 32] >> 4) | (((qh[l] >> 6) & 3) << 4)) as i8 - 32) as Lane;
        }
    }
}

/// Converts one row to floats (used for embedding lookups and tests).
pub fn dequantize_row(kind: Kind, row: &[u8], out: &mut [f32]) {
    match kind {
        Kind::F32 => {
            for (o, b) in out.iter_mut().zip(row.chunks_exact(4)) {
                *o = f32::from_le_bytes(b.try_into().unwrap());
            }
        }
        Kind::F16 => {
            for (o, b) in out.iter_mut().zip(row.chunks_exact(2)) {
                *o = f16_to_f32(u16::from_le_bytes([b[0], b[1]]));
            }
        }
        Kind::BF16 => {
            for (o, b) in out.iter_mut().zip(row.chunks_exact(2)) {
                *o = f32::from_bits((u16::from_le_bytes([b[0], b[1]]) as u32) << 16);
            }
        }
        _ => {
            let mut unpacked = Unpacked::default();
            unpacked.unpack(kind, row, out.len());
            for (i, o) in out.iter_mut().enumerate() {
                *o = unpacked.value(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32_to_f16(v: f32) -> u16 {
        // Enough for the test values: normal numbers without rounding concerns.
        let bits = v.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
        let mantissa = ((bits >> 13) & 0x3ff) as u16;
        if v == 0.0 {
            sign
        } else {
            sign | ((exponent as u16) << 10) | mantissa
        }
    }

    #[test]
    fn half_precision() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(f16_to_f32(0x7bff), 65504.0);
        assert!(f16_to_f32(0x7c00).is_infinite());
        assert_eq!(f16_to_f32(f32_to_f16(0.15625)), 0.15625);
    }

    /// A simple pseudo-random byte stream, so the tests need no fixtures.
    fn bytes(n: usize, seed: u32) -> Vec<u8> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    fn set_half(block: &mut [u8], at: usize, v: f32) {
        block[at..at + 2].copy_from_slice(&f32_to_f16(v).to_le_bytes());
    }

    /// Straight transcriptions of ggml's `dequantize_row_*` reference code.
    fn reference(kind: Kind, block: &[u8]) -> Vec<f32> {
        let mut y = Vec::new();
        match kind {
            Kind::Q4K | Kind::Q5K => {
                let (d, dmin) = (half(block, 0), half(block, 2));
                let scales = &block[4..16];
                let (qh, mut ql) = if kind == Kind::Q5K { (&block[16..48], &block[48..]) } else { (&block[0..0], &block[16..]) };
                let (mut is, mut u1, mut u2) = (0, 1u8, 2u8);
                for _ in 0..4 {
                    let (sc, m) = scale_min_k4(is, scales);
                    let (d1, m1) = (d * sc as f32, dmin * m as f32);
                    let (sc, m) = scale_min_k4(is + 1, scales);
                    let (d2, m2) = (d * sc as f32, dmin * m as f32);
                    for l in 0..32 {
                        let high = if kind == Kind::Q5K && qh[l] & u1 != 0 { 16.0 } else { 0.0 };
                        y.push(d1 * ((ql[l] & 0xf) as f32 + high) - m1);
                    }
                    for l in 0..32 {
                        let high = if kind == Kind::Q5K && qh[l] & u2 != 0 { 16.0 } else { 0.0 };
                        y.push(d2 * ((ql[l] >> 4) as f32 + high) - m2);
                    }
                    ql = &ql[32..];
                    is += 2;
                    u1 <<= 2;
                    u2 <<= 2;
                }
            }
            Kind::Q6K => {
                let d = half(block, 208);
                y.resize(256, 0.0);
                for n in 0..2 {
                    let (ql, qh, sc) = (&block[n * 64..], &block[128 + n * 32..], &block[192 + n * 8..]);
                    for l in 0..32 {
                        let is = l / 16;
                        let q1 = ((ql[l] & 0xf) | ((qh[l] & 3) << 4)) as i32 - 32;
                        let q2 = ((ql[l + 32] & 0xf) | (((qh[l] >> 2) & 3) << 4)) as i32 - 32;
                        let q3 = ((ql[l] >> 4) | (((qh[l] >> 4) & 3) << 4)) as i32 - 32;
                        let q4 = ((ql[l + 32] >> 4) | (((qh[l] >> 6) & 3) << 4)) as i32 - 32;
                        let s = |i: usize| sc[i] as i8 as f32;
                        y[n * 128 + l] = d * s(is) * q1 as f32;
                        y[n * 128 + l + 32] = d * s(is + 2) * q2 as f32;
                        y[n * 128 + l + 64] = d * s(is + 4) * q3 as f32;
                        y[n * 128 + l + 96] = d * s(is + 6) * q4 as f32;
                    }
                }
            }
            Kind::Q8_0 => y.extend(block[2..34].iter().map(|&q| half(block, 0) * q as i8 as f32)),
            Kind::Q4_0 => {
                let d = half(block, 0);
                y.resize(32, 0.0);
                for j in 0..16 {
                    y[j] = ((block[2 + j] & 0xf) as f32 - 8.0) * d;
                    y[j + 16] = ((block[2 + j] >> 4) as f32 - 8.0) * d;
                }
            }
            _ => unreachable!(),
        }
        y
    }

    #[test]
    fn unpacking_matches_ggml_reference() {
        for kind in [Kind::Q4_0, Kind::Q8_0, Kind::Q4K, Kind::Q5K, Kind::Q6K] {
            for seed in 1..20 {
                let mut block = bytes(kind.block_bytes(), seed);
                let d_at = if kind == Kind::Q6K { 208 } else { 0 };
                set_half(&mut block, d_at, 0.01 * seed as f32);
                if matches!(kind, Kind::Q4K | Kind::Q5K) {
                    set_half(&mut block, 2, 0.003 * seed as f32);
                }
                let expected = reference(kind, &block);
                let mut got = vec![0.0; kind.block_elements()];
                dequantize_row(kind, &block, &mut got);
                for (i, (a, b)) in got.iter().zip(&expected).enumerate() {
                    assert!((a - b).abs() <= 1e-5 * b.abs().max(1.0), "{kind:?} seed {seed} [{i}]: {a} vs {b}");
                }
            }
        }
    }

    #[test]
    fn quantized_dot_is_close_to_float_dot() {
        for kind in [Kind::Q4_0, Kind::Q8_0, Kind::Q4K, Kind::Q5K, Kind::Q6K] {
            let columns = 512;
            let mut row = Vec::new();
            for b in 0..columns / kind.block_elements() {
                let mut block = bytes(kind.block_bytes(), 100 + b as u32);
                set_half(&mut block, if kind == Kind::Q6K { 208 } else { 0 }, 0.02);
                if matches!(kind, Kind::Q4K | Kind::Q5K) {
                    set_half(&mut block, 2, 0.01);
                }
                row.extend(block);
            }
            let x: Vec<f32> = bytes(columns, 7).iter().map(|&b| (b as f32 - 128.0) / 50.0).collect();
            let mut w = vec![0.0; columns];
            dequantize_row(kind, &row, &mut w);
            let exact: f32 = w.iter().zip(&x).map(|(a, b)| a * b).sum();
            let mut unpacked = Unpacked::default();
            unpacked.unpack(kind, &row, columns);
            let got = unpacked.dot(&Q8::quantize(&x), &x);
            let magnitude: f32 = w.iter().zip(&x).map(|(a, b)| (a * b).abs()).sum();
            assert!((got - exact).abs() < 0.01 * magnitude, "{kind:?}: {got} vs {exact} (magnitude {magnitude})");
        }
    }
}
