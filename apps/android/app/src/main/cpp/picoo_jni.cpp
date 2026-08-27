#include <jni.h>

extern "C" {
#include <picoo_camera.h>
}

extern "C" JNIEXPORT jstring JNICALL
Java_com_picoo_camera_jni_PicooNative_getProtocolVersion(JNIEnv *env, jobject /* this */) {
    const char *version = picoo_protocol_version();
    return env->NewStringUTF(version);
}

extern "C" JNIEXPORT jlong JNICALL
Java_com_picoo_camera_jni_PicooNative_createSender(JNIEnv * /* env */, jobject /* this */) {
    return reinterpret_cast<jlong>(picoo_sender_create());
}

extern "C" JNIEXPORT void JNICALL
Java_com_picoo_camera_jni_PicooNative_destroySender(JNIEnv * /* env */, jobject /* this */, jlong handle) {
    picoo_sender_destroy(reinterpret_cast<void *>(handle));
}

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_ingestAccessUnit(
    JNIEnv *env,
    jobject /* this */,
    jlong handle,
    jbyteArray data,
    jboolean keyframe,
    jlong pts_us,
    jint stream_epoch) {
    if (handle == 0 || data == nullptr) {
        return -1;
    }

    jsize len = env->GetArrayLength(data);
    if (len <= 0) {
        return -1;
    }

    jbyte *bytes = env->GetByteArrayElements(data, nullptr);
    if (bytes == nullptr) {
        return -1;
    }

    uint32_t out_packets = 0;
    int32_t rc = picoo_sender_ingest_access_unit(
        reinterpret_cast<void *>(handle),
        reinterpret_cast<const uint8_t *>(bytes),
        static_cast<size_t>(len),
        keyframe ? 1 : 0,
        static_cast<uint64_t>(pts_us),
        static_cast<uint32_t>(stream_epoch),
        &out_packets);

    env->ReleaseByteArrayElements(data, bytes, JNI_ABORT);
    return rc == 0 ? static_cast<jint>(out_packets) : rc;
}

extern "C" JNIEXPORT jlongArray JNICALL
Java_com_picoo_camera_jni_PicooNative_getSenderStats(JNIEnv *env, jobject /* this */, jlong handle) {
    jlongArray result = env->NewLongArray(5);
    if (handle == 0 || result == nullptr) {
        return result;
    }

    PicooSenderStats stats{};
    if (picoo_sender_stats(reinterpret_cast<void *>(handle), &stats) != 0) {
        return result;
    }

    jlong values[5] = {
        static_cast<jlong>(stats.access_units),
        static_cast<jlong>(stats.packets),
        static_cast<jlong>(stats.bytes),
        static_cast<jlong>(stats.sent_datagrams),
        static_cast<jlong>(picoo_sender_pending_packets(reinterpret_cast<void *>(handle))),
    };
    env->SetLongArrayRegion(result, 0, 5, values);
    return result;
}

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_connect(
    JNIEnv *env,
    jobject /* this */,
    jlong handle,
    jstring host,
    jint port) {
    if (handle == 0 || host == nullptr) {
        return -1;
    }
    const char *host_chars = env->GetStringUTFChars(host, nullptr);
    if (host_chars == nullptr) {
        return -1;
    }
    int32_t rc = picoo_sender_connect(
        reinterpret_cast<void *>(handle),
        host_chars,
        static_cast<uint16_t>(port));
    env->ReleaseStringUTFChars(host, host_chars);
    return rc;
}

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_flushPending(JNIEnv * /* env */, jobject /* this */, jlong handle) {
    if (handle == 0) {
        return -1;
    }
    uint32_t sent = 0;
    int32_t rc = picoo_sender_flush(reinterpret_cast<void *>(handle), &sent);
    return rc == 0 ? static_cast<jint>(sent) : rc;
}

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_pump(JNIEnv * /* env */, jobject /* this */, jlong handle) {
    if (handle == 0) {
        return -1;
    }
    return picoo_sender_pump(reinterpret_cast<void *>(handle));
}
