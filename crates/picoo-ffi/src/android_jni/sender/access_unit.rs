//! Atomic MediaCodec access-unit handoff into the Rust Sender session.

use jni::objects::{JByteArray, JObject};
use jni::sys::{jboolean, jint, jlong, JNI_TRUE};
use jni::JNIEnv;
use picoo_packet::extract_sps_pps;
use picoo_sender::StreamConfigParams;

use super::super::with_sender;

const SUBMIT_ENCODER_ACCEPTED: jint = 1;
const SUBMIT_STREAM_CONFIGURED: jint = 1 << 1;
const SUBMIT_KEYFRAME_REQUESTED: jint = 1 << 2;

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_submitEncoderAccessUnit(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    data: JByteArray<'_>,
    keyframe: jboolean,
    pts_us: jlong,
    encoded_at_us: jlong,
    stream_epoch: jint,
    encoder_generation: jlong,
    encoder_width: jint,
    encoder_height: jint,
    configure_stream: jboolean,
    mirrored: jboolean,
    sps: JByteArray<'_>,
    pps: JByteArray<'_>,
) -> jint {
    if stream_epoch <= 0 || encoder_generation <= 0 || encoder_width <= 0 || encoder_height <= 0 {
        return -1;
    }
    let Ok(data) = env.convert_byte_array(data) else {
        return -1;
    };
    if data.is_empty() {
        return -1;
    }
    let convert_optional = |array: JByteArray<'_>| {
        if array.is_null() {
            Ok(Vec::new())
        } else {
            env.convert_byte_array(array)
        }
    };
    let Ok(sps) = convert_optional(sps) else {
        return -1;
    };
    let Ok(pps) = convert_optional(pps) else {
        return -1;
    };

    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        let stream_epoch = stream_epoch as u32;
        let transaction_id = session.encoder_transaction_id_for_epoch(stream_epoch);
        if !session.report_encoder_started(
            transaction_id,
            encoder_generation as u64,
            stream_epoch,
            encoder_height as u32,
        ) {
            let _ = session.pump();
            return if session.take_keyframe_request() {
                SUBMIT_KEYFRAME_REQUESTED
            } else {
                0
            };
        }

        let mut result = SUBMIT_ENCODER_ACCEPTED;
        if configure_stream == JNI_TRUE && keyframe == JNI_TRUE {
            let (sps, pps) = if pps.is_empty() {
                extract_sps_pps(&sps).unwrap_or((sps, pps))
            } else {
                (sps, pps)
            };
            let bitrate_bps = session.current_bitrate_bps();
            session.set_stream_config(StreamConfigParams {
                width: encoder_width as u32,
                height: encoder_height as u32,
                fps: 30,
                bitrate_bps,
                stream_epoch,
                mirrored: mirrored == JNI_TRUE,
                rotation: 0,
                sps,
                pps,
            });
            result |= SUBMIT_STREAM_CONFIGURED;
        }
        let fragments = session.ingest_encoder_access_unit(picoo_sender::NativeEncoderAccessUnit {
            data: &data,
            is_keyframe: keyframe == JNI_TRUE,
            pts_us: pts_us as u64,
            encoded_at_us: encoded_at_us as u64,
            transaction_id,
            encoder_generation: encoder_generation as u64,
            stream_epoch,
            height: encoder_height as u32,
        });
        if fragments.is_err() {
            return -2;
        }
        if fragments.is_ok_and(|count| count > 0) && session.flush_pending().is_err() {
            return -2;
        }
        if session.pump().is_err() {
            return -2;
        }
        if session.take_keyframe_request() {
            result |= SUBMIT_KEYFRAME_REQUESTED;
        }
        result
    })
    .unwrap_or(-1)
}
