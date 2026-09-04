use std::ptr;
use std::sync::Mutex;

use jni::objects::{JByteArray, JObject, JString, JValue};
use jni::sys::{jbyteArray, jint, jlong, jobject, jstring};
use jni::JNIEnv;
use picoo_pairing::{DeviceIdentity, TrustedDeviceStore};

use super::{
    identities, java_string, new_java_string, trusted_stores, with_identity, with_trusted_store,
};
use crate::handles::TrustedStoreInner;

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_loadTrustedStore(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    path: JString<'_>,
) -> jlong {
    let Some(path) = java_string(&mut env, path) else {
        return 0;
    };
    let Ok(store) = TrustedDeviceStore::load_from_path(&path) else {
        return 0;
    };
    let inner = TrustedStoreInner {
        store: Mutex::new(store),
        path: Mutex::new(Some(path)),
    };
    trusted_stores()
        .lock()
        .map(|mut handles| handles.insert(inner))
        .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_destroyTrustedStore(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    if let Ok(mut handles) = trusted_stores().lock() {
        handles.values.remove(&handle);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getTrustedDeviceCount(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_trusted_store(handle, |inner| {
        inner
            .store
            .lock()
            .map(|store| store.list().count() as jint)
            .unwrap_or(0)
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getTrustedDevice(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jobject {
    let Some(device) = with_trusted_store(handle, |inner| {
        inner.store.lock().ok()?.list().nth(index as usize).cloned()
    })
    .flatten() else {
        return ptr::null_mut();
    };
    let Ok(class) = env.find_class("com/picoo/camera/jni/PicooNative$TrustedDevice") else {
        return ptr::null_mut();
    };
    let (Ok(device_id), Ok(device_name), Ok(fingerprint)) = (
        env.new_string(device.device_id),
        env.new_string(device.device_name),
        env.new_string(device.certificate_fingerprint),
    ) else {
        return ptr::null_mut();
    };
    let device_id = JObject::from(device_id);
    let device_name = JObject::from(device_name);
    let fingerprint = JObject::from(fingerprint);
    env.new_object(
        class,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;JJ)V",
        &[
            JValue::Object(&device_id),
            JValue::Object(&device_name),
            JValue::Object(&fingerprint),
            JValue::Long(device.paired_at_ms as jlong),
            JValue::Long(device.last_connected_at_ms.unwrap_or(0) as jlong),
        ],
    )
    .map(JObject::into_raw)
    .unwrap_or(ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_clearTrustedDevices(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_trusted_store(handle, |inner| {
        inner
            .store
            .lock()
            .map(|mut store| store.clear() as jint)
            .unwrap_or(-1)
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_saveTrustedStore(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_trusted_store(handle, |inner| {
        let Some(path) = inner.path.lock().ok().and_then(|path| path.clone()) else {
            return -2;
        };
        inner
            .store
            .lock()
            .map_err(|_| ())
            .and_then(|store| store.save_to_path(path).map_err(|_| ()))
            .map(|_| 0)
            .unwrap_or(-3)
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_loadIdentityFromSecret(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    secret: JByteArray<'_>,
    default_name: JString<'_>,
) -> jlong {
    let Ok(secret) = env.convert_byte_array(secret) else {
        return 0;
    };
    let secret = zeroize::Zeroizing::new(secret);
    let mut env = env;
    let default_name = java_string(&mut env, default_name).unwrap_or_else(|| "Picoo Phone".into());
    let Ok(identity) = DeviceIdentity::from_secret_bytes(&default_name, &secret) else {
        return 0;
    };
    identities()
        .lock()
        .map(|mut handles| handles.insert(identity))
        .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_destroyIdentity(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    if let Ok(mut handles) = identities().lock() {
        handles.values.remove(&handle);
    }
}

fn identity_string(
    env: &mut JNIEnv<'_>,
    handle: jlong,
    get: impl FnOnce(&DeviceIdentity) -> &str,
) -> jstring {
    let value = with_identity(handle, |identity| get(identity).to_owned()).unwrap_or_default();
    new_java_string(env, &value)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getIdentityDeviceId(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jstring {
    identity_string(&mut env, handle, |identity| &identity.device_id)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getIdentityDeviceName(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jstring {
    identity_string(&mut env, handle, |identity| &identity.device_name)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getIdentityPublicKey(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jbyteArray {
    let key = with_identity(handle, |identity| identity.public_key().to_vec()).unwrap_or_default();
    env.byte_array_from_slice(&key)
        .map(JByteArray::into_raw)
        .unwrap_or(ptr::null_mut())
}
