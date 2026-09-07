//! Atomic MediaCodec access-unit handoff — REQ-PICOO-MEDIA-021/022.
//! MediaCodec Annex B input is normalized to canonical four-byte NAL lengths
//! before Core state mutation (REQ-PICOO-PROTOCOL-020).

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
    codec: jint,
    fps: jint,
    codec_configuration: JByteArray<'_>,
) -> jint {
    if stream_epoch <= 0 || encoder_generation <= 0 || encoder_width <= 0 || encoder_height <= 0 {
        return -1;
    }
    let Some(codec) = super::native_codec(codec) else {
        return -2;
    };
    let Ok(length) = env.get_array_length(&data) else {
        return -1;
    };
    if !(1..=2_097_152).contains(&length) || !matches!(fps, 30 | 60) {
        return -2;
    }
    let Ok(data) = env.convert_byte_array(data) else {
        return -1;
    };
    if data.is_empty() {
        return -1;
    }
    // Explicit MediaCodec byte-buffer framing is adapted before Core staging.
    let Ok(data) = crate::encoder_input::canonical(
        codec,
        picoo_bitstream::NalFormat::AnnexB,
        &data,
        keyframe == JNI_TRUE,
    ) else {
        return -2;
    };
    let configuration = if configure_stream == JNI_TRUE {
        let Ok(length) = env.get_array_length(&codec_configuration) else {
            return -1;
        };
        if !(1..=65536).contains(&length) {
            return -2;
        }
        let Ok(record) = env.convert_byte_array(codec_configuration) else {
            return -1;
        };
        let Ok(record) = picoo_bitstream::CodecConfiguration::parse(codec, record.into()) else {
            return -2;
        };
        Some(record)
    } else {
        None
    };

    with_sender(handle, |inner| {
        let Ok(mut session) = inner.session.lock() else {
            return -1;
        };
        let stream_epoch = stream_epoch as u32;
        let stream_config = if configure_stream == JNI_TRUE && keyframe == JNI_TRUE {
            let Some(configuration) = configuration else {
                return -2;
            };
            Some(StreamConfigParams {
                width: encoder_width as u32,
                height: encoder_height as u32,
                fps: fps as u32,
                bitrate_bps: session.current_bitrate_bps(),
                stream_epoch,
                mirrored: mirrored == JNI_TRUE,
                rotation: 0,
                configuration: configuration.into(),
            })
        } else {
            None
        };
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
