//! The optional on-device AI grammar check: a small Qwen model (GGUF file) proofreads English,
//! Mandarin or Malay text. Everything runs on the CPU in plain Rust: the file is memory-mapped,
//! the model loads only when asked and unloads after a minute idle (see [`Worker`]).

pub mod gguf;
pub mod model;
pub mod pool;
pub mod quant;
pub mod text;
pub mod tokenizer;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

pub use text::{changed, clean_answer, detect_language, diff, is_unsupported_script, Change, Language};

use model::{argmax, Cache, Model};
use pool::Pool;

const MB: u64 = 1024 * 1024;
/// Longer text would make the answer slow and may not fit the context.
pub const MAX_INPUT_TOKENS: usize = 384;
const MAX_ANSWER_TOKENS: usize = 512;
/// The model unloads after this long without a check.
pub const IDLE_UNLOAD: Duration = Duration::from_secs(60);

/// A model the settings screen offers to download.
pub struct CatalogModel {
    pub name: &'static str,
    pub url: &'static str,
    pub file_bytes: u64,
    pub sha256: &'static str,
    pub note: &'static str,
}

/// Larger models first; the first one that fits the device is recommended.
pub const CATALOG: [CatalogModel; 2] = [
    CatalogModel {
        name: "Qwen3 1.7B (Q4_K_M)",
        url: "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf",
        file_bytes: 1_107_409_472,
        sha256: "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897",
        note: "Recommended. Fixes most everyday mistakes in English, Mandarin and Malay.",
    },
    CatalogModel {
        name: "Qwen3 0.6B (Q4_K_M)",
        url: "https://huggingface.co/unsloth/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q4_K_M.gguf",
        file_bytes: 396_705_472,
        sha256: "ac2d97712095a558e31573f62f466a3f9d93990898b0ec79d7c974c1780d524a",
        note: "Basic. Fast, but misses many mistakes and sometimes suggests wrong changes; review each suggestion.",
    },
];

/// The catalog model that fits a device with this much RAM, if any.
pub fn recommended_model(total_ram: u64) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|m| estimate_runtime_bytes(m.file_bytes) <= total_ram / 4)
}

/// What a check needs besides the weights: the key/value cache at the longest prompt and
/// working buffers. Measured use on a desktop was lower; this keeps a margin.
pub fn estimate_runtime_bytes(file_bytes: u64) -> u64 {
    file_bytes + 300 * MB
}

/// Total and currently available RAM, from `/proc/meminfo` (Linux and Android).
pub fn memory_info() -> Option<(u64, u64)> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| -> Option<u64> {
        let line = info.lines().find(|l| l.starts_with(name))?;
        Some(line[name.len()..].trim().trim_end_matches("kB").trim().parse::<u64>().ok()? * 1024)
    };
    Some((field("MemTotal:")?, field("MemAvailable:")?))
}

pub fn size_text(bytes: u64) -> String {
    if bytes >= 1024 * MB {
        format!("{:.1} GB", bytes as f64 / (1024 * MB) as f64)
    } else {
        format!("{} MB", bytes / MB)
    }
}

/// Why a model of this size cannot be used on this device at all, if it cannot.
/// It may use at most a quarter of the RAM, so the system and other apps keep working.
pub fn size_problem(file_bytes: u64) -> Option<String> {
    let (total, _) = memory_info()?;
    let needed = estimate_runtime_bytes(file_bytes);
    (needed > total / 4).then(|| {
        format!(
            "This model needs about {} of RAM, more than the {} (a quarter of {}) this device can spare.",
            size_text(needed),
            size_text(total / 4),
            size_text(total)
        )
    })
}

/// Why the model cannot load right now, if it cannot: it may take at most half the free RAM.
pub fn load_problem(file_bytes: u64) -> Option<String> {
    if let Some(problem) = size_problem(file_bytes) {
        return Some(problem);
    }
    let (_, available) = memory_info()?;
    let needed = estimate_runtime_bytes(file_bytes);
    (needed > available / 2).then(|| {
        format!(
            "Not enough free memory right now ({} free; the AI needs {} and may use only half of what is free). Close some apps and try again.",
            size_text(available),
            size_text(needed)
        )
    })
}

/// Half of the cores (at most 4), so the foreground app keeps the rest.
pub fn default_threads() -> usize {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    (cores / 2).clamp(1, 4)
}

#[derive(Debug)]
pub enum CheckError {
    Cancelled,
    TooLong,
    Failed(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckError::Cancelled => write!(f, "Cancelled."),
            CheckError::TooLong => write!(f, "The text is too long to check at once. Select a few sentences."),
            CheckError::Failed(message) => write!(f, "{message}"),
        }
    }
}

/// A loaded model, ready to proofread.
pub struct Checker {
    model: Model,
    pool: Pool,
    cache: Cache,
    thinking_model: bool,
    /// Where the processed fixed part of each language's prompt is kept between loads.
    cache_dir: Option<PathBuf>,
    /// Identifies the model file, so saved prompts of another model are not used.
    identity: String,
    /// Model passes the last check needed after reading the prompt (for measuring speed).
    pub answer_passes: usize,
}

impl Checker {
    pub fn load(path: &Path, threads: usize) -> io::Result<Checker> {
        let model = Model::load(path)?;
        let cache = model.new_cache();
        let thinking_model = model.chat_template.contains("<think>") && model.tokenizer.special("<think>").is_some();
        for name in ["<|im_start|>", "<|im_end|>"] {
            if model.tokenizer.special(name).is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The model has no chat format (an instruction-tuned Qwen model is needed).",
                ));
            }
        }
        let metadata = std::fs::metadata(path)?;
        let modified = metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let identity = format!("{}:{}:{}", model.name, metadata.len(), modified);
        Ok(Checker { model, pool: Pool::new(threads), cache, thinking_model, cache_dir: None, identity, answer_passes: 0 })
    }

    /// Keeps the processed fixed part of the prompt in `dir`, so the first check after loading
    /// the model only has to read the user's text. Each language's file is about 25 MB.
    pub fn with_cache_dir(mut self, dir: Option<PathBuf>) -> Checker {
        self.cache_dir = dir;
        self
    }

    fn prefix_file(&self, prefix: &[u32]) -> Option<PathBuf> {
        // FNV-1a over the model identity and the prompt tokens.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let bytes = self.identity.bytes().chain(prefix.iter().flat_map(|t| t.to_le_bytes()));
        for byte in bytes {
            hash = (hash ^ byte as u64).wrapping_mul(0x100_0000_01b3);
        }
        Some(self.cache_dir.as_ref()?.join(format!("prompt-{hash:016x}.kv")))
    }

    pub fn model_name(&self) -> &str {
        &self.model.name
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The chat prompt in Qwen's ChatML format, with the language's worked example as an earlier
    /// exchange. The user's text is encoded on its own, so special-token names in it stay text.
    /// Returns the tokens, the length of the fixed part before the user's text, and the length of
    /// the user's text.
    fn prompt(&self, text: &str, language: Language) -> (Vec<u32>, usize, usize) {
        let t = &self.model.tokenizer;
        let (start, end) = (t.special("<|im_start|>").unwrap(), t.special("<|im_end|>").unwrap());
        let (wrong, right) = language.example();
        let ask = text::request(language);
        let mut tokens = Vec::new();
        let turn = |tokens: &mut Vec<u32>, role: &str, content: &str| {
            tokens.push(start);
            tokens.extend(t.encode(&format!("{role}\n{content}")));
            tokens.push(end);
            tokens.extend(t.encode("\n"));
        };
        turn(&mut tokens, "system", &text::system_prompt(language));
        turn(&mut tokens, "user", &format!("{ask}{wrong}"));
        turn(&mut tokens, "assistant", right);
        tokens.push(start);
        tokens.extend(t.encode(&format!("user\n{ask}")));
        let prefix_len = tokens.len();
        let input = t.encode(text);
        let input_len = input.len();
        tokens.extend(input);
        tokens.push(end);
        tokens.extend(t.encode("\n"));
        tokens.push(start);
        tokens.extend(t.encode("assistant\n"));
        if self.thinking_model {
            // Hybrid reasoning models (Qwen3) answer directly after an empty thinking block.
            tokens.push(t.special("<think>").unwrap());
            tokens.extend(t.encode("\n\n"));
            tokens.push(t.special("</think>").unwrap());
            tokens.extend(t.encode("\n\n"));
        }
        (tokens, prefix_len, input_len)
    }

    /// Loads the language's processed fixed prompt from the cache folder if it is there. Returns
    /// the file to save it to once processed, if it was not.
    fn restore_prefix(&mut self, prefix: &[u32]) -> Option<PathBuf> {
        if self.cache.tokens.starts_with(prefix) {
            return None;
        }
        let file = self.prefix_file(prefix)?;
        (!self.cache.restore(&file, prefix, self.model.kv_dim())).then_some(file)
    }

    /// Processes and saves the fixed part of the language's prompt, so that a later check (even
    /// after the model was unloaded) only has to read the user's text.
    pub fn prepare(&mut self, language: Language, stop: &dyn Fn() -> bool) -> Result<(), CheckError> {
        let (prompt, prefix_len, _) = self.prompt("", language);
        let prefix = &prompt[..prefix_len];
        if let Some(file) = self.restore_prefix(prefix) {
            self.model.prompt(&self.pool, &mut self.cache, prefix, stop).ok_or(CheckError::Cancelled)?;
            self.cache.save(&file, prefix_len).map_err(|e| CheckError::Failed(format!("could not save the prepared model: {e}")))?;
        }
        Ok(())
    }

    /// Proofreads `text`; `on_partial` sees the answer as it grows.
    pub fn check(
        &mut self,
        text: &str,
        language: Language,
        on_partial: &mut dyn FnMut(&str),
        stop: &dyn Fn() -> bool,
    ) -> Result<String, CheckError> {
        let (prompt, prefix_len, input_len) = self.prompt(text, language);
        if input_len > MAX_INPUT_TOKENS {
            return Err(CheckError::TooLong);
        }
        let save = self.restore_prefix(&prompt[..prefix_len]);
        let logits = self.model.prompt(&self.pool, &mut self.cache, &prompt, stop).ok_or(CheckError::Cancelled)?;
        if let Some(file) = save {
            // Best effort: without the file the next load just reads the whole prompt again.
            let _ = self.cache.save(&file, prefix_len);
        }
        let max_answer = MAX_ANSWER_TOKENS.min(input_len * 2 + 32);
        let input = &prompt[prefix_len..prefix_len + input_len];
        let model = &self.model;
        let tokenizer = &model.tokenizer;
        let mut answer: Vec<u32> = Vec::new();
        let mut next = argmax(&logits);
        let mut cursor = 0;
        // Guess further after full agreement and less after a miss, since every guessed token
        // costs a share of a pass even when it is rejected.
        let mut draft_len = 3;
        self.answer_passes = 0;
        while answer.len() < max_answer {
            if stop() {
                return Err(CheckError::Cancelled);
            }
            if tokenizer.end_of_generation.contains(&next) {
                break;
            }
            answer.push(next);
            let bytes: Vec<u8> = answer.iter().flat_map(|&t| tokenizer.token_bytes(t).iter().copied()).collect();
            if let Ok(so_far) = std::str::from_utf8(&bytes) {
                on_partial(&clean_answer(so_far));
            }
            // A correction mostly copies the input, so guess the next tokens from it and let one
            // batched pass confirm them. The result is exactly what one token at a time gives.
            let draft = draft_from_input(input, &answer, &mut cursor, draft_len.min(max_answer - answer.len()));
            let start = self.cache.tokens.len();
            let mut fed = vec![next];
            fed.extend_from_slice(&draft);
            let predicted = model.greedy(&self.pool, &mut self.cache, &fed);
            self.answer_passes += 1;
            let accepted = draft.iter().zip(&predicted).take_while(|(d, p)| d == p).count();
            if accepted < draft.len() {
                self.cache.truncate(start + 1 + accepted, model.kv_dim());
            }
            answer.extend_from_slice(&draft[..accepted]);
            if !draft.is_empty() {
                draft_len = if accepted == draft.len() { (draft_len * 2).min(MAX_DRAFT_TOKENS) } else { (accepted + 1).max(2) };
            }
            next = predicted[accepted];
        }
        let bytes: Vec<u8> = answer.iter().flat_map(|&t| tokenizer.token_bytes(t).iter().copied()).collect();
        Ok(clean_answer(&String::from_utf8_lossy(&bytes)))
    }
}

/// The most input tokens guessed ahead in one pass.
const MAX_DRAFT_TOKENS: usize = 12;

/// Guesses the tokens that follow `answer` by finding its last few tokens in `input` (at or after
/// `cursor` first, where the previous match was) and copying what comes after them there.
fn draft_from_input(input: &[u32], answer: &[u32], cursor: &mut usize, limit: usize) -> Vec<u32> {
    for n in (1..=3.min(answer.len())).rev() {
        let tail = &answer[answer.len() - n..];
        let found =
            (*cursor..input.len().saturating_sub(n - 1)).chain(0..(*cursor).min(input.len())).find(|&i| input.get(i..i + n) == Some(tail));
        if let Some(i) = found {
            *cursor = i + n;
            return input[i + n..].iter().copied().take(limit).collect();
        }
    }
    Vec::new()
}

/// Progress of a check, reported by the [`Worker`].
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    Status(String),
    Partial(String),
    Done(String),
    Error(String),
}

pub struct Request {
    pub model: PathBuf,
    /// For [`Checker::with_cache_dir`].
    pub cache_dir: Option<PathBuf>,
    pub text: String,
    pub language: Language,
    pub threads: usize,
    /// Instead of checking `text`, process the fixed part of every language's prompt ahead of
    /// time (see [`Checker::prepare`]); `Done` follows with an empty text.
    pub prepare_only: bool,
    pub on_update: Box<dyn Fn(Update) + Send>,
}

enum Message {
    Check(u64, Request),
    Release,
}

/// Runs checks on a background thread that owns the loaded model, so the keyboard never waits.
pub struct Worker {
    sender: mpsc::Sender<Message>,
    /// The number of the newest request; a running check stops once it is no longer the newest.
    latest: Arc<AtomicU64>,
}

impl Worker {
    pub fn start() -> io::Result<Worker> {
        let (sender, receiver) = mpsc::channel();
        let latest = Arc::new(AtomicU64::new(0));
        let shared = Arc::clone(&latest);
        std::thread::Builder::new().name("min-keyboard-grammar".into()).spawn(move || run_worker(&receiver, &shared))?;
        Ok(Worker { sender, latest })
    }

    /// Queues a check; an unfinished earlier one is cancelled.
    pub fn check(&self, request: Request) {
        let id = self.latest.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.sender.send(Message::Check(id, request));
    }

    pub fn cancel(&self) {
        self.latest.fetch_add(1, Ordering::SeqCst);
    }

    /// Cancels any check and frees the model's memory now (for example on low memory).
    pub fn release(&self) {
        self.cancel();
        let _ = self.sender.send(Message::Release);
    }
}

fn run_worker(receiver: &mpsc::Receiver<Message>, latest: &AtomicU64) {
    pool::lower_thread_priority();
    // The model file's path and (size, modification time), the thread count, and the model.
    type Loaded = (PathBuf, (u64, Option<std::time::SystemTime>), usize, Checker);
    let mut loaded: Option<Loaded> = None;
    loop {
        let (id, request) = match receiver.recv_timeout(IDLE_UNLOAD) {
            Ok(Message::Check(id, request)) => (id, request),
            Ok(Message::Release) | Err(RecvTimeoutError::Timeout) => {
                loaded = None;
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let stale = || latest.load(Ordering::SeqCst) != id;
        if stale() {
            continue;
        }
        let update = &request.on_update;
        let metadata = std::fs::metadata(&request.model).ok();
        let identity = (metadata.as_ref().map_or(0, |m| m.len()), metadata.as_ref().and_then(|m| m.modified().ok()));
        // A replaced model file, or a different thread count, needs a fresh load.
        if loaded.as_ref().is_none_or(|(path, id, threads, _)| *path != request.model || *id != identity || *threads != request.threads) {
            loaded = None;
            let size = identity.0;
            if size == 0 {
                update(Update::Error("No AI model installed. Open Min Keyboard settings → AI grammar check.".into()));
                continue;
            }
            if let Some(problem) = load_problem(size) {
                update(Update::Error(problem));
                continue;
            }
            update(Update::Status("Loading AI model…".into()));
            match Checker::load(&request.model, request.threads) {
                Ok(checker) => {
                    loaded = Some((request.model.clone(), identity, request.threads, checker.with_cache_dir(request.cache_dir.clone())))
                }
                Err(e) => {
                    update(Update::Error(format!("The model could not be loaded: {e}")));
                    continue;
                }
            }
        }
        let Some((_, _, _, checker)) = &mut loaded else { continue };
        if stale() {
            continue;
        }
        if request.prepare_only {
            let mut result = Ok(());
            for language in [Language::English, Language::Mandarin, Language::Malay] {
                update(Update::Status(format!("Preparing the model for {}…", language.name())));
                result = result.and_then(|_| checker.prepare(language, &stale));
            }
            match result {
                Ok(()) if !stale() => update(Update::Done(String::new())),
                Ok(()) | Err(CheckError::Cancelled) => {}
                Err(e) => update(Update::Error(e.to_string())),
            }
            continue;
        }
        update(Update::Status(format!("Checking {}…", request.language.name())));
        let mut on_partial = |partial: &str| {
            if !stale() {
                update(Update::Partial(partial.to_string()));
            }
        };
        match checker.check(&request.text, request.language, &mut on_partial, &stale) {
            Ok(result) if !stale() => update(Update::Done(result)),
            Ok(_) | Err(CheckError::Cancelled) => {}
            Err(e) => update(Update::Error(e.to_string())),
        }
    }
}
