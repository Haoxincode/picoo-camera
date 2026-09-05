//! Atomic MediaCodec access-unit handoff — REQ-PICOO-MEDIA-021/022.

use jni::objects::{JByteArray, JObject};
use jni::sys::{jboolean, jint, jlong, JNI_TRUE};
use jni::JNIEnv;
use picoo_sender::{NativeEncoderEvent, StreamConfigParams};

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
        let stream_config =
            (configure_stream == JNI_TRUE && keyframe == JNI_TRUE).then(|| StreamConfigParams {
                width: encoder_width as u32,
                height: encoder_height as u32,
                fps: 30,
                bitrate_bps: session.current_bitrate_bps(),
                stream_epoch,
                mirrored: mirrored == JNI_TRUE,
                rotation: 0,
                sps,
                pps,
            });
        session
            .submit_encoder_event(NativeEncoderEvent {
                data: &data,
                is_keyframe: keyframe == JNI_TRUE,
                pts_us: pts_us as u64,
                encoded_at_us: encoded_at_us as u64,
                encoder_generation: encoder_generation as u64,
                stream_epoch,
                width: encoder_width as u32,
                height: encoder_height as u32,
                stream_config,
            })
            .map(|outcome| {
                let mut result = 0;
                if outcome.encoder_accepted {
                    result |= SUBMIT_ENCODER_ACCEPTED;
                }
                if outcome.stream_configured {
                    result |= SUBMIT_STREAM_CONFIGURED;
                }
                if outcome.keyframe_requested {
                    result |= SUBMIT_KEYFRAME_REQUESTED;
                }
                result
            })
            .unwrap_or(-2)
    })
    .unwrap_or(-1)
}
