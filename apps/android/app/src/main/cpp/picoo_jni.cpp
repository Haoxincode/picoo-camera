#include <jni.h>
#include <cstring>

extern "C" {
#include <picoo_camera.h>
}

static jstring makeJString(JNIEnv *env, const char *value) {
    return env->NewStringUTF(value != nullptr ? value : "");
}

static int copyJString(JNIEnv *env, jstring value, char *out, size_t out_len) {
    if (value == nullptr || out == nullptr || out_len == 0) {
        return -1;
    }
    const char *chars = env->GetStringUTFChars(value, nullptr);
    if (chars == nullptr) {
        return -1;
    }
    std::strncpy(out, chars, out_len - 1);
    out[out_len - 1] = '\0';
    env->ReleaseStringUTFChars(value, chars);
    return 0;
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
        static_cast<jlong>(picoo_sender_pending_packets(reinterpret_cast<void *>(handle)),
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

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_getSenderStatus(JNIEnv * /* env */, jobject /* this */, jlong handle) {
    if (handle == 0) {
        return 0;
    }
    return picoo_sender_status(reinterpret_cast<void *>(handle));
}

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_sendClientHello(
    JNIEnv *env,
    jobject /* this */,
    jlong handle,
    jstring senderId,
    jstring deviceName,
    jbyteArray publicKey) {
    if (handle == 0 || senderId == nullptr || deviceName == nullptr) {
        return -1;
    }
    const char *sender_chars = env->GetStringUTFChars(senderId, nullptr);
    const char *device_chars = env->GetStringUTFChars(deviceName, nullptr);
    if (sender_chars == nullptr || device_chars == nullptr) {
        if (sender_chars != nullptr) {
            env->ReleaseStringUTFChars(senderId, sender_chars);
        }
        if (device_chars != nullptr) {
            env->ReleaseStringUTFChars(deviceName, device_chars);
        }
        return -1;
    }

    const uint8_t *key_ptr = nullptr;
    jsize key_len = 0;
    if (publicKey != nullptr) {
        key_len = env->GetArrayLength(publicKey);
        key_ptr = reinterpret_cast<const uint8_t *>(env->GetByteArrayElements(publicKey, nullptr));
    }

    int32_t rc = picoo_sender_send_client_hello(
        reinterpret_cast<void *>(handle),
        sender_chars,
        device_chars,
        key_ptr,
        static_cast<size_t>(key_len));

    env->ReleaseStringUTFChars(senderId, sender_chars);
    env->ReleaseStringUTFChars(deviceName, device_chars);
    if (publicKey != nullptr && key_ptr != nullptr) {
        env->ReleaseByteArrayElements(publicKey, reinterpret_cast<jbyte *>(const_cast<uint8_t *>(key_ptr)), JNI_ABORT);
    }
    return rc;
}

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_sendPairingConfirm(
    JNIEnv *env,
    jobject /* this */,
    jlong handle,
    jstring receiverId) {
    if (handle == 0 || receiverId == nullptr) {
        return -1;
    }
    const char *receiver_chars = env->GetStringUTFChars(receiverId, nullptr);
    if (receiver_chars == nullptr) {
        return -1;
    }
    int32_t rc = picoo_sender_send_pairing_confirm(
        reinterpret_cast<void *>(handle),
        receiver_chars);
    env->ReleaseStringUTFChars(receiverId, receiver_chars);
    return rc;
}

extern "C" JNIEXPORT jstring JNICALL
Java_com_picoo_camera_jni_PicooNative_getPairingShortCode(JNIEnv *env, jobject /* this */, jlong handle) {
    if (handle == 0) {
        return makeJString(env, "");
    }
    char buf[16] = {0};
    if (picoo_sender_pairing_short_code(reinterpret_cast<void *>(handle), buf, sizeof(buf)) <= 0) {
        return makeJString(env, "");
    }
    return makeJString(env, buf);
}

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_setStreamConfig(
    JNIEnv * /* env */,
    jobject /* this */,
    jlong handle,
    jint width,
    jint height,
    jint fps,
    jint bitrateBps,
    jint streamEpoch,
    jboolean mirrored) {
    if (handle == 0) {
        return -1;
    }
    return picoo_sender_set_stream_config(
        reinterpret_cast<void *>(handle),
        static_cast<uint32_t>(width),
        static_cast<uint32_t>(height),
        static_cast<uint32_t>(fps),
        static_cast<uint32_t>(bitrateBps),
        static_cast<uint32_t>(streamEpoch),
        mirrored ? 1 : 0);
}

extern "C" JNIEXPORT jlong JNICALL
Java_com_picoo_camera_jni_PicooNative_createDiscoveryBrowser(JNIEnv * /* env */, jobject /* this */) {
    return reinterpret_cast<jlong>(picoo_discovery_browser_create());
}

extern "C" JNIEXPORT void JNICALL
Java_com_picoo_camera_jni_PicooNative_destroyDiscoveryBrowser(
    JNIEnv * /* env */,
    jobject /* this */,
    jlong handle) {
    picoo_discovery_browser_destroy(reinterpret_cast<void *>(handle));
}

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_pollDiscoveryBrowser(
    JNIEnv * /* env */,
    jobject /* this */,
    jlong handle,
    jint timeoutMs) {
    if (handle == 0) {
        return -1;
    }
    return picoo_discovery_browser_poll(reinterpret_cast<void *>(handle), static_cast<uint32_t>(timeoutMs));
}

extern "C" JNIEXPORT jint JNICALL
Java_com_picoo_camera_jni_PicooNative_getDiscoveryCount(JNIEnv * /* env */, jobject /* this */, jlong handle) {
    if (handle == 0) {
        return 0;
    }
    return static_cast<jint>(picoo_discovery_browser_count(reinterpret_cast<void *>(handle)));
}

extern "C" JNIEXPORT jobject JNICALL
Java_com_picoo_camera_jni_PicooNative_getDiscoveredReceiver(
    JNIEnv *env,
    jobject /* this */,
    jlong handle,
    jint index) {
    if (handle == 0) {
        return nullptr;
    }

    PicooDiscoveredReceiver item{};
    if (picoo_discovery_browser_get(reinterpret_cast<void *>(handle), static_cast<uint32_t>(index), &item) != 0) {
        return nullptr;
    }

    jclass cls = env->FindClass("com/picoo/camera/jni/PicooNative$DiscoveredReceiver");
    if (cls == nullptr) {
        return nullptr;
    }
    jmethodID ctor = env->GetMethodID(cls, "<init>", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;I)V");
    if (ctor == nullptr) {
        return nullptr;
    }

    return env->NewObject(
        cls,
        ctor,
        makeJString(env, reinterpret_cast<const char *>(item.receiver_id)),
        makeJString(env, reinterpret_cast<const char *>(item.display_name)),
        makeJString(env, reinterpret_cast<const char *>(item.host)),
        static_cast<jint>(item.quic_port));
}
