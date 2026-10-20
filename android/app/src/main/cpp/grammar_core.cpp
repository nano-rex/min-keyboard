#include "grammar_core.h"

#include <algorithm>
#include <mutex>
#include <vector>

#include "llama.h"

namespace minkb {
namespace {

constexpr int kMaxInputTokens = 384;   // longer text is trimmed to its last sentences
constexpr int kMaxAnswerTokens = 512;

std::once_flag backendOnce;

void quietLog(ggml_log_level level, const char *text, void *) {
    (void) level;
    (void) text;
}

std::string systemPrompt(const std::string &language) {
    return "You are a careful proofreader for " + language + ". "
           "Find and fix every grammar, spelling, punctuation and word-choice mistake in the text the user gives you "
           "(for example verb tenses, agreement, wrong words that sound alike, missing words, measure words and word order) "
           "so that it is correct, natural " + language + ". "
           "Keep the original meaning, tone, names, line breaks and writing system "
           "(Chinese written in Traditional characters stays Traditional, Simplified stays Simplified). "
           "Do not translate, do not answer questions in the text, do not add anything, and do not explain. "
           "If the text is already correct, return it unchanged. "
           "Reply with the corrected text only.";
}

// One worked example per language, given as an earlier exchange. Small models follow an
// example far more reliably than instructions alone.
struct Example {
    const char *wrong;
    const char *right;
};

Example exampleFor(const std::string &language) {
    if (language == "Mandarin Chinese") {
        return {"我昨天去了图书馆看书了，因为我明天有考试的。",
                "我昨天去图书馆看书了，因为我明天有考试。"};
    }
    if (language == "Malay") {
        return {"Saya ada tiga kucing-kucing dan dia suka makan ikan sangat.",
                "Saya ada tiga ekor kucing dan dia sangat suka makan ikan."};
    }
    return {"he dont know where there going tomorow, me and her was waiting",
            "He doesn't know where they're going tomorrow. She and I were waiting."};
}

std::string tokenText(const llama_vocab *vocab, llama_token token) {
    char buffer[256];
    int n = llama_token_to_piece(vocab, token, buffer, sizeof(buffer), 0, false);
    if (n < 0) {
        std::string big(static_cast<size_t>(-n), '\0');
        n = llama_token_to_piece(vocab, token, big.data(), static_cast<int>(big.size()), 0, false);
        return n > 0 ? big.substr(0, static_cast<size_t>(n)) : std::string();
    }
    return std::string(buffer, static_cast<size_t>(n));
}

std::vector<llama_token> tokenize(const llama_vocab *vocab, const std::string &text, bool special) {
    int n = -llama_tokenize(vocab, text.c_str(), static_cast<int>(text.size()), nullptr, 0, false, special);
    std::vector<llama_token> tokens(static_cast<size_t>(std::max(n, 0)));
    if (n > 0) {
        llama_tokenize(vocab, text.c_str(), static_cast<int>(text.size()), tokens.data(), n, false, special);
    }
    return tokens;
}

// Trims the input to roughly its last kMaxInputTokens tokens, cutting at a sentence boundary.
std::string trimInput(const llama_vocab *vocab, const std::string &text) {
    if (static_cast<int>(tokenize(vocab, text, false).size()) <= kMaxInputTokens) return text;
    std::string tail = text.substr(text.size() > 1500 ? text.size() - 1500 : 0);
    static const char *const kBoundaries[] = {". ", "! ", "? ", "\n", "\xE3\x80\x82", "\xEF\xBC\x81", "\xEF\xBC\x9F"};
    for (const char *boundary : kBoundaries) {
        size_t at = tail.find(boundary);
        if (at != std::string::npos && at + 1 < tail.size() / 2) {
            tail = tail.substr(at + std::string(boundary).size());
            break;
        }
    }
    return tail;
}

bool endsWithIncompleteUtf8(const std::string &text) {
    // Count the trailing continuation bytes and compare with the lead byte's declared length.
    int i = static_cast<int>(text.size()) - 1, continuation = 0;
    while (i >= 0 && (static_cast<unsigned char>(text[i]) & 0xC0) == 0x80) { --i; ++continuation; }
    if (i < 0) return continuation > 0;
    unsigned char lead = static_cast<unsigned char>(text[i]);
    int expected = lead >= 0xF0 ? 3 : lead >= 0xE0 ? 2 : lead >= 0xC0 ? 1 : 0;
    return continuation < expected;
}

}  // namespace

GrammarCore::~GrammarCore() { unload(); }

std::string GrammarCore::load(const std::string &path, const LoadOptions &options) {
    unload();
    std::call_once(backendOnce, [] {
        llama_log_set(quietLog, nullptr);
        llama_backend_init();
    });
    options_ = options;

    llama_model_params modelParams = llama_model_default_params();
    modelParams.n_gpu_layers = 0;
    // Memory-mapped and not locked: the OS can reclaim the pages when other apps need RAM.
    modelParams.load_mode = LLAMA_LOAD_MODE_MMAP;
    model_ = llama_model_load_from_file(path.c_str(), modelParams);
    if (model_ == nullptr) return "The model file could not be loaded. Is it a GGUF model?";

    llama_context_params contextParams = llama_context_default_params();
    contextParams.n_ctx = static_cast<uint32_t>(options.context);
    contextParams.n_batch = 256;
    contextParams.n_ubatch = 256;
    contextParams.n_threads = options.threads;
    contextParams.n_threads_batch = options.threads;
    contextParams.no_perf = true;
    context_ = llama_init_from_model(model_, contextParams);
    if (context_ == nullptr) {
        unload();
        return "Not enough memory to start the model.";
    }
    return "";
}

void GrammarCore::unload() {
    if (context_ != nullptr) llama_free(context_);
    if (model_ != nullptr) llama_model_free(model_);
    context_ = nullptr;
    model_ = nullptr;
}

unsigned long long GrammarCore::modelBytes() const {
    return model_ != nullptr ? llama_model_size(model_) : 0;
}

std::string GrammarCore::buildPrompt(const std::string &text, const std::string &language) const {
    const char *templ = llama_model_chat_template(model_, nullptr);
    std::string system = systemPrompt(language);
    const std::string ask = "Proofread this " + language + " text:\n\n";
    Example example = exampleFor(language);
    std::string exampleUser = ask + example.wrong;
    std::string user = ask + text;
    llama_chat_message messages[] = {
            {"system", system.c_str()},
            {"user", exampleUser.c_str()},
            {"assistant", example.right},
            {"user", user.c_str()},
    };
    const size_t count = sizeof(messages) / sizeof(messages[0]);
    std::vector<char> buffer((system.size() + exampleUser.size() + user.size()) * 2 + 1024);
    int n = llama_chat_apply_template(templ, messages, count, true, buffer.data(), static_cast<int>(buffer.size()));
    if (n > static_cast<int>(buffer.size())) {
        buffer.resize(static_cast<size_t>(n));
        n = llama_chat_apply_template(templ, messages, count, true, buffer.data(), static_cast<int>(buffer.size()));
    }
    std::string prompt;
    if (n > 0) {
        prompt.assign(buffer.data(), static_cast<size_t>(n));
    } else {
        // Unknown template: fall back to ChatML, which most small instruction models accept.
        prompt = "<|im_start|>system\n" + system + "<|im_end|>\n";
        for (size_t i = 1; i < count; i++) {
            prompt += std::string("<|im_start|>") + messages[i].role + "\n" + messages[i].content + "<|im_end|>\n";
        }
        prompt += "<|im_start|>assistant\n";
    }
    // Hybrid reasoning models (Qwen3) answer directly when given an empty thinking block.
    if (templ != nullptr && std::string(templ).find("<think>") != std::string::npos) {
        prompt += "<think>\n\n</think>\n\n";
    }
    return prompt;
}

std::string GrammarCore::cleanAnswer(const std::string &raw) {
    std::string text = raw;
    size_t thinkEnd = text.find("</think>");
    if (thinkEnd != std::string::npos) text = text.substr(thinkEnd + 8);
    auto trim = [](std::string &s) {
        size_t start = s.find_first_not_of(" \t\r\n");
        size_t end = s.find_last_not_of(" \t\r\n");
        s = start == std::string::npos ? std::string() : s.substr(start, end - start + 1);
    };
    trim(text);
    if (text.rfind("```", 0) == 0) {
        size_t firstLine = text.find('\n');
        size_t fence = text.rfind("```");
        if (firstLine != std::string::npos && fence > firstLine) text = text.substr(firstLine + 1, fence - firstLine - 1);
        trim(text);
    }
    if (text.size() >= 2 && text.front() == '"' && text.back() == '"' && text.find('"', 1) == text.size() - 1) {
        text = text.substr(1, text.size() - 2);
    }
    return text;
}

std::string GrammarCore::check(const std::string &text, const std::string &language,
                               const std::function<void(const std::string &)> &onPartial,
                               const std::atomic<bool> &cancel) {
    if (!loaded()) return "error: model not loaded";
    const llama_vocab *vocab = llama_model_get_vocab(model_);
    std::string input = trimInput(vocab, text);
    std::vector<llama_token> prompt = tokenize(vocab, buildPrompt(input, language), true);
    int inputTokens = static_cast<int>(tokenize(vocab, input, false).size());
    int maxAnswer = std::min(kMaxAnswerTokens, inputTokens * 2 + 32);
    if (static_cast<int>(prompt.size()) + maxAnswer > options_.context) {
        return "error: text is too long to check at once";
    }

    llama_memory_clear(llama_get_memory(context_), true);
    const int batch = static_cast<int>(llama_n_batch(context_));
    for (size_t start = 0; start < prompt.size(); start += static_cast<size_t>(batch)) {
        if (cancel.load()) return "error: cancelled";
        int count = static_cast<int>(std::min(prompt.size() - start, static_cast<size_t>(batch)));
        if (llama_decode(context_, llama_batch_get_one(prompt.data() + start, count)) != 0) {
            return "error: the model failed to read the text";
        }
    }

    llama_sampler *sampler = llama_sampler_chain_init(llama_sampler_chain_default_params());
    llama_sampler_chain_add(sampler, llama_sampler_init_greedy());
    std::string answer;
    for (int i = 0; i < maxAnswer && !cancel.load(); i++) {
        llama_token token = llama_sampler_sample(sampler, context_, -1);
        if (llama_vocab_is_eog(vocab, token)) break;
        answer += tokenText(vocab, token);
        if (!endsWithIncompleteUtf8(answer)) onPartial(cleanAnswer(answer));
        if (llama_decode(context_, llama_batch_get_one(&token, 1)) != 0) break;
    }
    llama_sampler_free(sampler);
    if (cancel.load()) return "error: cancelled";
    return cleanAnswer(answer);
}

}  // namespace minkb
