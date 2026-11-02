//! Reads GGUF model files (the format llama.cpp uses). The file is memory-mapped read-only, so
//! the weights are paged in on demand and the system can drop them again under memory pressure.

use std::collections::HashMap;
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

use crate::quant::Kind;

#[cfg(target_pointer_width = "64")]
type OffT = i64;
#[cfg(not(target_pointer_width = "64"))]
type OffT = i32;

extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, offset: OffT) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> i32;
}

const PROT_READ: i32 = 1;
const MAP_PRIVATE: i32 = 2;
const MAP_FAILED: *mut c_void = !0usize as *mut c_void;

/// A read-only memory mapping of a whole file.
pub struct Mapping {
    ptr: *const u8,
    len: usize,
}

// The mapping is read-only and lives until drop, so sharing it between threads is fine.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    pub fn open(path: &Path) -> io::Result<Mapping> {
        let file = File::open(path)?;
        let len = usize::try_from(file.metadata()?.len()).map_err(|_| io::Error::other("file too large for this device"))?;
        if len == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "empty file"));
        }
        // SAFETY: a fresh private read-only mapping of an open file; checked for failure below.
        let ptr = unsafe { mmap(std::ptr::null_mut(), len, PROT_READ, MAP_PRIVATE, file.as_raw_fd(), 0) };
        if ptr == MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Mapping { ptr: ptr as *const u8, len })
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: the mapping is `len` bytes long and stays mapped while `self` lives.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: unmapping exactly what `open` mapped.
        unsafe { munmap(self.ptr as *mut c_void, self.len) };
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Array(Vec<Value>),
}

impl Value {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TensorInfo {
    pub kind: Kind,
    /// Dimensions, innermost first (GGUF's `ne`): a weight matrix is `[columns, rows]`.
    pub dims: Vec<usize>,
    /// Byte offset from the start of the file.
    pub offset: usize,
    pub size: usize,
}

pub struct Gguf {
    pub metadata: HashMap<String, Value>,
    pub tensors: HashMap<String, TensorInfo>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or_else(|| invalid("truncated model file"))?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn len(&mut self) -> io::Result<usize> {
        let n = self.u64()?;
        // Every element takes at least one byte, so a length beyond the file is corrupt.
        usize::try_from(n).ok().filter(|&n| n <= self.data.len()).ok_or_else(|| invalid("bad length in model file"))
    }

    fn string(&mut self) -> io::Result<String> {
        let n = self.len()?;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn value(&mut self, kind: u32) -> io::Result<Value> {
        let int = |b: &[u8]| -> i64 {
            match b.len() {
                1 => b[0] as i64,
                2 => u16::from_le_bytes([b[0], b[1]]) as i64,
                4 => u32::from_le_bytes(b.try_into().unwrap()) as i64,
                _ => i64::from_le_bytes(b.try_into().unwrap()),
            }
        };
        Ok(match kind {
            0 => Value::Int(int(self.take(1)?)),
            1 => Value::Int(self.take(1)?[0] as i8 as i64),
            2 => Value::Int(int(self.take(2)?)),
            3 => Value::Int(i16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            4 => Value::Int(int(self.take(4)?)),
            5 => Value::Int(i32::from_le_bytes(self.take(4)?.try_into().unwrap()) as i64),
            6 => Value::Float(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) as f64),
            7 => Value::Bool(self.take(1)?[0] != 0),
            8 => Value::Str(self.string()?),
            9 => {
                let inner = self.u32()?;
                let n = self.len()?;
                let mut items = Vec::with_capacity(n.min(1 << 20));
                for _ in 0..n {
                    items.push(self.value(inner)?);
                }
                Value::Array(items)
            }
            10 => Value::Int(self.u64()? as i64),
            11 => Value::Int(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            12 => Value::Float(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            other => return Err(invalid(format!("unknown metadata type {other}"))),
        })
    }
}

impl Gguf {
    pub fn parse(data: &[u8]) -> io::Result<Gguf> {
        let mut r = Reader { data, pos: 0 };
        if r.take(4)? != b"GGUF" {
            return Err(invalid("This is not a GGUF model file."));
        }
        let version = r.u32()?;
        if !(2..=3).contains(&version) {
            return Err(invalid(format!("unsupported GGUF version {version}")));
        }
        let tensor_count = r.len()?;
        let kv_count = r.len()?;
        let mut metadata = HashMap::new();
        for _ in 0..kv_count {
            let key = r.string()?;
            let kind = r.u32()?;
            metadata.insert(key, r.value(kind)?);
        }
        let alignment = metadata.get("general.alignment").and_then(Value::as_int).unwrap_or(32).max(1) as usize;
        let mut infos = Vec::with_capacity(tensor_count);
        for _ in 0..tensor_count {
            let name = r.string()?;
            let n_dims = r.u32()? as usize;
            if n_dims > 4 {
                return Err(invalid(format!("tensor {name} has {n_dims} dimensions")));
            }
            let dims = (0..n_dims).map(|_| r.len()).collect::<io::Result<Vec<_>>>()?;
            let kind_id = r.u32()?;
            let offset = r.len()?;
            infos.push((name, dims, kind_id, offset));
        }
        let data_start = r.pos.div_ceil(alignment) * alignment;
        let mut tensors = HashMap::new();
        for (name, dims, kind_id, offset) in infos {
            // Tensors of unsupported types are recorded only if the model never uses them.
            let Some(kind) = Kind::from_ggml(kind_id) else { continue };
            let elements: usize = dims.iter().product();
            if dims.first().is_none_or(|&d| d % kind.block_elements() != 0) {
                return Err(invalid(format!("tensor {name} does not fit its block size")));
            }
            let size = elements / kind.block_elements() * kind.block_bytes();
            let offset = data_start + offset;
            if offset.checked_add(size).is_none_or(|end| end > data.len()) {
                return Err(invalid(format!("tensor {name} lies outside the file")));
            }
            tensors.insert(name, TensorInfo { kind, dims, offset, size });
        }
        Ok(Gguf { metadata, tensors })
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
}
