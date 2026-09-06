//! HEVC Main source facts via the bounded Scuffle SPS parser — NEXT-003/025.
use crate::{BitstreamError, VideoColorFacts, VideoSpsFacts};
use scuffle_h265::{AspectRatioInfo, SpsNALUnit};

impl VideoSpsFacts {
    /// Admit single-layer, progressive Main 8-bit 4:2:0 with no picture reordering.
    /// This is source metadata admission, not proof of a native Decoder's capability.
    pub fn parse_hevc(nal: &[u8]) -> Result<Self, BitstreamError> {
        if nal.len() > 64 * 1024 {
            return Err(BitstreamError::Limit);
        }
        if nal.len() < 2 || nal[0] != 0x42 || nal[1] != 1 {
            return Err(BitstreamError::Unsupported("expected base-layer HEVC SPS"));
        }
        let mut cursor = std::io::Cursor::new(nal);
        let parsed = SpsNALUnit::parse(&mut cursor)?;
        if cursor.position() != nal.len() as u64 {
            return Err(BitstreamError::Malformed("trailing bytes after HEVC SPS"));
        }
        let sps = parsed.rbsp;
        let profile = &sps.profile_tier_level.general_profile;
        if profile.profile_space != 0
            || profile.profile_idc != 1
            || profile.interlaced_source_flag
            || sps.chroma_format_idc != 1
            || sps.separate_colour_plane_flag
            || sps.bit_depth_luma_minus8 != 0
            || sps.bit_depth_chroma_minus8 != 0
            || sps.range_extension.is_some()
            || sps.multilayer_extension.is_some()
            || sps.sps_3d_extension.is_some()
            || sps.scc_extension.is_some()
            || sps
                .sub_layer_ordering_info
                .sps_max_num_reorder_pics
                .iter()
                .any(|n| *n != 0)
            || sps
                .vui_parameters
                .as_ref()
                .is_some_and(|vui| vui.field_seq_flag)
        {
            return Err(BitstreamError::Unsupported(
                "requires progressive HEVC Main without reordering or extensions",
            ));
        }
        let dimension = |value: u64| -> Result<u32, BitstreamError> {
            if value == 0 || value > 8192 || !value.is_multiple_of(2) {
                Err(BitstreamError::Limit)
            } else {
                Ok(value as u32)
            }
        };
        let coded_width = dimension(sps.pic_width_in_luma_samples.get())?;
        let coded_height = dimension(sps.pic_height_in_luma_samples.get())?;
        let window = &sps.conformance_window;
        let vui = sps.vui_parameters.as_ref();
        let display = vui.map(|v| &v.default_display_window);
        let crop = |base: u64, extra: u64| {
            base.checked_add(extra)
                .and_then(|sum| sum.checked_mul(2))
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(BitstreamError::Limit)
        };
        let left = crop(
            window.conf_win_left_offset,
            display.map_or(0, |d| d.def_disp_win_left_offset),
        )?;
        let right = crop(
            window.conf_win_right_offset,
            display.map_or(0, |d| d.def_disp_win_right_offset),
        )?;
        let top = crop(
            window.conf_win_top_offset,
            display.map_or(0, |d| d.def_disp_win_top_offset),
        )?;
        let bottom = crop(
            window.conf_win_bottom_offset,
            display.map_or(0, |d| d.def_disp_win_bottom_offset),
        )?;
        let visible = |size: u32, before: u32, after: u32| {
            size.checked_sub(before)
                .and_then(|n| n.checked_sub(after))
                .filter(|n| *n > 0)
                .ok_or(BitstreamError::Malformed("HEVC crop removes image"))
        };
        let chroma_location = match vui.and_then(|v| v.chroma_loc_info.as_ref()) {
            Some(loc) if loc.top_field == loc.bottom_field && loc.top_field <= 5 => {
                loc.top_field as u8
            }
            Some(_) => {
                return Err(BitstreamError::Unsupported(
                    "HEVC field chroma locations differ",
                ))
            }
            None => 0,
        };
        Ok(Self {
            coded_width,
            coded_height,
            visible_x: left,
            visible_y: top,
            visible_width: visible(coded_width, left, right)?,
            visible_height: visible(coded_height, top, bottom)?,
            pixel_aspect_ratio: vui
                .map(|v| aspect(&v.aspect_ratio_info))
                .transpose()?
                .flatten(),
            color: vui.map(|v| VideoColorFacts {
                full_range: v.video_signal_type.video_full_range_flag,
                primaries: v.video_signal_type.colour_primaries,
                transfer: v.video_signal_type.transfer_characteristics,
                matrix: v.video_signal_type.matrix_coeffs,
            }),
            chroma_location,
        })
    }
}

fn aspect(info: &AspectRatioInfo) -> Result<Option<(u32, u32)>, BitstreamError> {
    const RATIOS: [(u32, u32); 16] = [
        (1, 1),
        (12, 11),
        (10, 11),
        (16, 11),
        (40, 33),
        (24, 11),
        (20, 11),
        (32, 11),
        (80, 33),
        (18, 11),
        (15, 11),
        (64, 33),
        (160, 99),
        (4, 3),
        (3, 2),
        (2, 1),
    ];
    match info {
        AspectRatioInfo::Predefined(id) if id.0 == 0 => Ok(None),
        AspectRatioInfo::Predefined(id) => RATIOS
            .get(usize::from(id.0) - 1)
            .copied()
            .map(Some)
            .ok_or(BitstreamError::Unsupported("reserved HEVC pixel aspect")),
        AspectRatioInfo::ExtendedSar {
            sar_width,
            sar_height,
        } if *sar_width != 0 && *sar_height != 0 => {
            Ok(Some((u32::from(*sar_width), u32::from(*sar_height))))
        }
        _ => Err(BitstreamError::Malformed("zero HEVC pixel aspect")),
    }
}
