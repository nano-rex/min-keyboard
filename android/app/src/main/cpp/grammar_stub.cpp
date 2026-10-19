// 32-bit ARM build: llama.cpp is not compiled (too slow and too little memory on these devices,
// and it does not build for armv7). This stub keeps the APK installable on 32-bit phones;
// DeviceProfiler already reports the AI check as unsupported there.
#include <jni.h>

extern "C" {

JNIEXPORT jstring JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeLoad(JNIEnv *env, jclass, jstring, jint, jint) {
    return env->NewStringUTF("The AI grammar check needs a 64-bit CPU.");
}

JNIEXPORT void JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeUnload(JNIEnv *, jclass) {}

JNIEXPORT jlong JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeModelBytes(JNIEnv *, jclass) { return 0; }

JNIEXPORT void JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeCancel(JNIEnv *, jclass) {}

JNIEXPORT jstring JNICALL
Java_org_convoy_pinyinime_GrammarChecker_nativeCheck(JNIEnv *env, jclass, jstring, jstring, jobject) {
    return env->NewStringUTF("error: the AI grammar check needs a 64-bit CPU");
}

}  // extern "C"
