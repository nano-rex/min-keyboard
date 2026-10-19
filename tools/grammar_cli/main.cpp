// Runs the keyboard's grammar engine from the command line.
// Usage: grammar_cli MODEL.gguf LANGUAGE TEXT [THREADS]
//        grammar_cli MODEL.gguf --batch FILE [THREADS]   (FILE lines: LANGUAGE<TAB>TEXT)
#include <atomic>
#include <chrono>
#include <cstdio>
#include <fstream>
#include <string>

#include "grammar_core.h"

static void run(minkb::GrammarCore &core, const std::string &language, const std::string &text) {
    std::atomic<bool> cancel{false};
    auto start = std::chrono::steady_clock::now();
    std::string result = core.check(text, language, [](const std::string &) {}, cancel);
    double seconds = std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
    std::printf("[%s] %s\n  -> %s  (%.1fs)\n", language.c_str(), text.c_str(), result.c_str(), seconds);
}

int main(int argc, char **argv) {
    if (argc < 4) {
        std::fprintf(stderr, "usage: %s MODEL.gguf LANGUAGE TEXT [THREADS] | MODEL.gguf --batch FILE [THREADS]\n", argv[0]);
        return 2;
    }
    minkb::LoadOptions options;
    options.threads = argc > 4 ? std::stoi(argv[4]) : 4;
    minkb::GrammarCore core;
    std::string error = core.load(argv[1], options);
    if (!error.empty()) {
        std::fprintf(stderr, "load failed: %s\n", error.c_str());
        return 1;
    }
    std::printf("model: %.0f MB\n", core.modelBytes() / 1e6);
    if (std::string(argv[2]) == "--batch") {
        std::ifstream file(argv[3]);
        std::string line;
        while (std::getline(file, line)) {
            size_t tab = line.find('\t');
            if (tab != std::string::npos) run(core, line.substr(0, tab), line.substr(tab + 1));
        }
    } else {
        run(core, argv[2], argv[3]);
    }
#ifdef __linux__
    // Peak resident memory, used to calibrate ModelAdvisor.estimateRuntimeBytes().
    std::ifstream status("/proc/self/status");
    std::string field;
    while (std::getline(status, field)) {
        if (field.rfind("VmHWM:", 0) == 0) std::printf("peak memory: %s\n", field.substr(6).c_str());
    }
#endif
    return 0;
}
