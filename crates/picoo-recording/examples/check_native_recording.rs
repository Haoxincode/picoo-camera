//! Exercise the production ordered recorder with native synthetic encoder AUs.
//! REQ-PICOO-NEXT-018/021/022; no camera input or real camera frames are saved.
#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use picoo_bitstream::{Codec, CodecConfiguration};
    use picoo_packet::AssembledAccessUnit;
    use picoo_protocol::control::{StreamConfig, VideoFormat};
    use picoo_recording::{
        bundle::RecordingState, encoded::EncodedWriter, ingress::RecordingInput,
    };
    use std::{path::PathBuf, sync::Arc, time::Instant};

    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(args.next().ok_or("missing synthetic input directory")?);
    let output = PathBuf::from(args.next().ok_or("missing output directory")?);
    std::fs::create_dir_all(&output)?;
    for (wire, codec) in [(1, Codec::Avc), (2, Codec::Hevc)] {
        for (width, height) in [(1280, 720), (1920, 1080)] {
            for fps in [30, 60] {
                let stem = format!("{wire}-{height}-{fps}");
                let configuration = CodecConfiguration::parse(
                    codec,
                    std::fs::read(source.join(format!("{stem}.config")))?.into(),
                )?;
                configuration.validate_visible_size(width, height)?;
                let format = VideoFormat::from_codec_configuration(&configuration, fps)?;
                let config = Arc::new(StreamConfig {
                    codec: format.codec,
                    profile: format.profile,
                    level_idc: u32::from(configuration.level_idc()),
                    codec_configuration: configuration.record().to_vec(),
                    width,
                    height,
                    fps,
                    stream_epoch: 1,
                    color_range: format.color.ok_or("missing source color")?.range,
                    ..Default::default()
                });
                let data = std::fs::read(source.join(format!("{stem}.au")))?;
                let parent = output.join(&stem);
                // A repeated run must choose a new output directory.
                std::fs::create_dir(&parent)?;
                let mut writer = EncodedWriter::create(&parent)?;
                for frame in 0..=3 * fps {
                    let pts_us = u64::from(frame) * 1_000_000 / u64::from(fps);
                    writer.write_ordered(
                        RecordingInput::new(
                            1,
                            Arc::clone(&config),
                            AssembledAccessUnit {
                                data: data.clone().into(),
                                frame_id: u64::from(frame) + 1,
                                pts_us,
                                encoded_at_us: pts_us,
                                keyframe: false,
                                discardable: false,
                                stream_epoch: 1,
                                fragment_count: 1,
                                first_fragment_at: Instant::now(),
                            },
                        )
                        .map_err(|error| format!("recording ingress: {error:?}"))?,
                    )?;
                }
                writer.finish()?;
                if writer.state() != RecordingState::Complete {
                    return Err(format!("{stem}: {:?}", writer.state()).into());
                }
                println!("PASS {stem}: {}", writer.path().display());
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Production ordered recording validation currently requires macOS");
    std::process::exit(1);
}
