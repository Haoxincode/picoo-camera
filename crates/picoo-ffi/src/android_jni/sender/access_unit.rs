//! Atomic MediaCodec access-unit handoff — REQ-PICOO-MEDIA-021/022.
//! MediaCodec Annex B input is normalized to canonical four-byte NAL lengths
//! before Core state mutation (REQ-PICOO-PROTOCOL-020).

use jni::objects::{JByteArray, JObject, JValue};
use jni::sys::{jboolean, jint, jlong, jobject, JNI_TRUE};
use jni::JNIEnv;
use picoo_sender::{EncoderEventOutcome, NativeEncoderEvent, StreamConfigParams};

use super::super::with_sender;

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_submitEncoderAccessUnit(
    mut env: JNIEnv<'_>,
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
) -> jobject {
    let outcome = (|| -> Result<EncoderEventOutcome, jint> {
        if stream_epoch <= 0 || encoder_generation <= 0 || encoder_width <= 0 || encoder_height <= 0
        {
            return Err(-1);
        }
        let Some(codec) = super::native_codec(codec) else {
            return Err(-2);
        };
        let Ok(length) = env.get_array_length(&data) else {
            return Err(-1);
        };
        if !(1..=2_097_152).contains(&length) || !matches!(fps, 30 | 60) {
            return Err(-2);
        }
        let Ok(data) = env.convert_byte_array(data) else {
            return Err(-1);
        };
        if data.is_empty() {
            return Err(-1);
        }
        // Explicit MediaCodec byte-buffer framing is adapted before Core staging.
        let Ok(data) = crate::encoder_input::canonical(
            codec,
            picoo_bitstream::NalFormat::AnnexB,
            &data,
            keyframe == JNI_TRUE,
        ) else {
            return Err(-2);
        };
        let configuration = if configure_stream == JNI_TRUE {
            let Ok(length) = env.get_array_length(&codec_configuration) else {
                return Err(-1);
            };
            if !(1..=65536).contains(&length) {
                return Err(-2);
            }
            let Ok(record) = env.convert_byte_array(codec_configuration) else {
                return Err(-1);
            };
            let Ok(record) = picoo_bitstream::CodecConfiguration::parse(codec, record.into())
            else {
                return Err(-2);
            };
            Some(record)
        } else {
            None
        };

        with_sender(handle, |inner| {
            let Ok(mut session) = inner.session.lock() else {
                return Err(-1);
            };
            let stream_epoch = stream_epoch as u32;
            let stream_config = if configure_stream == JNI_TRUE && keyframe == JNI_TRUE {
                let Some(configuration) = configuration else {
                    return Err(-2);
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
                .map_err(|_| -2)
        })
        .ok_or(-1)?
    })();
    submission_object(&mut env, outcome)
}

fn submission_object(env: &mut JNIEnv<'_>, outcome: Result<EncoderEventOutcome, jint>) -> jobject {
    let object = match outcome {
        Ok(outcome) if outcome.encoder_accepted => env.new_object(
            "com/picoo/camera/media/EncoderSubmitOutcome$Accepted",
            "(ZZ)V",
            &[
                JValue::Bool(outcome.stream_configured.into()),
                JValue::Bool(outcome.keyframe_requested.into()),
            ],
        ),
        Ok(outcome) => env.new_object(
            "com/picoo/camera/media/EncoderSubmitOutcome$Rejected",
            "(Z)V",
            &[JValue::Bool(outcome.keyframe_requested.into())],
        ),
        Err(code) => env.new_object(
            "com/picoo/camera/media/EncoderSubmitOutcome$Error",
            "(I)V",
            &[JValue::Int(code)],
        ),
    };
    match object {
        Ok(object) => object.into_raw(),
        Err(error) => {
            // Allocation/class errors cannot be represented by a success object.
            // Preserve an existing JVM exception (including OutOfMemoryError).
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/IllegalStateException", error.to_string());
            }
            std::ptr::null_mut()
        }
    }
}
