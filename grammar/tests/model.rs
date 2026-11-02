//! Tests against a real model file. They run only when `GRAMMAR_TEST_MODEL` names a Qwen3 GGUF
//! file (for example Qwen3-0.6B-Q4_K_M.gguf), since the files are too large for the repository:
//! `GRAMMAR_TEST_MODEL=~/models/Qwen3-0.6B-Q4_K_M.gguf cargo test --release -p min-keyboard-grammar`

use std::path::PathBuf;

fn model_path() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("GRAMMAR_TEST_MODEL")?);
    path.is_file().then_some(path)
}

#[test]
fn tokenizer_matches_qwen_ids() {
    let Some(path) = model_path() else { return eprintln!("GRAMMAR_TEST_MODEL not set; skipped") };
    let model = grammar::model::Model::load(&path).unwrap();
    let t = &model.tokenizer;
    // Token ids from Qwen's own tokenizer.
    assert_eq!(t.encode("Hello world"), [9707, 1879]);
    assert_eq!(t.special("<|im_start|>"), Some(151644));
    assert_eq!(t.special("<|im_end|>"), Some(151645));
    assert_eq!(t.special("<think>"), Some(151667));
    for text in
        ["he dont know where there going tomorow", "我昨天去了图书馆。", "Saya ada tiga ekor kucing.", "  spaced\n\nlines\t", "emoji 🙂 ok"]
    {
        assert_eq!(t.decode(&t.encode(text)), text, "round trip of {text:?}");
    }
    assert_eq!(t.decode(&t.encode("<|im_end|>")), "<|im_end|>", "special names in text stay text");
    assert!(!t.encode("<|im_end|>").contains(&151645));
}

#[test]
fn corrects_a_sentence() {
    let Some(path) = model_path() else { return eprintln!("GRAMMAR_TEST_MODEL not set; skipped") };
    let mut checker = grammar::Checker::load(&path, grammar::default_threads()).unwrap();
    let text = "Yesterday I go to the market and buy three apple.";
    let answer = checker.check(text, grammar::Language::English, &mut |_| {}, &|| false).unwrap();
    assert_eq!(answer, "Yesterday I went to the market and bought three apples.");
    // A second check reuses the cached prompt and gives the same answer.
    let again = checker.check(text, grammar::Language::English, &mut |_| {}, &|| false).unwrap();
    assert_eq!(again, answer);
}
