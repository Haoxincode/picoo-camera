//! Android JNI exports implemented directly in Rust.

#![allow(non_snake_case)]

use std::collections::HashMap;
use std::ptr;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use jni::objects::{JByteArray, JIntArray, JObject, JString, JValue};
use jni::sys::{
    jboolean, jbyteArray, jdoubleArray, jint, jlong, jlongArray, jobject, jobjectArray, jstring,
    JNI_TRUE,
};
use jni::JNIEnv;
use picoo_diagnostics::DiagnosticSessionSnapshot;

use super::*;

struct HandleMap<T> {
    next: jlong,
    values: HashMap<jlong, T>,
}

impl<T> Default for HandleMap<T> {
    fn default() -> Self {
        Self {
            next: 1,
            values: HashMap::new(),
        }
    }
}

impl<T> HandleMap<T> {
    fn insert(&mut self, value: T) -> jlong {
        let handle = self.next;
        self.next = self.next.saturating_add(1).max(1);
        self.values.insert(handle, value);
        handle
    }
}

static SENDERS: OnceLock<Mutex<HandleMap<SenderInner>>> = OnceLock::new();
static BROWSERS: OnceLock<Mutex<HandleMap<BrowserInner>>> = OnceLock::new();
static TRUSTED_STORES: OnceLock<Mutex<HandleMap<TrustedStoreInner>>> = OnceLock::new();
static IDENTITIES: OnceLock<Mutex<HandleMap<DeviceIdentity>>> = OnceLock::new();

fn senders() -> &'static Mutex<HandleMap<SenderInner>> {
    SENDERS.get_or_init(|| Mutex::new(HandleMap::default()))
}

fn browsers() -> &'static Mutex<HandleMap<BrowserInner>> {
    BROWSERS.get_or_init(|| Mutex::new(HandleMap::default()))
}

fn trusted_stores() -> &'static Mutex<HandleMap<TrustedStoreInner>> {
    TRUSTED_STORES.get_or_init(|| Mutex::new(HandleMap::default()))
}

fn identities() -> &'static Mutex<HandleMap<DeviceIdentity>> {
    IDENTITIES.get_or_init(|| Mutex::new(HandleMap::default()))
}

fn with_sender<R>(handle: jlong, f: impl FnOnce(&mut SenderInner) -> R) -> Option<R> {
    let mut handles = senders().lock().ok()?;
    Some(f(handles.values.get_mut(&handle)?))
}

fn java_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Option<String> {
    if value.is_null() {
        return None;
    }
    env.get_string(&value).ok().map(Into::into)
}

fn optional_java_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Option<String> {
    java_string(env, value).filter(|value| !value.is_empty())
}

fn new_java_string(env: &mut JNIEnv<'_>, value: &str) -> jstring {
    env.new_string(value)
        .map(JString::into_raw)
        .unwrap_or(ptr::null_mut())
}

fn fixed_string(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getProtocolVersion(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jstring {
    new_java_string(&mut env, picoo_protocol::ALPN)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_createSender(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jlong {
    let inner = SenderInner {
        session: Mutex::new(SenderSession::new(QuicSenderTransport::new())),
    };
    senders()
        .lock()
        .map(|mut handles| handles.insert(inner))
        .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_destroySender(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    if let Ok(mut handles) = senders().lock() {
        handles.values.remove(&handle);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_ingestAccessUnit(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    data: JByteArray<'_>,
    keyframe: jboolean,
    pts_us: jlong,
    stream_epoch: jint,
) -> jint {
    let Ok(data) = env.convert_byte_array(data) else {
        return -1;
    };
    if data.is_empty() {
        return -1;
    }
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        session
            .ingest_access_unit(
                &data,
                keyframe == JNI_TRUE,
                pts_us as u64,
                stream_epoch as u32,
            )
            .map(|count| count as jint)
            .unwrap_or(-2)
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getSenderStats(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jlongArray {
    let Ok(result) = env.new_long_array(5) else {
        return ptr::null_mut();
    };
    let values = with_sender(handle, |inner| {
        let Ok(session) = inner.session.lock() else {
            return [0; 5];
        };
        let stats = session.stats();
        [
            stats.pipeline.access_units as jlong,
            stats.pipeline.packets as jlong,
            stats.pipeline.bytes as jlong,
            stats.sent_datagrams as jlong,
            session.pending_packets() as jlong,
        ]
    })
    .unwrap_or([0; 5]);
    let _ = env.set_long_array_region(&result, 0, &values);
    result.into_raw()
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_connect(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    host: JString<'_>,
    port: jint,
) -> jint {
    let Some(host) = java_string(&mut env, host) else {
        return -1;
    };
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        session
            .connect(Endpoint {
                host,
                port: port as u16,
            })
            .map(|_| 0)
            .unwrap_or(-2)
    })
    .unwrap_or(-1)
}

macro_rules! sender_int_call {
    ($name:ident, $invalid:expr, |$session:ident| $body:expr) => {
        #[no_mangle]
        pub extern "system" fn $name(_env: JNIEnv<'_>, _this: JObject<'_>, handle: jlong) -> jint {
            with_sender(handle, |inner| {
                #[allow(unused_mut)]
                let Ok(mut $session) = inner.session.lock() else {
                    return $invalid;
                };
                $body
            })
            .unwrap_or($invalid)
        }
    };
}

sender_int_call!(
    Java_com_picoo_camera_jni_PicooNative_disconnect,
    -1,
    |session| {
        session.disconnect();
        0
    }
);
sender_int_call!(
    Java_com_picoo_camera_jni_PicooNative_flushPending,
    -1,
    |session| session
        .flush_pending()
        .map(|sent| sent as jint)
        .unwrap_or(-2)
);
sender_int_call!(Java_com_picoo_camera_jni_PicooNative_pump, -1, |session| {
    session.pump().map(|_| 0).unwrap_or(-2)
});
sender_int_call!(
    Java_com_picoo_camera_jni_PicooNative_markPermissionRequired,
    -1,
    |session| {
        session.mark_permission_required();
        0
    }
);
sender_int_call!(
    Java_com_picoo_camera_jni_PicooNative_clearPermissionRequired,
    -1,
    |session| {
        session.clear_permission_required();
        0
    }
);
sender_int_call!(
    Java_com_picoo_camera_jni_PicooNative_takeKeyframeRequest,
    -1,
    |session| i32::from(session.take_keyframe_request())
);
sender_int_call!(
    Java_com_picoo_camera_jni_PicooNative_beginStreamReconfiguration,
    0,
    |session| session.begin_stream_reconfiguration() as jint
);

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_cancelStreamReconfiguration(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    stream_epoch: jint,
) -> jint {
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        if session.cancel_stream_reconfiguration(stream_epoch as u32) {
            0
        } else {
            -2
        }
    })
    .unwrap_or(-1)
}

/// [status, bitrate, activeHeight, receiverMaxHeight, epoch, reconnectAttempt,
/// reconnectDelayMs], captured under one sender-session lock.
#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getSenderSnapshot(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jlongArray {
    let Ok(result) = env.new_long_array(7) else {
        return ptr::null_mut();
    };
    let values = with_sender(handle, |inner| {
        let Ok(session) = inner.session.lock() else {
            return [0; 7];
        };
        let snapshot = sender_snapshot(&session);
        [
            snapshot.status as jlong,
            snapshot.current_bitrate_bps as jlong,
            snapshot.active_height as jlong,
            snapshot.receiver_max_height as jlong,
            snapshot.stream_epoch as jlong,
            snapshot.reconnect_attempt as jlong,
            snapshot.reconnect_delay_ms as jlong,
        ]
    })
    .unwrap_or([0; 7]);
    let _ = env.set_long_array_region(&result, 0, &values);
    result.into_raw()
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getEncoderDirective(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jlongArray {
    let Some(directive) = with_sender(handle, |inner| {
        inner.session.lock().ok()?.pending_encoder_directive()
    })
    .flatten() else {
        return ptr::null_mut();
    };
    let Ok(result) = env.new_long_array(5) else {
        return ptr::null_mut();
    };
    let values = [
        directive.id as jlong,
        directive.kind as u32 as jlong,
        directive.target_height as jlong,
        directive.target_bitrate_bps as jlong,
        directive.stream_epoch as jlong,
    ];
    if env.set_long_array_region(&result, 0, &values).is_err() {
        return ptr::null_mut();
    }
    result.into_raw()
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_ackEncoderDirective(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    directive_id: jlong,
    actual_height: jint,
) -> jint {
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        i32::from(session.acknowledge_encoder_directive(directive_id as u64, actual_height as u32))
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_nackEncoderDirective(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    directive_id: jlong,
) -> jint {
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        i32::from(session.reject_encoder_directive(directive_id as u64))
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_sendClientHello(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    sender_id: JString<'_>,
    device_name: JString<'_>,
    public_key: JByteArray<'_>,
) -> jint {
    let (Some(sender_id), Some(device_name)) = (
        java_string(&mut env, sender_id),
        java_string(&mut env, device_name),
    ) else {
        return -1;
    };
    let public_key = if public_key.is_null() {
        Vec::new()
    } else {
        match env.convert_byte_array(public_key) {
            Ok(bytes) => bytes,
            Err(_) => return -1,
        }
    };
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        session
            .send_client_hello(&sender_id, &device_name, &public_key)
            .map(|_| 0)
            .unwrap_or(-2)
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_sendPairingConfirm(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    receiver_id: JString<'_>,
) -> jint {
    let Some(receiver_id) = java_string(&mut env, receiver_id) else {
        return -1;
    };
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        session
            .send_pairing_confirm(&receiver_id)
            .map(|_| 0)
            .unwrap_or(-2)
    })
    .unwrap_or(-1)
}

fn sender_string(
    env: &mut JNIEnv<'_>,
    handle: jlong,
    get: impl FnOnce(&SenderSession<QuicSenderTransport>) -> Option<&str>,
) -> jstring {
    let value = with_sender(handle, |inner| {
        inner
            .session
            .lock()
            .ok()
            .and_then(|session| get(&session).map(str::to_owned))
    })
    .flatten()
    .unwrap_or_default();
    new_java_string(env, &value)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getPairingShortCode(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jstring {
    sender_string(&mut env, handle, SenderSession::pairing_short_code)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_setStreamConfig(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    width: jint,
    height: jint,
    fps: jint,
    bitrate_bps: jint,
    mirrored: jboolean,
    rotation: jint,
    sps: JByteArray<'_>,
    pps: JByteArray<'_>,
) -> jint {
    let sps = if sps.is_null() {
        Vec::new()
    } else {
        match env.convert_byte_array(sps) {
            Ok(bytes) => bytes,
            Err(_) => return -1,
        }
    };
    let pps = if pps.is_null() {
        Vec::new()
    } else {
        match env.convert_byte_array(pps) {
            Ok(bytes) => bytes,
            Err(_) => return -1,
        }
    };
    let (sps, pps) = if pps.is_empty() {
        extract_sps_pps(&sps).unwrap_or((sps, pps))
    } else {
        (sps, pps)
    };
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        session.set_stream_config(StreamConfigParams {
            width: width as u32,
            height: height as u32,
            fps: fps as u32,
            bitrate_bps: bitrate_bps as u32,
            stream_epoch: 0,
            mirrored: mirrored == JNI_TRUE,
            rotation: rotation as u32,
            sps,
            pps,
        });
        0
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getLinkStats(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jdoubleArray {
    let Some(values) = with_sender(handle, |inner| {
        let session = inner.session.lock().ok()?;
        let stats = session.last_receiver_stats()?;
        Some([
            stats.rtt_ms,
            stats.packet_loss,
            stats.jitter_ms,
            stats.frame_age_ms,
            f64::from(stats.receive_bitrate),
            stats.jitter_buffer_depth_ms,
        ])
    })
    .flatten() else {
        return ptr::null_mut();
    };
    let Ok(result) = env.new_double_array(values.len() as jint) else {
        return ptr::null_mut();
    };
    if env.set_double_array_region(&result, 0, &values).is_err() {
        return ptr::null_mut();
    }
    result.into_raw()
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_takeCameraCommand(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    out: JIntArray<'_>,
) -> jint {
    let Some(command) = with_sender(handle, |inner| {
        inner.session.lock().ok()?.take_camera_command()
    })
    .flatten() else {
        return 0;
    };
    if !out.is_null() {
        let (width, height) = command
            .resolution
            .as_ref()
            .map(|resolution| (resolution.width, resolution.height))
            .unwrap_or((0, 0));
        let values = [width as jint, height as jint, i32::from(command.mirrored)];
        let _ = env.set_int_array_region(&out, 0, &values);
    }
    command.command
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_lastSessionError(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jstring {
    sender_string(&mut env, handle, SenderSession::last_session_error)
}

macro_rules! sender_set_u32 {
    ($name:ident, $method:ident) => {
        #[no_mangle]
        pub extern "system" fn $name(
            _env: JNIEnv<'_>,
            _this: JObject<'_>,
            handle: jlong,
            value: jint,
        ) -> jint {
            with_sender(handle, |inner| {
                let Ok(mut session) = inner.session.lock() else {
                    return -1;
                };
                session.$method(value as u32);
                0
            })
            .unwrap_or(-1)
        }
    };
}

sender_set_u32!(
    Java_com_picoo_camera_jni_PicooNative_setPreferredHeight,
    set_preferred_height
);
#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_reportEncoderHeight(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    height: jint,
    stream_epoch: jint,
) -> jint {
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        if session.report_encoder_height(height as u32, stream_epoch as u32) {
            0
        } else {
            -2
        }
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_bitrateInitialForHeight(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    height: jint,
) -> jint {
    BitrateLadder::for_height(height as u32).initial_bps as jint
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_bitrateClampForHeight(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    bitrate_bps: jint,
    height: jint,
) -> jint {
    let ladder = BitrateLadder::for_height(height as u32);
    (bitrate_bps as u32).clamp(ladder.min_bps, ladder.max_bps) as jint
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_setThermalHold(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    hold: jboolean,
) -> jint {
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        session.set_thermal_hold(hold == JNI_TRUE);
        0
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_extractSpsPps(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    data: JByteArray<'_>,
) -> jobjectArray {
    let Ok(data) = env.convert_byte_array(data) else {
        return ptr::null_mut();
    };
    let Some((sps, pps)) = extract_sps_pps(&data) else {
        return ptr::null_mut();
    };
    let Ok(byte_array_class) = env.find_class("[B") else {
        return ptr::null_mut();
    };
    let Ok(result) = env.new_object_array(2, byte_array_class, JObject::null()) else {
        return ptr::null_mut();
    };
    let (Ok(sps), Ok(pps)) = (
        env.byte_array_from_slice(&sps),
        env.byte_array_from_slice(&pps),
    ) else {
        return ptr::null_mut();
    };
    if env.set_object_array_element(&result, 0, sps).is_err()
        || env.set_object_array_element(&result, 1, pps).is_err()
    {
        return ptr::null_mut();
    }
    result.into_raw()
}

fn with_browser<R>(handle: jlong, f: impl FnOnce(&mut BrowserInner) -> R) -> Option<R> {
    let mut handles = browsers().lock().ok()?;
    Some(f(handles.values.get_mut(&handle)?))
}

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
        advertisement.protocol_version,
        advertisement.quic_port.to_string(),
        advertisement.pairing_state.as_str().to_owned(),
        advertisement.public_key_fingerprint_prefix,
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
                pairing_state: [0; 32],
            };
            write_field(&mut item.receiver_id, &entry.advertisement.receiver_id);
            write_field(&mut item.display_name, &entry.advertisement.display_name);
            write_field(&mut item.host, &entry.host);
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
        fixed_string(&item.pairing_state),
    ];
    let (Ok(receiver_id), Ok(display_name), Ok(host), Ok(pairing_state)) = (
        env.new_string(&strings[0]),
        env.new_string(&strings[1]),
        env.new_string(&strings[2]),
        env.new_string(&strings[3]),
    ) else {
        return ptr::null_mut();
    };
    let receiver_id = JObject::from(receiver_id);
    let display_name = JObject::from(display_name);
    let host = JObject::from(host);
    let pairing_state = JObject::from(pairing_state);
    env.new_object(
        class,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ILjava/lang/String;)V",
        &[
            JValue::Object(&receiver_id),
            JValue::Object(&display_name),
            JValue::Object(&host),
            JValue::Int(item.quic_port as jint),
            JValue::Object(&pairing_state),
        ],
    )
    .map(JObject::into_raw)
    .unwrap_or(ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_attachTrustedStore(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    path: JString<'_>,
) -> jint {
    let Some(path) = java_string(&mut env, path) else {
        return -1;
    };
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        session.attach_trusted_store(path).map(|_| 0).unwrap_or(-2)
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getConnectedReceiverId(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jstring {
    sender_string(&mut env, handle, SenderSession::connected_receiver_id)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getConnectedReceiverDisplayName(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jstring {
    sender_string(
        &mut env,
        handle,
        SenderSession::connected_receiver_display_name,
    )
}

fn with_trusted_store<R>(handle: jlong, f: impl FnOnce(&mut TrustedStoreInner) -> R) -> Option<R> {
    let mut handles = trusted_stores().lock().ok()?;
    Some(f(handles.values.get_mut(&handle)?))
}

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
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_removeTrustedDevice(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    device_id: JString<'_>,
) -> jint {
    let Some(device_id) = java_string(&mut env, device_id) else {
        return -1;
    };
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        match session.remove_trusted_device(&device_id) {
            Ok(removed) => i32::from(removed),
            Err(_) => -2,
        }
    })
    .unwrap_or(-1)
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

fn with_identity<R>(handle: jlong, f: impl FnOnce(&mut DeviceIdentity) -> R) -> Option<R> {
    let mut handles = identities().lock().ok()?;
    Some(f(handles.values.get_mut(&handle)?))
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_loadOrCreateIdentity(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    path: JString<'_>,
    default_name: JString<'_>,
) -> jlong {
    let Some(path) = java_string(&mut env, path) else {
        return 0;
    };
    let default_name = java_string(&mut env, default_name).unwrap_or_else(|| "Picoo Phone".into());
    let Ok(identity) = DeviceIdentity::load_or_create(path, &default_name) else {
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

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_exportDiagnosticsToPath(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    trusted_store_path: JString<'_>,
    platform: JString<'_>,
    app_version: JString<'_>,
    out_path: JString<'_>,
) -> jint {
    let (Some(trusted_store_path), Some(platform), Some(app_version), Some(out_path)) = (
        java_string(&mut env, trusted_store_path),
        java_string(&mut env, platform),
        java_string(&mut env, app_version),
        java_string(&mut env, out_path),
    ) else {
        return -1;
    };
    match export_diagnostics_from_trusted_path(&trusted_store_path, &platform, &app_version) {
        Ok(json) => std::fs::write(out_path, json).map(|_| 0).unwrap_or(-4),
        Err(code) => code,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_exportDiagnosticsToPathWithSession(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    trusted_store_path: JString<'_>,
    platform: JString<'_>,
    app_version: JString<'_>,
    role: JString<'_>,
    status: JString<'_>,
    access_units: jlong,
    packets: jlong,
    packets_dropped_unpaired: jlong,
    peer_host: JString<'_>,
    out_path: JString<'_>,
) -> jint {
    let (
        Some(trusted_store_path),
        Some(platform),
        Some(app_version),
        Some(role),
        Some(status),
        Some(out_path),
    ) = (
        java_string(&mut env, trusted_store_path),
        java_string(&mut env, platform),
        java_string(&mut env, app_version),
        java_string(&mut env, role),
        java_string(&mut env, status),
        java_string(&mut env, out_path),
    )
    else {
        return -1;
    };
    let hosts = optional_java_string(&mut env, peer_host)
        .map(|host| vec![host])
        .unwrap_or_default();
    let snapshot = DiagnosticSessionSnapshot {
        role,
        status,
        access_units: access_units as u64,
        packets: packets as u64,
        packets_dropped_unpaired: packets_dropped_unpaired as u64,
        hosts: Vec::new(),
    };
    match export_diagnostics_with_session(
        &trusted_store_path,
        &platform,
        &app_version,
        Some(snapshot),
        &hosts,
    ) {
        Ok(json) => std::fs::write(out_path, json).map(|_| 0).unwrap_or(-4),
        Err(code) => code,
    }
}
