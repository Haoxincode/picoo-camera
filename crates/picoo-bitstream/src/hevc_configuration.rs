//! Native MediaCodec CSD adaptation using Scuffle's SPS and hvcC implementations.
use crate::{BitstreamError, Codec, CodecConfiguration, NalFormat};
use bytes::Bytes;
use scuffle_h265::{
    ConstantFrameRate, HEVCDecoderConfigurationRecord, NALUnitType, NaluArray, NumTemporalLayers,
    ParallelismType, Profile, ProfileAdditionalFlags,
};

impl CodecConfiguration {
    /// REQ-PICOO-NEXT-003/025: explicit Annex B codec-specific data, never a picture.
    pub fn from_hevc_annex_b(data: &[u8]) -> Result<Self, BitstreamError> {
        if data.len() > 64 * 1024 {
            return Err(BitstreamError::Limit);
        }
        let mut sets: [Option<&[u8]>; 3] = [None; 3];
        for nal in crate::split_nals(NalFormat::AnnexB, data)? {
            let kind = crate::access_unit::nal_type(Codec::Hevc, nal)?;
            if !(32..=34).contains(&kind) {
                return Err(BitstreamError::Malformed(
                    "HEVC codec-config contains non-parameter NAL",
                ));
            }
            let target = &mut sets[(kind - 32) as usize];
            if target.is_some_and(|previous| previous != nal) {
                return Err(BitstreamError::Unsupported(
                    "multiple distinct native HEVC parameter sets",
                ));
            }
            *target = Some(nal);
        }
        let [Some(vps), Some(sps), Some(pps)] = sets else {
            return Err(BitstreamError::Malformed("missing HEVC parameter sets"));
        };
        // Source admission rejects unsupported syntax before record synthesis.
        let (parsed, _) = crate::hevc_facts::parse_source(sps)?;
        let profile = &parsed.profile_tier_level.general_profile;
        let record = HEVCDecoderConfigurationRecord {
            general_profile_space: profile.profile_space,
            general_tier_flag: profile.tier_flag,
            general_profile_idc: profile.profile_idc,
            general_profile_compatibility_flags: profile.profile_compatibility_flag,
            general_constraint_indicator_flags: constraints(profile),
            general_level_idc: profile
                .level_idc
                .ok_or(BitstreamError::Malformed("missing HEVC level"))?,
            // Unknown is explicit: no PPS tile/WPP interpretation or measured fps here.
            min_spatial_segmentation_idc: 0,
            parallelism_type: ParallelismType::MixedOrUnknown,
            chroma_format_idc: parsed.chroma_format_idc,
            bit_depth_luma_minus8: parsed.bit_depth_luma_minus8,
            bit_depth_chroma_minus8: parsed.bit_depth_chroma_minus8,
            avg_frame_rate: 0,
            constant_frame_rate: ConstantFrameRate::Unknown,
            num_temporal_layers: NumTemporalLayers::from(parsed.sps_max_sub_layers_minus1 + 1),
            temporal_id_nested: parsed.sps_temporal_id_nesting_flag,
            length_size_minus_one: 3,
            arrays: [
                (NALUnitType::VpsNut, vps),
                (NALUnitType::SpsNut, sps),
                (NALUnitType::PpsNut, pps),
            ]
            .into_iter()
            .map(|(nal_unit_type, nal)| NaluArray {
                array_completeness: true,
                nal_unit_type,
                nalus: vec![Bytes::copy_from_slice(nal)],
            })
            .collect(),
        };
        let mut bytes = Vec::with_capacity(record.size() as usize);
        record.mux(&mut bytes)?;
        Self::parse(Codec::Hevc, bytes.into())
    }
}

// ISO/IEC 14496-15 hvcC stores the parsed profile constraint fields as 48 bits.
// Standard syntax interpretation remains in Scuffle; reserved output bits are zero.
fn constraints(profile: &Profile) -> u64 {
    let mut bits = (u64::from(profile.progressive_source_flag) << 47)
        | (u64::from(profile.interlaced_source_flag) << 46)
        | (u64::from(profile.non_packed_constraint_flag) << 45)
        | (u64::from(profile.frame_only_constraint_flag) << 44)
        | u64::from(profile.inbld_flag.unwrap_or(false));
    match profile.additional_flags {
        ProfileAdditionalFlags::None => {}
        ProfileAdditionalFlags::Main10Profile {
            one_picture_only_constraint_flag,
        } => {
            bits |= u64::from(one_picture_only_constraint_flag) << 36;
        }
        ProfileAdditionalFlags::Full {
            max_12bit_constraint_flag,
            max_10bit_constraint_flag,
            max_8bit_constraint_flag,
            max_422chroma_constraint_flag,
            max_420chroma_constraint_flag,
            max_monochrome_constraint_flag,
            intra_constraint_flag,
            one_picture_only_constraint_flag,
            lower_bit_rate_constraint_flag,
            max_14bit_constraint_flag,
        } => {
            for (flag, shift) in [
                (max_12bit_constraint_flag, 43),
                (max_10bit_constraint_flag, 42),
                (max_8bit_constraint_flag, 41),
                (max_422chroma_constraint_flag, 40),
                (max_420chroma_constraint_flag, 39),
                (max_monochrome_constraint_flag, 38),
                (intra_constraint_flag, 37),
                (one_picture_only_constraint_flag, 36),
                (lower_bit_rate_constraint_flag, 35),
                (max_14bit_constraint_flag.unwrap_or(false), 34),
            ] {
                bits |= u64::from(flag) << shift;
            }
        }
    }
    bits
}
