#include <jni.h>

extern "C" {
#include <picoo_camera.h>
}

extern "C" JNIEXPORT jstring JNICALL
Java_com_picoo_camera_jni_PicooNative_getProtocolVersion(JNIEnv *env, jobject /* this */) {
    const char *version = picoo_protocol_version();
    return env->NewStringUTF(version);
}
