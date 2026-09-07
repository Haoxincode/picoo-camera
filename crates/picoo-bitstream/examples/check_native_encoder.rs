//! Validate production Apple encoder harness output — REQ-PICOO-MEDIA-045.
use picoo_bitstream::{
    AccessUnit, Codec, CodecConfiguration, NalFormat, NalLengthSize, PictureKind, RandomAccessPoint,
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("missing output directory")?,
    );
    for (wire, codec, rap) in [
        (1, Codec::Avc, RandomAccessPoint::AvcIdr),
        (2, Codec::Hevc, RandomAccessPoint::HevcIdr),
    ] {
        for (width, height) in [(1280, 720), (1920, 1080)] {
            for fps in [30, 60] {
                let stem = format!("{wire}-{height}-{fps}");
                let record = CodecConfiguration::parse(
                    codec,
                    std::fs::read(directory.join(format!("{stem}.config")))?.into(),
                )?;
                record.validate_visible_size(width, height)?;
                let bytes = std::fs::read(directory.join(format!("{stem}.au")))?;
                let picture = AccessUnit::parse(
                    codec,
                    NalFormat::LengthPrefixed(NalLengthSize::Four),
                    &bytes,
                )?;
                record.validate_parameter_sets(&picture)?;
                if picture.picture().kind != PictureKind::RandomAccess(rap) {
                    return Err(format!("{stem}: native sync is not closed IDR").into());
                }
                let facts = record.source_facts()?;
                if facts.color
                    != Some(picoo_bitstream::VideoColorFacts {
                        full_range: false,
                        primaries: 1,
                        transfer: 1,
                        matrix: 1,
                    })
                {
                    return Err(
                        format!("{stem}: missing or conflicting BT.709 limited color").into(),
                    );
                }
                println!(
                    "PASS {stem} coded={}x{} color={:?}",
                    facts.coded_width, facts.coded_height, facts.color
                );
            }
        }
    }
    Ok(())
}
