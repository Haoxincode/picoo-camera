use std::ptr;
use std::sync::Mutex;
use std::time::Duration;

use jni::objects::{JByteArray, JObject, JString, JValue};
use jni::sys::{jint, jlong, jobject, jobjectArray};
use jni::JNIEnv;
use picoo_discovery::MdnsBrowser;

use super::{browsers, fixed_string, java_string, with_browser};
use crate::c_discovery::PicooDiscoveredReceiver;
use crate::handles::{write_field, BrowserInner};

/// Validate Android NSD TXT bytes with the canonical Rust discovery parser.
#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_parseDiscoveryTxt(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    keys: jobjectArray,
    values: jobjectArray,
) -> jobjectArray {
    if keys.is_null() || values.is_null() {
        return ptr::null_mut();
    }
    let keys = unsafe { jni::objects::JObjectArray::from_raw(keys) };
    let values = unsafe { jni::objects::JObjectArray::from_raw(values) };
    let (Ok(key_count), Ok(value_count)) =
        (env.get_array_length(&keys), env.get_array_length(&values))
    else {
        return ptr::null_mut();
    };
    if key_count != value_count {
        return ptr::null_mut();
    }

    let mut properties = Vec::with_capacity(key_count as usize);
    for index in 0..key_count {
        let (Ok(key), Ok(value)) = (
            env.get_object_array_element(&keys, index),
            env.get_object_array_element(&values, index),
        ) else {
            return ptr::null_mut();
        };
        let Some(key) = java_string(&mut env, JString::from(key)) else {
            return ptr::null_mut();
        };
        let Ok(bytes) = env.convert_byte_array(JByteArray::from(value)) else {
            return ptr::null_mut();
        };
        let Ok(value) = String::from_utf8(bytes) else {
            return ptr::null_mut();
        };
        properties.push((key, value.trim().to_owned()));
    }

    let Ok(advertisement) =
        picoo_discovery::ReceiverAdvertisement::from_txt_properties(&properties)
    else {
        return ptr::null_mut();
    };
    let Ok(string_class) = env.find_class("java/lang/String") else {
        return ptr::null_mut();
    };
    let Ok(result) = env.new_object_array(6, string_class, JObject::null()) else {
        return ptr::null_mut();
    };
    let fields = [
        advertisement.receiver_id,
        advertisement.display_name,
        advertisement.quic_port.to_string(),
        advertisement.pairing_state.as_str().to_owned(),
        advertisement.public_key_fingerprint_prefix,
        advertisement.platform.as_str().to_owned(),
    ];
    for (index, field) in fields.iter().enumerate() {
        let Ok(value) = env.new_string(field) else {
            return ptr::null_mut();
        };
        if env
            .set_object_array_element(&result, index as jint, value)
            .is_err()
        {
            return ptr::null_mut();
        }
    }
    result.into_raw()
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_createDiscoveryBrowser(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jlong {
    let Ok(browser) = MdnsBrowser::new() else {
        return 0;
    };
    let inner = BrowserInner {
        browser: Mutex::new(browser),
        receivers: Mutex::new(Vec::new()),
    };
    browsers()
        .lock()
        .map(|mut handles| handles.insert(inner))
        .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_destroyDiscoveryBrowser(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    if let Ok(mut handles) = browsers().lock() {
        handles.values.remove(&handle);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_pollDiscoveryBrowser(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    timeout_ms: jint,
) -> jint {
    with_browser(handle, |inner| {
        let Ok(mut browser) = inner.browser.lock() else {
            return -1;
        };
        if browser
            .poll(Duration::from_millis(timeout_ms.max(0) as u64))
            .is_err()
        {
            return -2;
        }
        let Ok(mut cached) = inner.receivers.lock() else {
            return -1;
        };
        cached.clear();
        for entry in browser.list() {
            let mut item = PicooDiscoveredReceiver {
                receiver_id: [0; 64],
                display_name: [0; 64],
                host: [0; 64],
                quic_port: entry.advertisement.quic_port,
                platform: [0; 16],
                pairing_state: [0; 32],
            };
            write_field(&mut item.receiver_id, &entry.advertisement.receiver_id);
            write_field(&mut item.display_name, &entry.advertisement.display_name);
            write_field(&mut item.host, &entry.host);
            write_field(&mut item.platform, entry.advertisement.platform.as_str());
            write_field(
                &mut item.pairing_state,
                entry.advertisement.pairing_state.as_str(),
            );
            cached.push(item);
        }
        cached.len() as jint
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getDiscoveryCount(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_browser(handle, |inner| {
        inner
            .receivers
            .lock()
            .map(|cached| cached.len() as jint)
            .unwrap_or(0)
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getDiscoveredReceiver(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    index: jint,
) -> jobject {
    let Some(item) = with_browser(handle, |inner| {
        inner.receivers.lock().ok()?.get(index as usize).copied()
    })
    .flatten() else {
        return ptr::null_mut();
    };
    let Ok(class) = env.find_class("com/picoo/camera/jni/PicooNative$DiscoveredReceiver") else {
        return ptr::null_mut();
    };
    let strings = [
        fixed_string(&item.receiver_id),
        fixed_string(&item.display_name),
        fixed_string(&item.host),
        fixed_string(&item.platform),
        fixed_string(&item.pairing_state),
    ];
    let (Ok(receiver_id), Ok(display_name), Ok(host), Ok(platform), Ok(pairing_state)) = (
        env.new_string(&strings[0]),
        env.new_string(&strings[1]),
        env.new_string(&strings[2]),
        env.new_string(&strings[3]),
        env.new_string(&strings[4]),
    ) else {
        return ptr::null_mut();
    };
    let receiver_id = JObject::from(receiver_id);
    let display_name = JObject::from(display_name);
    let host = JObject::from(host);
    let platform = JObject::from(platform);
    let pairing_state = JObject::from(pairing_state);
    env.new_object(
        class,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;)V",
        &[
            JValue::Object(&receiver_id),
            JValue::Object(&display_name),
            JValue::Object(&host),
            JValue::Int(item.quic_port as jint),
            JValue::Object(&platform),
            JValue::Object(&pairing_state),
        ],
    )
    .map(JObject::into_raw)
    .unwrap_or(ptr::null_mut())
}
