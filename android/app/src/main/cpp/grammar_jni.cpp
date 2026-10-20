// JNI bridge for org.convoy.pinyinime.GrammarChecker. All calls come from one worker thread,
// except nativeCancel, which only flips an atomic flag.
#include <jni.h>

#include <atomic>
#include <mutex>
#include <string>

#include "grammar_core.h"

namespace {

minkb::GrammarCore core;
std::mutex coreMutex;
std::atomic<bool> cancelFlag{false};

std::string toString(JNIEnv *env, jstring value) {
    if (value == nullptr) return std::string();
    const char *chars = env->GetStringUTFChars(value, nullptr);
    std::string result(chars != nullptr ? chars : "");
    env->ReleaseStringUTFChars(value, chars);
    return result;
}

// NewStringUTF expects modified UTF-8, so build Java strings from UTF-16 instead.
jstring toJava(JNIEnv *env, const std::string &utf8) {
    std::u16string utf16;
    for (size_t i = 0; i < utf8.size();) {
        unsigned char c = static_cast<unsigned char>(utf8[i]);
        uint32_t code;
        int length;
        if (c < 0x80) { code = c; length = 1; }
        else if ((c >> 5) == 0x6) { code = c & 0x1F; length = 2; }
        else if ((c >> 4) == 0xE) { code = c & 0x0F; length = 3; }
        else if ((c >> 3) == 0x1E) { code = c & 0x07; length = 4; }
        else { i++; continue; }
        if (i + length > utf8.size()) break;
        for (int k = 1; k < length; k++) code = (code << 6) | (static_cast<unsigned char>(utf8[i + k]) & 0x3F);
        i += length;
        if (code >= 0x10000) {
            code -= 0x10000;
            utf16.push_back(static_cast<char16_t>(0xD800 + (code >> 10)));
            utf16.push_back(static_cast<char16_t>(0xDC00 + (code & 0x3FF)));
        } else {
            utf16.push_back(static_cast<char16_t>(code));
        }
    }
    return env->NewString(reinterpret_cast<const jchar *>(utf16.data()), static_cast<jsize>(utf16.size()));
}

}  // namespace

extern "C" {

JNIEXPORT jstring JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeLoad(JNIEnv *env, jclass, jstring path, jint threads, jint context) {
    std::lock_guard<std::mutex> lock(coreMutex);
    minkb::LoadOptions options;
    options.threads = threads;
    options.context = context;
    return toJava(env, core.load(toString(env, path), options));
}

JNIEXPORT void JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeUnload(JNIEnv *, jclass) {
    cancelFlag.store(true);
    std::lock_guard<std::mutex> lock(coreMutex);
    core.unload();
}

JNIEXPORT jlong JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeModelBytes(JNIEnv *, jclass) {
    std::lock_guard<std::mutex> lock(coreMutex);
    return static_cast<jlong>(core.modelBytes());
}

JNIEXPORT void JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeCancel(JNIEnv *, jclass) {
    cancelFlag.store(true);
}

JNIEXPORT jstring JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeCheck(JNIEnv *env, jclass, jstring text, jstring language, jobject listener) {
    std::lock_guard<std::mutex> lock(coreMutex);
    cancelFlag.store(false);
    jmethodID onPartial = nullptr;
    if (listener != nullptr) {
        jclass type = env->GetObjectClass(listener);
        onPartial = env->GetMethodID(type, "onPartial", "(Ljava/lang/String;)V");
        env->DeleteLocalRef(type);
    }
    std::string result = core.check(toString(env, text), toString(env, language),
            [&](const std::string &partial) {
                if (onPartial == nullptr) return;
                jstring value = toJava(env, partial);
                env->CallVoidMethod(listener, onPartial, value);
                env->DeleteLocalRef(value);
                if (env->ExceptionCheck()) {
                    env->ExceptionClear();
                    cancelFlag.store(true);
                }
            },
            cancelFlag);
    return toJava(env, result);
}

}  // extern "C"
