use std::ptr;
use std::sync::Mutex;
use std::time::Duration;

use jni::objects::{JByteArray, JIntArray, JObject, JString};
use jni::sys::{jboolean, jdoubleArray, jint, jlong, jlongArray, jobjectArray, jstring, JNI_TRUE};
use jni::JNIEnv;
use picoo_packet::extract_sps_pps;
use picoo_rate_control::BitrateLadder;
use picoo_sender::{EncoderFailureOutcome, SenderError, SenderSession, StreamConfigParams};
use picoo_transport::{ClientNetworkBinding, Endpoint, QuicSenderTransport, TransportError};

use super::{identities, java_string, new_java_string, senders, with_sender};
use crate::c_sender::sender_snapshot;
use crate::handles::SenderInner;

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_getProtocolName(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jstring {
    new_java_string(&mut env, picoo_protocol::ALPN)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_createSender(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    identity_handle: jlong,
) -> jlong {
    let identity = identities()
        .lock()
        .ok()
        .and_then(|handles| handles.values.get(&identity_handle).cloned());
    let Some(identity) = identity else {
        return 0;
    };
    let transport = QuicSenderTransport::new();
    let event_wake = transport.event_wake();
    let inner = SenderInner {
        session: Mutex::new(SenderSession::new_with_identity(transport, identity)),
        event_wake,
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
    encoded_at_us: jlong,
    stream_epoch: jint,
    transaction_id: jlong,
    encoder_generation: jlong,
    encoder_height: jint,
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
            .ingest_encoder_access_unit(picoo_sender::NativeEncoderAccessUnit {
                data: &data,
                is_keyframe: keyframe == JNI_TRUE,
                pts_us: pts_us as u64,
                encoded_at_us: encoded_at_us as u64,
                transaction_id: transaction_id as u64,
                encoder_generation: encoder_generation as u64,
                stream_epoch: stream_epoch as u32,
                height: encoder_height as u32,
            })
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
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_waitForSenderEvent(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    after_revision: jlong,
    timeout_ms: jint,
) -> jlong {
    let wake = with_sender(handle, |inner| inner.event_wake.clone());
    let Some(wake) = wake else {
        return after_revision;
    };
    wake.wait_after(
        after_revision as u64,
        Duration::from_millis(timeout_ms.max(0) as u64),
    ) as jlong
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
        match session.connect(Endpoint {
            host,
            port: port as u16,
        }) {
            Ok(_) => 0,
            Err(SenderError::Transport(TransportError::NetworkBindingFailed(_))) => -3,
            Err(_) => -2,
        }
    })
    .unwrap_or(-1)
}

/// Bind future Quinn UDP sockets to Android's physical Wi-Fi `Network`.
#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_setNetworkHandle(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    network_handle: jlong,
    allow_system_lan_route_fallback: jni::sys::jboolean,
) -> jint {
    if network_handle <= 0 {
        return -1;
    }
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        session
            .transport_mut()
            .set_network_binding(ClientNetworkBinding::AndroidNetwork {
                network_handle: network_handle as u64,
                allow_system_lan_route_fallback: allow_system_lan_route_fallback != 0,
            });
        0
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
#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_beginStreamReconfiguration(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    target_height: jint,
) -> jint {
    let Ok(target_height) = u32::try_from(target_height) else {
        return 0;
    };
    if target_height == 0 {
        return 0;
    }
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return 0;
        };
        session.begin_stream_reconfiguration(target_height) as jint
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_encoderTransactionId(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    stream_epoch: jint,
) -> jlong {
    if stream_epoch <= 0 {
        return 0;
    }
    with_sender(handle, |inner| {
        inner
            .session
            .lock()
            .map(|session| session.encoder_transaction_id_for_epoch(stream_epoch as u32) as jlong)
            .unwrap_or(0)
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_reportEncoderStarted(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    transaction_id: jlong,
    encoder_generation: jlong,
    stream_epoch: jint,
    height: jint,
) -> jint {
    if transaction_id < 0 || encoder_generation <= 0 || stream_epoch <= 0 || height <= 0 {
        return -1;
    }
    with_sender(handle, |inner| {
        inner
            .session
            .lock()
            .map(|mut session| {
                i32::from(session.report_encoder_started(
                    transaction_id as u64,
                    encoder_generation as u64,
                    stream_epoch as u32,
                    height as u32,
                ))
            })
            .unwrap_or(-1)
    })
    .unwrap_or(-1)
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_reportEncoderFailed(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    transaction_id: jlong,
    encoder_generation: jlong,
) -> jint {
    if transaction_id < 0 || encoder_generation < 0 {
        return -1;
    }
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        match session.report_encoder_failed(transaction_id as u64, encoder_generation as u64) {
            EncoderFailureOutcome::Ignored => 0,
            EncoderFailureOutcome::RolledBack => 1,
            EncoderFailureOutcome::RecoveryRequested => 2,
            EncoderFailureOutcome::Disconnected => 3,
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
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_sendClientHello(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jint {
    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        session.send_client_hello().map(|_| 0).unwrap_or(-2)
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
            stats.jitter_buffer_target_ms,
            stats.jitter_buffer_actual_delay_ms,
            stats.jitter_buffer_occupancy_ms,
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
