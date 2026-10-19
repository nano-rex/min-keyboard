// On-device grammar checking with llama.cpp. Kept free of JNI so it can be tested on a desktop.
#pragma once

#include <atomic>
#include <functional>
#include <string>

struct llama_model;
struct llama_context;

namespace minkb {

struct LoadOptions {
    int threads = 2;        // generation threads; kept low so other apps stay responsive
    int context = 1024;     // tokens of prompt + answer; bounds the KV-cache memory
};

class GrammarCore {
public:
    GrammarCore() = default;
    ~GrammarCore();
    GrammarCore(const GrammarCore &) = delete;
    GrammarCore &operator=(const GrammarCore &) = delete;

    // Returns an empty string on success, otherwise an error message.
    std::string load(const std::string &path, const LoadOptions &options);
    void unload();
    bool loaded() const { return model_ != nullptr; }

    // Bytes used by the model weights (0 when not loaded).
    unsigned long long modelBytes() const;

    // Proofreads `text` written in `language` ("English", "Mandarin Chinese", "Malay").
    // `onPartial` receives the answer so far; `cancel` stops generation early.
    // Returns the corrected text, or a message starting with "error:".
    std::string check(const std::string &text, const std::string &language,
                      const std::function<void(const std::string &)> &onPartial,
                      const std::atomic<bool> &cancel);

    // Exposed for tests: builds the chat prompt for the loaded model.
    std::string buildPrompt(const std::string &text, const std::string &language) const;

    // Exposed for tests: strips thinking blocks, code fences and wrapping quotes.
    static std::string cleanAnswer(const std::string &raw);

private:
    llama_model *model_ = nullptr;
    llama_context *context_ = nullptr;
    LoadOptions options_;
};

}  // namespace minkb
