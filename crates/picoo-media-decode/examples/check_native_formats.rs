//! Real production Decoder validation; never advertises capabilities from fixtures.
use picoo_bitstream::{Codec, CodecConfiguration};
use picoo_media_decode::{
    create_platform_decoder, AccessUnitTimeline, DecodeSubmission, DecodeToken, FrameKind,
};
use picoo_protocol::control::{ColorRange, StreamConfig, VideoCodec, VideoProfile};
use std::{path::PathBuf, sync::Arc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("missing fixture directory")?,
    );
    let mut decoder = create_platform_decoder();
    let capabilities = picoo_media_decode::probe_capabilities(decoder.as_mut())?;
    let mut identity = 0;
    for (wire, codec, profile) in [
        (VideoCodec::Avc, Codec::Avc, VideoProfile::AvcHigh),
        (VideoCodec::Hevc, Codec::Hevc, VideoProfile::HevcMain),
    ] {
        for (width, height) in [(1280, 720), (1920, 1080)] {
            for fps in [30, 60] {
                identity += 1;
                let stem = format!("{}-{height}-{fps}", wire as i32);
                let bytes = std::fs::read(directory.join(format!("{stem}.config")))?;
                let record = CodecConfiguration::parse(codec, bytes.clone().into())?;
                let config = Arc::new(StreamConfig {
                    codec: wire as i32,
                    profile: profile as i32,
                    level_idc: u32::from(record.level_idc()),
                    width,
                    height,
                    fps,
                    color_range: ColorRange::Limited as i32,
                    codec_configuration: bytes,
                    stream_epoch: identity as u32,
                    ..Default::default()
                });
                let format = config.validated_video_format()?;
                let data = std::fs::read(directory.join(format!("{stem}.au")))?;
                if !capabilities.supports(&format, config.level_idc, u32::try_from(data.len())?) {
                    return Err(format!(
                        "{stem}: actual source has no probed decoder offer: {format:?}"
                    )
                    .into());
                }
                let token = Arc::new(DecodeToken {
                    timeline: AccessUnitTimeline {
                        connection_generation: 1,
                        stream_generation: identity,
                        frame_id: identity,
                        source_pts_us: identity * 1_000_000,
                        encoded_at_us: identity * 1_000_000,
                        received_at_us: identity * 1_000_000,
                        decode_submitted_at_us: identity * 1_000_000,
                        kind: FrameKind::Key,
                    },
                    decoder_generation: 1,
                    config_revision: identity,
                    stream_config: Some(config),
                });
                let outcome = decoder.submit(DecodeSubmission {
                    access_unit: &data,
                    token: token.clone(),
                })?;
                if !outcome.refresh_accepted || outcome.frames.len() != 1 {
                    return Err(format!("{stem}: no complete native IDR output").into());
                }
                let output = &outcome.frames[0];
                if !Arc::ptr_eq(&output.token, &token) {
                    return Err(format!("{stem}: original token lost").into());
                }
                #[cfg(any(target_os = "macos", windows))]
                {
                    let visible = output.frame.description().native_format.visible_rect;
                    if (visible.width, visible.height) != (width, height) {
                        return Err(format!("{stem}: output presentation differs").into());
                    }
                }
                println!(
                    "PASS {stem} source={:?} output={:?}; sustained_fps=not_measured",
                    format.coded_size,
                    output.frame.description()
                );
            }
        }
    }
    decoder.reset()?;
    Ok(())
}
