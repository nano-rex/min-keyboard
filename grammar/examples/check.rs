//! Proofreads text from the command line, for trying models on a desktop:
//! `cargo run --release -p min-keyboard-grammar --example check -- model.gguf "he go to school yesterday"`
//! Set `GRAMMAR_CACHE_DIR` to keep the processed fixed part of the prompt between runs.

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(model), Some(text)) = (args.next(), args.next()) else {
        eprintln!("usage: check <model.gguf> <text> [threads]");
        std::process::exit(2);
    };
    let threads = args.next().and_then(|t| t.parse().ok()).unwrap_or_else(grammar::default_threads);
    let started = Instant::now();
    let mut checker = match grammar::Checker::load(&PathBuf::from(model), threads) {
        Ok(checker) => checker.with_cache_dir(std::env::var_os("GRAMMAR_CACHE_DIR").map(PathBuf::from)),
        Err(e) => {
            eprintln!("cannot load the model: {e}");
            std::process::exit(1);
        }
    };
    let loaded = started.elapsed();
    let language = grammar::detect_language(&text, false);
    let mut tokens = 0usize;
    let mut first: Option<Instant> = None;
    let started = Instant::now();
    let result = checker.check(
        &text,
        language,
        &mut |_| {
            tokens += 1;
            first.get_or_insert_with(Instant::now);
        },
        &|| false,
    );
    let total = started.elapsed();
    match result {
        Ok(answer) => {
            println!("{answer}");
            let prompt = first.map(|f| f - started).unwrap_or(total);
            let generation = total - prompt;
            eprintln!(
                "{} · {language:?} · {threads} threads · load {:.2}s · prompt {:.2}s · {tokens} tokens in {:.2}s ({:.1}/s)",
                checker.model_name(),
                loaded.as_secs_f32(),
                prompt.as_secs_f32(),
                generation.as_secs_f32(),
                tokens.saturating_sub(1) as f32 / generation.as_secs_f32().max(1e-3)
            );
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
