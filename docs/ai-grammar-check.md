# On-device AI grammar check (Linux edition)

The Linux edition can proofread what you are writing in **English, Mandarin
and Malay** with a small language model that runs entirely on your computer.
Setup and usage are in [linux/README.md](../linux/README.md); this document
covers the models and the resource budget.

The Android app had the same check in version 0.2.0, built on llama.cpp. The
0.3.0 rewrite in Rust leaves it out for now, because it would bring back a C++
dependency.

## Choosing a model

`linux/grammar.py` checks the model against the machine before loading it:

| Rule | Why |
| --- | --- |
| The model may use at most **¼ of total RAM** | Leaves room for the system and other open apps |
| When loading, it may use at most **½ of the RAM that is currently free** | If other apps are using the memory, the check waits instead of pushing them out |
| At most **half the CPU cores** | Other programs stay responsive |

Estimated memory use is the file size plus about 300 MB for the context and
working buffers. Desktop measurements were about 170 MB above the file size,
so the estimate leaves a margin.

| Model | File | Needs RAM | Quality |
| --- | --- | --- | --- |
| [Qwen3 1.7B Q4_K_M](https://huggingface.co/unsloth/Qwen3-1.7B-GGUF) | 1.1 GB | ~1.3 GB | Recommended; fixes most everyday mistakes in all three languages |
| [Qwen3 0.6B Q4_K_M](https://huggingface.co/unsloth/Qwen3-0.6B-GGUF) | 0.4 GB | ~0.7 GB | Basic; misses many mistakes and sometimes suggests wrong ones |

Both models are Apache-2.0. Other GGUF models work too, within the same budget.

## Keeping resource use low

- The model loads only when you ask for a check and **unloads after 60
  seconds idle**.
- Prompts include one worked example per language, because small models follow
  an example more reliably than instructions alone. Qwen3's thinking mode is
  turned off.
