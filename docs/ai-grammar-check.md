# On-device AI grammar check

Both editions can proofread text in **English, Mandarin and Malay** with a small
language model that runs entirely on the device. Nothing is sent anywhere.

## Android

The check is **off by default** and never runs while you type.

1. Open **Min Keyboard** settings → **AI grammar check ›** and tap **Check this
   device**. It shows how much RAM and how many cores the AI may use, and which
   model fits.
2. Tap **Download …** to get the model file in your browser. Min Keyboard itself
   has no internet permission. Then tap **Install model file (.gguf)…** and pick
   the downloaded file. It is checked, copied into the app and prepared for each
   language, so the first check starts quickly.
3. Turn on **Show the check button when text is selected**.
4. In any text field, select some text. **✓ Check grammar** appears above the
   keys. Tap it to see the suggested changes: added words in green, removed words
   struck through in red. Tap **Replace** to put the corrected text in place of
   the selection, or **Close** to keep it as it is.

**Test speed** in the settings runs one real check, so you can see how long
checks take on your phone. A model installed by version 0.2.0 is still used.

The model runs in plain Rust (the `grammar/` crate, no third-party code). That
crate reads the GGUF file, tokenizes the text with the model's own vocabulary and
runs the Qwen transformer on the CPU. It supports the Q4_K, Q5_K, Q6_K, Q8_0,
Q4_0 and float formats that Q4_K_M files use. To try a model on a desktop, run:

```bash
cargo run --release -p min-keyboard-grammar --example check -- model.gguf "he go to school yesterday"
```

## Linux

Setup and usage are in [linux/README.md](../linux/README.md). The Linux edition
runs the same models through `llama-cpp-python`.

## Choosing a model

Both editions check the model against the device before loading it:

| Rule | Why |
| --- | --- |
| The model may use at most **¼ of total RAM** | Leaves room for the system and other open apps |
| When loading, it may use at most **½ of the RAM that is currently free** | If other apps are using the memory, the check waits instead of pushing them out |
| At most **half the CPU cores** (4 at most, 1 in battery saver) | Other programs stay responsive |

Estimated memory use is the file size plus about 300 MB for the context and
working buffers.

| Model | File | Needs RAM | Quality |
| --- | --- | --- | --- |
| [Qwen3 1.7B Q4_K_M](https://huggingface.co/unsloth/Qwen3-1.7B-GGUF) | 1.1 GB | ~1.4 GB | Recommended; fixes most everyday mistakes in all three languages |
| [Qwen3 0.6B Q4_K_M](https://huggingface.co/unsloth/Qwen3-0.6B-GGUF) | 0.4 GB | ~0.7 GB | Basic; misses many mistakes and sometimes suggests wrong ones |

Both models are Apache-2.0. Other Qwen3 or Qwen2.5 GGUF models work too, within
the same budget, if their layer widths are multiples of 256.

## Keeping resource use low

- The model is memory-mapped and loads only when you ask for a check. It **unloads
  after 60 seconds idle**, and at once when Android reports low memory.
- The check runs at background priority. On Android it refuses to start while the
  phone is too hot.
- Prompts include one worked example per language, because small models follow
  an example more reliably than instructions alone. Qwen3's thinking mode is
  turned off.
- On Android, the fixed part of each language's prompt (instructions and
  example) is processed once, right after the model is installed, and kept in the
  app's cache (about 22 MB per language). A check then only has to read the
  selected text. If Android clears the cache, the next check rebuilds it.
- A correction mostly repeats the original text, so while answering, the check
  guesses the next few tokens from the selection and confirms them all in one
  pass of the model instead of one pass per token. The answer is exactly the same;
  on lightly edited text it needs several times fewer passes.
- A selection is limited to about 384 tokens (a few sentences).
