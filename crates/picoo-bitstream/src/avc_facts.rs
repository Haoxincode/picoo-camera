//! Bounded AVC geometry and VUI facts — REQ-PICOO-BITSTREAM-004.
//! h264-reader parses standard syntax; Picoo admits progressive 8-bit 4:2:0.

use crate::BitstreamError;
use h264_reader::nal::{
    sps::{AspectRatioInfo, ChromaFormat, FrameMbsFlags, SeqParameterSet as Sps},
    Nal, RefNal,
};

use crate::{VideoColorFacts, VideoSpsFacts};

impl VideoSpsFacts {
    pub fn parse_avc(nal: &[u8]) -> Result<Self, BitstreamError> {
        if nal.len() > 64 * 1024 {
            return Err(BitstreamError::Limit);
        }
        if nal.first().is_none_or(|byte| byte & 0x9f != 7) {
            return Err(BitstreamError::Malformed("expected SPS NAL"));
        }
        Self::from_sps(parse_sps(nal)?)
    }

    fn from_sps(sps: Sps) -> Result<Self, BitstreamError> {
        if sps.frame_mbs_flags != FrameMbsFlags::Frames {
            return Err(BitstreamError::Unsupported("interlaced AVC"));
        }
        let chroma = &sps.chroma_info;
        if chroma.chroma_format != ChromaFormat::YUV420
            || chroma.bit_depth_luma_minus8 != 0
            || chroma.bit_depth_chroma_minus8 != 0
        {
            return Err(BitstreamError::Unsupported("requires 8-bit 4:2:0 AVC"));
        }
        let dimension = |blocks: u32| -> Result<u32, BitstreamError> {
            blocks
                .checked_add(1)
                .and_then(|n| n.checked_mul(16))
                .filter(|n| *n <= 8192)
                .ok_or(BitstreamError::Limit)
        };
        let coded_width = dimension(sps.pic_width_in_mbs_minus1)?;
        let coded_height = dimension(sps.pic_height_in_map_units_minus1)?;
        let crop = |n: u32| -> Result<u32, BitstreamError> {
            n.checked_mul(2).ok_or(BitstreamError::Limit)
        };
        let (left, right, top, bottom) = match sps.frame_cropping {
            Some(c) => (
                crop(c.left_offset)?,
                crop(c.right_offset)?,
                crop(c.top_offset)?,
                crop(c.bottom_offset)?,
            ),
            None => (0, 0, 0, 0),
        };
        let visible = |coded: u32, before: u32, after: u32| {
            coded
                .checked_sub(before)
                .and_then(|n| n.checked_sub(after))
                .filter(|n| *n > 0)
                .ok_or(BitstreamError::Malformed(
                    "SPS crop removes the entire image",
                ))
        };
        let vui = sps.vui_parameters.as_ref();
        let aspect = vui.and_then(|v| v.aspect_ratio_info.as_ref());
        if matches!(aspect, Some(AspectRatioInfo::Reserved(_))) {
            return Err(BitstreamError::Unsupported("reserved AVC aspect ratio"));
        }
        let pixel_aspect_ratio = aspect
            .and_then(|a| a.get())
            .map(|(w, h)| (u32::from(w), u32::from(h)));
        let chroma_location = match vui.and_then(|v| v.chroma_loc_info.as_ref()) {
            None => 0,
            Some(loc)
                if loc.chroma_sample_loc_type_top_field
                    == loc.chroma_sample_loc_type_bottom_field
                    && loc.chroma_sample_loc_type_top_field <= 5 =>
            {
                loc.chroma_sample_loc_type_top_field as u8
            }
            Some(_) => {
                return Err(BitstreamError::Unsupported(
                    "invalid progressive chroma location",
                ))
            }
        };
        Ok(Self {
            coded_width,
            coded_height,
            visible_x: left,
            visible_y: top,
            visible_width: visible(coded_width, left, right)?,
            visible_height: visible(coded_height, top, bottom)?,
            pixel_aspect_ratio,
            color: vui
                .and_then(|v| v.video_signal_type.as_ref())
                .map(|c| VideoColorFacts {
                    full_range: c.video_full_range_flag,
                    primaries: c
                        .colour_description
                        .as_ref()
                        .map_or(2, |c| c.colour_primaries),
                    transfer: c
                        .colour_description
                        .as_ref()
                        .map_or(2, |c| c.transfer_characteristics),
                    matrix: c
                        .colour_description
                        .as_ref()
                        .map_or(2, |c| c.matrix_coefficients),
                }),
            chroma_location,
        })
    }
}

fn parse_sps(nal: &[u8]) -> Result<Sps, BitstreamError> {
    Sps::from_bits(RefNal::new(nal, &[], true).rbsp_bits())
        .map_err(|_| BitstreamError::Malformed("invalid AVC SPS"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn full_hd_sps() -> Vec<u8> {
        crate::avc::extract_sps_pps(include_bytes!(
            "../../picoo-testkit/fixtures/avc-1080p-red-idr.h264"
        ))
        .unwrap()
        .0
    }
    #[test]
    fn generated_hardware_sources_declare_bt709_without_inventing_missing_aspect() {
        for (au, width, height) in [
            (
                &include_bytes!("../../picoo-testkit/fixtures/avc-64x64-bt709-idr.h264")[..],
                64,
                64,
            ),
            (
                &include_bytes!("../../picoo-testkit/fixtures/avc-1280x720-bt709-idr.h264")[..],
                1280,
                720,
            ),
            (
                &include_bytes!("../../picoo-testkit/fixtures/avc-1920x1080-bt709-idr.h264")[..],
                1920,
                1080,
            ),
        ] {
            let (sps, _) = crate::avc::extract_sps_pps(au).unwrap();
            let facts = VideoSpsFacts::parse_avc(&sps).unwrap();
            assert_eq!((facts.visible_width, facts.visible_height), (width, height));
            // VideoToolbox omits square SAR from the elementary SPS even when the
            // compression session explicitly commits it; wire/native metadata must carry it.
            assert_eq!(facts.pixel_aspect_ratio, None);
            assert_eq!(
                facts.color,
                Some(VideoColorFacts {
                    full_range: false,
                    primaries: 1,
                    transfer: 1,
                    matrix: 1
                })
            );
        }
    }
    #[test]
    fn native_avc_full_hd_preserves_coded_padding_and_visible_crop() {
        let facts = VideoSpsFacts::parse_avc(&full_hd_sps()).unwrap();
        assert_eq!((facts.coded_width, facts.coded_height), (1920, 1088));
        assert_eq!((facts.visible_width, facts.visible_height), (1920, 1080));
        assert_eq!((facts.visible_x, facts.visible_y), (0, 0));
    }
    #[test]
    fn malformed_geometry_is_rejected_without_arithmetic_panics() {
        let mut sps = parse_sps(full_hd_sps().as_slice()).unwrap();
        sps.pic_width_in_mbs_minus1 = u32::MAX;
        assert!(matches!(
            VideoSpsFacts::from_sps(sps),
            Err(BitstreamError::Limit)
        ));
        let mut sps = parse_sps(full_hd_sps().as_slice()).unwrap();
        sps.frame_cropping.as_mut().unwrap().bottom_offset = 544;
        assert!(VideoSpsFacts::from_sps(sps).is_err());
    }
    #[test]
    fn unspecified_color_and_aspect_are_not_invented() {
        let mut sps = parse_sps(full_hd_sps().as_slice()).unwrap();
        sps.vui_parameters = None;
        let facts = VideoSpsFacts::from_sps(sps).unwrap();
        assert_eq!(facts.color, None);
        assert_eq!(facts.pixel_aspect_ratio, None);
        assert_eq!(facts.chroma_location, 0);
    }
    #[test]
    fn oversized_exp_golomb_returns_error_instead_of_panicking() {
        assert!(VideoSpsFacts::parse_avc(include_bytes!(
            "../tests/fixtures/sps-exp-golomb-overflow.bin"
        ))
        .is_err());
    }
    #[test]
    fn truncated_and_oversized_sps_are_rejected() {
        assert!(VideoSpsFacts::parse_avc(&[0x67]).is_err());
        assert!(matches!(
            VideoSpsFacts::parse_avc(&vec![0x67; 65537]),
            Err(BitstreamError::Limit)
        ));
    }
}
