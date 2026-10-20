# On-device AI grammar check

Min Keyboard can proofread the sentence you are writing in **English, Mandarin
and Malay** with a small language model that runs entirely on the device. The
app has no internet permission. You download the model file yourself and
install it from the settings screen.

## Using it

1. Open **Min Keyboard → AI grammar check**. The device check runs at once and
   tells you which model fits this phone, or that none does.
2. Tap **Download …** to fetch the recommended file in your browser, then
   **Install model file** and pick the downloaded `.gguf`.
3. While typing, tap **✓** above the keys. Min Keyboard checks the selected
   text, or the current paragraph if nothing is selected. Changes are shown
   with removed words struck through and new words in bold. **Replace** applies
   them; nothing changes until you tap it.

**Test speed** in the settings runs one real check, so you can see how long
checks take on your phone.

## Choosing a model: the device check

`ModelAdvisor` makes the decision from facts that `DeviceProfiler` measures:
RAM, free RAM, CPU cores and architecture, free storage, battery saver and
temperature.

| Rule | Why |
| --- | --- |
| The model may use at most **¼ of total RAM** | Leaves room for the system and other open apps |
| When loading, it may use at most **½ of the RAM that is currently free** | If other apps are using the memory, the check waits instead of pushing them out |
| At most **half the CPU cores** (max 4), at background priority; one core with battery saver on | Foreground apps stay responsive |
| A 64-bit CPU and at least 4 cores | Older devices would take minutes per sentence |
| No checks while the phone is hot (thermal status severe or worse) | Avoids adding heat |

Estimated memory use is the file size plus about 300 MB for the context and
working buffers. Desktop measurements were about 170 MB above the file size,
so the estimate leaves a margin.

| Model | File | Needs RAM | Fits phones with | Quality |
| --- | --- | --- | --- | --- |
| [Qwen3 1.7B Q4_K_M](https://huggingface.co/unsloth/Qwen3-1.7B-GGUF) | 1.1 GB | ~1.3 GB | 6 GB RAM or more | Recommended; fixes most everyday mistakes in all three languages |
| [Qwen3 0.6B Q4_K_M](https://huggingface.co/unsloth/Qwen3-0.6B-GGUF) | 0.4 GB | ~0.7 GB | 3–5 GB RAM | Basic; misses many mistakes and sometimes suggests wrong ones |
| none | | | under 3 GB, 32-bit CPU, fewer than 4 cores | The keyboard works normally without the AI |

Both models are Apache-2.0. You can install another GGUF model. Min Keyboard
checks its size against the same budget before copying it, and recognises the
catalogue models by their SHA-256 checksum.

## Keeping resource use low while other apps run

- Model weights are **memory-mapped and never locked**, so Android can reclaim
  those pages under pressure.
- The model loads only when you tap ✓. It **unloads after 60 seconds idle**,
  and at once when Android signals memory pressure (`onTrimMemory`,
  `onLowMemory`). Closing the check panel stops a check that is still running.
- The context is limited to 1,024 tokens, which keeps the KV cache at about
  120 MB. Longer text is trimmed to its last sentences.

## Engine

The native code is [llama.cpp](https://github.com/ggml-org/llama.cpp), pinned
to release `b11327` by URL and SHA-256 in `android/app/src/main/cpp/llama.cmake`.
`grammar_core.cpp` holds the prompt and generation loop, and `grammar_jni.cpp`
is the thin JNI bridge. Prompts include one worked example per language,
because small models follow an example more reliably than instructions alone.
Qwen3's thinking mode is turned off.

### Trying prompts and models on a desktop

```bash
cmake -S tools/grammar_cli -B build/grammar_cli -DCMAKE_BUILD_TYPE=Release
cmake --build build/grammar_cli -j
build/grammar_cli/grammar_cli Qwen3-1.7B-Q4_K_M.gguf English "She go to school yesterday." 2
```

`--batch FILE` runs a list of `LANGUAGE<TAB>TEXT` lines and prints peak memory
use. The same prompts are used by the Linux edition (`linux/grammar.py`).
