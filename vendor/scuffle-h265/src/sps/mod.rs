use std::io;
use std::num::NonZero;

use scuffle_bytes_util::{BitReader, EmulationPreventionIo, range_check};
use scuffle_expgolomb::BitReaderExpGolombExt;

use crate::NALUnitType;
use crate::nal_unit_header::NALUnitHeader;
use crate::rbsp_trailing_bits::rbsp_trailing_bits;

mod bounds;
mod conformance_window;
mod long_term_ref_pics;
mod pcm;
mod profile_tier_level;
mod scaling_list;
mod sps_3d_extension;
mod sps_multilayer_extension;
mod sps_range_extension;
mod sps_scc_extension;
mod st_ref_pic_set;
mod sub_layer_ordering_info;
mod vui_parameters;

pub use conformance_window::*;
pub use long_term_ref_pics::*;
pub use pcm::*;
pub use profile_tier_level::*;
pub use scaling_list::*;
pub use sps_3d_extension::*;
pub use sps_multilayer_extension::*;
pub use sps_range_extension::*;
pub use sps_scc_extension::*;
pub use st_ref_pic_set::*;
pub use sub_layer_ordering_info::*;
pub use vui_parameters::*;

// Some notes on the spec:
//
// The data appears like this on the wire: `NALU(RBSP(SODB))`
//
// NALU: NAL unit
// This is the outer most encapsulation layer and what is sent over the wire.
//
// RBSP: Raw byte sequence payload
// Additional encapsulation layer that adds trailing bits and emulation prevention.
//
// SODB: String of data bits
// This is the actual payload data.

/// Sequence parameter set contained in a NAL unit.
///
/// This only represents sequence parameter sets that are part of NAL units.
/// Therefore the NAL unit header is included in this struct as [`SpsNALUnit::nal_unit_header`].
#[derive(Debug, Clone, PartialEq)]
pub struct SpsNALUnit {
    /// The NAL unit header.
    pub nal_unit_header: NALUnitHeader,
    /// The SPS RBSP.
    pub rbsp: SpsRbsp,
}

impl SpsNALUnit {
    /// Parses an SPS NAL unit from the given reader.
    pub fn parse(mut reader: impl io::Read) -> io::Result<Self> {
        let nal_unit_header = NALUnitHeader::parse(&mut reader)?;
        if nal_unit_header.nal_unit_type != NALUnitType::SpsNut {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "nal_unit_type is not SPS_NUT"));
        }

        let rbsp = SpsRbsp::parse(reader, nal_unit_header.nuh_layer_id)?;

        Ok(SpsNALUnit { nal_unit_header, rbsp })
    }
}

/// Sequence parameter set RBSP.
///
/// For parsing SPS RBSPs that are part of NAL units, please use [`SpsNALUnit::parse`].
///
/// `seq_parameter_set_rbsp()`
///
/// - ISO/IEC 23008-2 - 7.3.2.2
/// - ISO/IEC 23008-2 - 7.4.3.2
#[derive(Debug, Clone, PartialEq)]
pub struct SpsRbsp {
    /// Specifies the value of the vps_video_parameter_set_id of the active VPS.
    pub sps_video_parameter_set_id: u8,
    /// This value plus 1 specifies the maximum number of temporal sub-layers that may be
    /// present in each CVS referring to the SPS.
    ///
    /// The value is in range \[0, 6\]. The value must be less than or equal to `vps_max_sub_layers_minus1`.
    pub sps_max_sub_layers_minus1: u8,
    /// Specifies whether inter prediction is additionally restricted for CVSs referring to the SPS.
    ///
    /// When `sps_max_sub_layers_minus1 == 0`, this flag is `true`.
    pub sps_temporal_id_nesting_flag: bool,
    /// The [`ProfileTierLevel`] structure contained in this SPS.
    pub profile_tier_level: ProfileTierLevel,
    /// Provides an identifier for the SPS for reference by other syntax elements.
    ///
    /// The value is in range \[0, 15\].
    pub sps_seq_parameter_set_id: u64,
    /// Specifies the chroma sampling relative to the luma sampling as specified in ISO/IEC 23008-2 - 6.2.
    ///
    /// The value is in range \[0, 3\].
    pub chroma_format_idc: u8,
    /// Equal to `true` specifies that the three colour components of the 4:4:4 chroma format are coded separately.
    ///
    /// Equal to `false` specifies that the colour components are not coded separately.
    ///
    /// Defines [`ChromaArrayType`](Self::chroma_array_type).
    pub separate_colour_plane_flag: bool,
    /// Specifies the width of each decoded picture in units of luma samples.
    ///
    /// This value is never zero and an integer multiple of [`MinCbSizeY`](Self::min_cb_size_y).
    pub pic_width_in_luma_samples: NonZero<u64>,
    /// Specifies the height of each decoded picture in units of luma samples.
    ///
    /// This value is never zero and an integer multiple of [`MinCbSizeY`](Self::min_cb_size_y).
    pub pic_height_in_luma_samples: NonZero<u64>,
    /// `conf_win_left_offset`, `conf_win_right_offset`, `conf_win_top_offset`, and `conf_win_bottom_offset`.
    ///
    /// See [`ConformanceWindow`] for details.
    pub conformance_window: ConformanceWindow,
    /// Specifies the bit depth of the samples of the luma array [`BitDepth_Y`](Self::bit_depth_y) and
    /// the value of the luma quantization parameter range offset [`QpBdOffset_Y`](Self::qp_bd_offset_y).
    ///
    /// The value is in range \[0, 8\].
    pub bit_depth_luma_minus8: u8,
    /// specifies the bit depth of the samples of the chroma arrays [`BitDepth_C`](Self::bit_depth_c) and
    /// the value of the chroma quantization parameter range offset [`QpBdOffset_C`](Self::qp_bd_offset_c)
    ///
    /// The value is in range \[0, 8\].
    pub bit_depth_chroma_minus8: u8,
    /// Specifies the value of the variable [`MaxPicOrderCntLsb`](Self::max_pic_order_cnt_lsb) that is used
    /// in the decoding process for picture order count.
    ///
    /// The value is in range \[0, 12\].
    pub log2_max_pic_order_cnt_lsb_minus4: u8,
    /// `sps_max_dec_pic_buffering_minus1`, `sps_max_num_reorder_pics`, and `sps_max_latency_increase_plus1` for each sub-layer.
    ///
    /// See [`SubLayerOrderingInfo`] for details.
    pub sub_layer_ordering_info: SubLayerOrderingInfo,
    /// This value plus 3 defines the minimum luma coding block size.
    ///
    /// Defines [`MinCbLog2SizeY`](Self::min_cb_log2_size_y).
    pub log2_min_luma_coding_block_size_minus3: u64,
    /// Specifies the difference between the maximum and minimum luma coding block size.
    pub log2_diff_max_min_luma_coding_block_size: u64,
    /// This value plus 2 specifies the minimum luma transform block size.
    ///
    /// Defines [`MinTbLog2SizeY`](Self::min_tb_log2_size_y).
    pub log2_min_luma_transform_block_size_minus2: u64,
    /// Specifies the difference between the maximum and minimum luma transform block size.
    ///
    /// Defines [`MaxTbLog2SizeY`](Self::max_tb_log2_size_y).
    pub log2_diff_max_min_luma_transform_block_size: u64,
    /// Specifies the maximum hierarchy depth for transform units of coding units coded in inter prediction mode.
    ///
    /// This value is in range \[0, [`CtbLog2SizeY`](Self::ctb_log2_size_y) - [`MinTbLog2SizeY`](Self::min_tb_log2_size_y)\].
    pub max_transform_hierarchy_depth_inter: u64,
    /// Specifies the maximum hierarchy depth for transform units of coding units coded in intra prediction mode.
    ///
    /// This value is in range \[0, [`CtbLog2SizeY`](Self::ctb_log2_size_y) - [`MinTbLog2SizeY`](Self::min_tb_log2_size_y)\].
    pub max_transform_hierarchy_depth_intra: u64,
    /// The [`ScalingListData`] structure contained in this SPS, if present.
    pub scaling_list_data: Option<ScalingListData>,
    /// Equal to `true` specifies that asymmetric motion partitions, i.e. `PartMode` equal to
    /// `PART_2NxnU`, `PART_2NxnD`, `PART_nLx2N`, or `PART_nRx2N`, may be used in CTBs.
    ///
    /// Equal to `false` specifies that asymmetric motion partitions cannot be used in CTBs.
    pub amp_enabled_flag: bool,
    /// Equal to `true` specifies that the sample adaptive offset process is applied to the reconstructed picture
    /// after the deblocking filter process.
    ///
    /// Equal to `false` specifies that the sample adaptive offset process is not
    /// applied to the reconstructed picture after the deblocking filter process.
    pub sample_adaptive_offset_enabled_flag: bool,
    /// `pcm_sample_bit_depth_luma_minus1`, `pcm_sample_bit_depth_chroma_minus1`, `log2_min_pcm_luma_coding_block_size_minus3`,
    /// `log2_diff_max_min_pcm_luma_coding_block_size` and `pcm_loop_filter_disabled_flag`, if `pcm_enabled_flag` is `true`.
    ///
    /// See [`Pcm`] for details.
    pub pcm: Option<Pcm>,
    /// The [`ShortTermRefPicSets`] structure contained in this SPS.
    pub short_term_ref_pic_sets: ShortTermRefPicSets,
    /// `lt_ref_pic_poc_lsb_sps[i]` and `used_by_curr_pic_lt_sps_flag[i]`, if `long_term_ref_pics_present_flag` is `true`.
    ///
    /// See [`LongTermRefPics`] for details.
    pub long_term_ref_pics: Option<LongTermRefPics>,
    /// Equal to `true` specifies that `slice_temporal_mvp_enabled_flag` is present
    /// in the slice headers of non-IDR pictures in the CVS.
    ///
    /// Equal to `false` specifies that `slice_temporal_mvp_enabled_flag` is not present
    /// in slice headers and that temporal motion vector predictors are not used in the CVS.
    pub sps_temporal_mvp_enabled_flag: bool,
    /// Equal to `true` specifies that bi-linear interpolation is conditionally
    /// used in the intra prediction filtering process in the CVS as specified in ISO/IEC 23008-2 - 8.4.4.2.3.
    ///
    /// Equal to `false` specifies that the bi-linear interpolation is not used in the CVS.
    pub strong_intra_smoothing_enabled_flag: bool,
    /// The [`VuiParameters`] structure contained in this SPS, if present.
    pub vui_parameters: Option<VuiParameters>,
    /// The [`SpsRangeExtension`] structure contained in this SPS, if present.
    pub range_extension: Option<SpsRangeExtension>,
    /// The [`SpsMultilayerExtension`] structure contained in this SPS, if present.
    pub multilayer_extension: Option<SpsMultilayerExtension>,
    /// The [`Sps3dExtension`] structure contained in this SPS, if present.
    pub sps_3d_extension: Option<Sps3dExtension>,
    /// The [`SpsSccExtension`] structure contained in this SPS, if present.
    pub scc_extension: Option<SpsSccExtension>,
}

impl SpsRbsp {
    /// Parses an SPS RBSP from the given reader.
    ///
    /// Uses [`EmulationPreventionIo`] to handle emulation prevention bytes.
    ///
    /// Returns an [`SpsRbsp`] struct.
    pub fn parse(reader: impl io::Read, nuh_layer_id: u8) -> io::Result<Self> {
        let mut bit_reader = BitReader::new(EmulationPreventionIo::new(reader));

        let sps_video_parameter_set_id = bit_reader.read_bits(4)? as u8;

        let sps_max_sub_layers_minus1 = bit_reader.read_bits(3)? as u8;
        range_check!(sps_max_sub_layers_minus1, 0, 6)?;

        let sps_temporal_id_nesting_flag = bit_reader.read_bit()?;

        if sps_max_sub_layers_minus1 == 0 && !sps_temporal_id_nesting_flag {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "sps_temporal_id_nesting_flag must be 1 when sps_max_sub_layers_minus1 is 0",
            ));
        }

        let profile_tier_level = ProfileTierLevel::parse(&mut bit_reader, sps_max_sub_layers_minus1)?;

        let sps_seq_parameter_set_id = bit_reader.read_exp_golomb()?;
        range_check!(sps_seq_parameter_set_id, 0, 15)?;

        let chroma_format_idc = bit_reader.read_exp_golomb()?;
        range_check!(chroma_format_idc, 0, 3)?;
        let chroma_format_idc = chroma_format_idc as u8;

        let mut separate_colour_plane_flag = false;
        if chroma_format_idc == 3 {
            separate_colour_plane_flag = bit_reader.read_bit()?;
        }

        // Table 6-1
        let sub_width_c = if chroma_format_idc == 1 || chroma_format_idc == 2 {
            2
        } else {
            1
        };
        let sub_height_c = if chroma_format_idc == 1 { 2 } else { 1 };

        let pic_width_in_luma_samples = NonZero::new(bit_reader.read_exp_golomb()?)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "pic_width_in_luma_samples must not be 0"))?;

        let pic_height_in_luma_samples = NonZero::new(bit_reader.read_exp_golomb()?)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "pic_height_in_luma_samples must not be 0"))?;

        let conformance_window_flag = bit_reader.read_bit()?;

        let conformance_window = conformance_window_flag
            .then(|| ConformanceWindow::parse(&mut bit_reader))
            .transpose()?
            .unwrap_or_default();

        bounds::remaining(pic_width_in_luma_samples.get(), conformance_window.conf_win_left_offset,
            conformance_window.conf_win_right_offset, sub_width_c)?;
        bounds::remaining(pic_height_in_luma_samples.get(), conformance_window.conf_win_top_offset,
            conformance_window.conf_win_bottom_offset, sub_height_c)?;

        let bit_depth_luma_minus8 = bit_reader.read_exp_golomb()?;
        range_check!(bit_depth_luma_minus8, 0, 8)?;
        let bit_depth_luma_minus8 = bit_depth_luma_minus8 as u8;
        let bit_depth_y = 8 + bit_depth_luma_minus8; // BitDepth_Y
        let bit_depth_chroma_minus8 = bit_reader.read_exp_golomb()?;
        range_check!(bit_depth_chroma_minus8, 0, 8)?;
        let bit_depth_chroma_minus8 = bit_depth_chroma_minus8 as u8;
        let bit_depth_c = 8 + bit_depth_chroma_minus8; // BitDepth_C

        let log2_max_pic_order_cnt_lsb_minus4 = bit_reader.read_exp_golomb()?;
        range_check!(log2_max_pic_order_cnt_lsb_minus4, 0, 12)?;
        let log2_max_pic_order_cnt_lsb_minus4 = log2_max_pic_order_cnt_lsb_minus4 as u8;

        let sps_sub_layer_ordering_info_present_flag = bit_reader.read_bit()?;
        let sub_layer_ordering_info = SubLayerOrderingInfo::parse(
            &mut bit_reader,
            sps_sub_layer_ordering_info_present_flag,
            sps_max_sub_layers_minus1,
        )?;

        let log2_min_luma_coding_block_size_minus3 = bit_reader.read_exp_golomb()?;
        range_check!(log2_min_luma_coding_block_size_minus3, 0, 3)?;
        let log2_diff_max_min_luma_coding_block_size = bit_reader.read_exp_golomb()?;
        range_check!(log2_diff_max_min_luma_coding_block_size, 0, 3 - log2_min_luma_coding_block_size_minus3)?;

        let min_cb_log2_size_y = log2_min_luma_coding_block_size_minus3 + 3;
        let ctb_log2_size_y = min_cb_log2_size_y + log2_diff_max_min_luma_coding_block_size;
        range_check!(ctb_log2_size_y, 4, 6)?;

        let log2_min_luma_transform_block_size_minus2 = bit_reader.read_exp_golomb()?;
        range_check!(log2_min_luma_transform_block_size_minus2, 0, min_cb_log2_size_y.min(5) - 2)?;

        let min_tb_log2_size_y = log2_min_luma_transform_block_size_minus2 + 2;

        let log2_diff_max_min_luma_transform_block_size = bit_reader.read_exp_golomb()?;
        range_check!(log2_diff_max_min_luma_transform_block_size, 0, ctb_log2_size_y.min(5) - min_tb_log2_size_y)?;
        let max_transform_hierarchy_depth_inter = bit_reader.read_exp_golomb()?;
        range_check!(max_transform_hierarchy_depth_inter, 0, ctb_log2_size_y - min_tb_log2_size_y)?;
        let max_transform_hierarchy_depth_intra = bit_reader.read_exp_golomb()?;
        range_check!(max_transform_hierarchy_depth_intra, 0, ctb_log2_size_y - min_tb_log2_size_y)?;

        let scaling_list_enabled_flag = bit_reader.read_bit()?;

        let mut scaling_list_data = None;
        if scaling_list_enabled_flag {
            let sps_scaling_list_data_present_flag = bit_reader.read_bit()?;

            if sps_scaling_list_data_present_flag {
                scaling_list_data = Some(ScalingListData::parse(&mut bit_reader)?);
            }
        }

        let amp_enabled_flag = bit_reader.read_bit()?;
        let sample_adaptive_offset_enabled_flag = bit_reader.read_bit()?;

        let mut pcm = None;
        let pcm_enabled_flag = bit_reader.read_bit()?;
        if pcm_enabled_flag {
            pcm = Some(Pcm::parse(
                &mut bit_reader,
                bit_depth_y,
                bit_depth_c,
                min_cb_log2_size_y,
                ctb_log2_size_y,
            )?);
        }

        let num_short_term_ref_pic_sets = bit_reader.read_exp_golomb()?;
        range_check!(num_short_term_ref_pic_sets, 0, 64)?;
        let num_short_term_ref_pic_sets = num_short_term_ref_pic_sets as u8;
        let short_term_ref_pic_sets = ShortTermRefPicSets::parse(
            &mut bit_reader,
            num_short_term_ref_pic_sets as usize,
            nuh_layer_id,
            *sub_layer_ordering_info
                .sps_max_dec_pic_buffering_minus1
                .last()
                .expect("unreachable: cannot be empty"),
        )?;

        let mut long_term_ref_pics = None;
        let long_term_ref_pics_present_flag = bit_reader.read_bit()?;
        if long_term_ref_pics_present_flag {
            long_term_ref_pics = Some(LongTermRefPics::parse(&mut bit_reader, log2_max_pic_order_cnt_lsb_minus4)?);
        }

        let sps_temporal_mvp_enabled_flag = bit_reader.read_bit()?;
        let strong_intra_smoothing_enabled_flag = bit_reader.read_bit()?;

        let mut vui_parameters = None;
        let vui_parameters_present_flag = bit_reader.read_bit()?;
        if vui_parameters_present_flag {
            vui_parameters = Some(VuiParameters::parse(
                &mut bit_reader,
                sps_max_sub_layers_minus1,
                bit_depth_y,
                bit_depth_c,
                chroma_format_idc,
                &profile_tier_level.general_profile,
                &conformance_window,
                sub_width_c,
                pic_width_in_luma_samples,
                sub_height_c,
                pic_height_in_luma_samples,
            )?);
        }

        // Extensions
        let mut range_extension = None;
        let mut multilayer_extension = None;
        let mut sps_3d_extension = None;
        let mut scc_extension = None;

        let sps_extension_flag = bit_reader.read_bit()?;
        if sps_extension_flag {
            let sps_range_extension_flag = bit_reader.read_bit()?;
            let sps_multilayer_extension_flag = bit_reader.read_bit()?;
            let sps_3d_extension_flag = bit_reader.read_bit()?;
            let sps_scc_extension_flag = bit_reader.read_bit()?;
            let sps_extension_4bits = bit_reader.read_bits(4)? as u8;

            if sps_extension_4bits != 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "sps_extension_4bits must be 0"));
            }

            if sps_range_extension_flag {
                range_extension = Some(SpsRangeExtension::parse(&mut bit_reader)?);
            }

            if sps_multilayer_extension_flag {
                multilayer_extension = Some(SpsMultilayerExtension::parse(&mut bit_reader)?);
            }

            if sps_3d_extension_flag {
                sps_3d_extension = Some(Sps3dExtension::parse(&mut bit_reader, min_cb_log2_size_y, ctb_log2_size_y)?);
            }

            if sps_scc_extension_flag {
                scc_extension = Some(SpsSccExtension::parse(
                    &mut bit_reader,
                    chroma_format_idc,
                    bit_depth_y,
                    bit_depth_c,
                )?);
            }

            // No sps_extension_data_flag is present because sps_extension_4bits is 0.
        }

        rbsp_trailing_bits(&mut bit_reader)?;

        Ok(SpsRbsp {
            sps_video_parameter_set_id,
            sps_max_sub_layers_minus1,
            sps_temporal_id_nesting_flag,
            profile_tier_level,
            sps_seq_parameter_set_id,
            chroma_format_idc,
            separate_colour_plane_flag,
            pic_width_in_luma_samples,
            pic_height_in_luma_samples,
            conformance_window,
            bit_depth_luma_minus8,
            bit_depth_chroma_minus8,
            log2_max_pic_order_cnt_lsb_minus4,
            sub_layer_ordering_info,
            log2_min_luma_coding_block_size_minus3,
            log2_diff_max_min_luma_coding_block_size,
            log2_min_luma_transform_block_size_minus2,
            log2_diff_max_min_luma_transform_block_size,
            max_transform_hierarchy_depth_inter,
            max_transform_hierarchy_depth_intra,
            scaling_list_data,
            amp_enabled_flag,
            sample_adaptive_offset_enabled_flag,
            pcm,
            short_term_ref_pic_sets,
            long_term_ref_pics,
            sps_temporal_mvp_enabled_flag,
            strong_intra_smoothing_enabled_flag,
            vui_parameters,
            range_extension,
            multilayer_extension,
            sps_3d_extension,
            scc_extension,
        })
    }

    /// The `croppedWidth` as a [`u64`].
    ///
    /// This is computed from other fields, and doesn't directly appear in the bitstream.
    ///
    /// `croppedWidth = pic_width_in_luma_samples - SubWidthC * (conf_win_right_offset + conf_win_left_offset)` (D-28)
    ///
    /// ISO/IEC 23008-2 - D.3.29
    pub fn cropped_width(&self) -> u64 {
        self.pic_width_in_luma_samples.get()
            - self.sub_width_c() as u64
                * (self.conformance_window.conf_win_left_offset + self.conformance_window.conf_win_right_offset)
    }

    /// The `croppedHeight` as a [`u64`].
    ///
    /// This is computed from other fields, and doesn't directly appear in the bitstream.
    ///
    /// `croppedHeight = pic_height_in_luma_samples - SubHeightC * (conf_win_top_offset + conf_win_bottom_offset)` (D-29)
    ///
    /// ISO/IEC 23008-2 - D.3.29
    pub fn cropped_height(&self) -> u64 {
        self.pic_height_in_luma_samples.get()
            - self.sub_height_c() as u64
                * (self.conformance_window.conf_win_top_offset + self.conformance_window.conf_win_bottom_offset)
    }

    /// - If [`separate_colour_plane_flag`](Self::separate_colour_plane_flag) is equal to `false`, `ChromaArrayType` is set equal to [`chroma_format_idc`](Self::chroma_format_idc).
    /// - Otherwise ([`separate_colour_plane_flag`](Self::separate_colour_plane_flag) is equal to `true`), `ChromaArrayType` is set equal to 0.
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn chroma_array_type(&self) -> u8 {
        if self.separate_colour_plane_flag {
            0
        } else {
            self.chroma_format_idc
        }
    }

    /// ISO/IEC 23008-2 - Table 6-1
    pub fn sub_width_c(&self) -> u8 {
        if self.chroma_format_idc == 1 || self.chroma_format_idc == 2 {
            2
        } else {
            1
        }
    }

    /// ISO/IEC 23008-2 - Table 6-1
    pub fn sub_height_c(&self) -> u8 {
        if self.chroma_format_idc == 1 { 2 } else { 1 }
    }

    /// The bit depth of the samples of the luma array.
    ///
    /// `BitDepth_Y = 8 + bit_depth_luma_minus8` (7-4)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn bit_depth_y(&self) -> u8 {
        8 + self.bit_depth_luma_minus8
    }

    /// The luma quantization parameter range offset.
    ///
    /// `QpBdOffset_Y = 6 * bit_depth_luma_minus8` (7-5)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn qp_bd_offset_y(&self) -> u8 {
        6 * self.bit_depth_y()
    }

    /// The bit depth of the samples of the chroma arrays.
    ///
    /// `BitDepth_C = 8 + bit_depth_chroma_minus8` (7-6)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    #[inline]
    pub fn bit_depth_c(&self) -> u8 {
        8 + self.bit_depth_chroma_minus8
    }

    /// The chroma quantization parameter range offset.
    ///
    /// `QpBdOffset_C = 6 * bit_depth_chroma_minus8` (7-7)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn qp_bd_offset_c(&self) -> u8 {
        6 * self.bit_depth_c()
    }

    /// Used in the decoding process for picture order count.
    ///
    /// `MaxPicOrderCntLsb = 2^(log2_max_pic_order_cnt_lsb_minus4 + 4)` (7-8)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn max_pic_order_cnt_lsb(&self) -> u32 {
        2u32.pow(self.log2_max_pic_order_cnt_lsb_minus4 as u32 + 4)
    }

    /// `MinCbLog2SizeY = log2_min_luma_coding_block_size_minus3 + 3` (7-10)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn min_cb_log2_size_y(&self) -> u64 {
        self.log2_min_luma_coding_block_size_minus3 + 3
    }

    /// `CtbLog2SizeY = MinCbLog2SizeY + log2_diff_max_min_luma_coding_block_size` (7-11)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn ctb_log2_size_y(&self) -> u64 {
        self.min_cb_log2_size_y() + self.log2_diff_max_min_luma_coding_block_size
    }

    /// `MinCbSizeY = 1 << MinCbLog2SizeY` (7-12)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn min_cb_size_y(&self) -> u64 {
        1 << self.min_cb_log2_size_y()
    }

    /// `CtbSizeY = 1 << CtbLog2SizeY` (7-13)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn ctb_size_y(&self) -> NonZero<u64> {
        NonZero::new(1 << self.ctb_log2_size_y()).unwrap()
    }

    /// `PicWidthInMinCbsY = pic_width_in_luma_samples / MinCbSizeY` (7-14)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn pic_width_in_min_cbs_y(&self) -> u64 {
        self.pic_width_in_luma_samples.get() / self.min_cb_size_y()
    }

    /// `PicWidthInCtbsY = Ceil(pic_width_in_luma_samples ÷ CtbSizeY)` (7-15)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn pic_width_in_ctbs_y(&self) -> u64 {
        (self.pic_width_in_luma_samples.get() / self.ctb_size_y()) + 1
    }

    /// `PicHeightInMinCbsY = pic_height_in_luma_samples / MinCbSizeY` (7-16)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn pic_height_in_min_cbs_y(&self) -> u64 {
        self.pic_height_in_luma_samples.get() / self.min_cb_size_y()
    }

    /// `PicHeightInCtbsY = Ceil(pic_height_in_luma_samples ÷ CtbSizeY)` (7-17)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn pic_height_in_ctbs_y(&self) -> u64 {
        (self.pic_height_in_luma_samples.get() / self.ctb_size_y()) + 1
    }

    /// `PicSizeInMinCbsY = PicWidthInMinCbsY * PicHeightInMinCbsY` (7-18)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn pic_size_in_min_cbs_y(&self) -> u64 {
        self.pic_width_in_min_cbs_y() * self.pic_height_in_min_cbs_y()
    }

    /// `PicSizeInCtbsY = PicWidthInCtbsY * PicHeightInCtbsY` (7-19)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn pic_size_in_ctbs_y(&self) -> u64 {
        self.pic_width_in_ctbs_y() * self.pic_height_in_ctbs_y()
    }

    /// `PicSizeInSamplesY = pic_width_in_luma_samples * pic_height_in_luma_samples` (7-20)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn pic_size_in_samples_y(&self) -> u64 {
        self.pic_width_in_luma_samples.get() * self.pic_height_in_luma_samples.get()
    }

    /// `PicWidthInSamplesC = pic_width_in_luma_samples / SubWidthC` (7-21)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn pic_width_in_samples_c(&self) -> u64 {
        self.pic_width_in_luma_samples.get() / self.sub_width_c() as u64
    }

    /// `PicHeightInSamplesC = pic_height_in_luma_samples / SubHeightC` (7-22)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn pic_height_in_samples_c(&self) -> u64 {
        self.pic_height_in_luma_samples.get() / self.sub_height_c() as u64
    }

    /// - If `chroma_format_idc` is equal to 0 (monochrome) or [`separate_colour_plane_flag`](Self::separate_colour_plane_flag) is equal to `true`,
    ///   `CtbWidthC` is equal to 0.
    /// - Otherwise, `CtbWidthC` is derived as follows: `CtbWidthC = CtbSizeY / SubWidthC` (7-23)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn ctb_width_c(&self) -> u64 {
        if self.chroma_format_idc == 0 || self.separate_colour_plane_flag {
            0
        } else {
            self.ctb_size_y().get() / self.sub_width_c() as u64
        }
    }

    /// - If `chroma_format_idc` is equal to 0 (monochrome) or [`separate_colour_plane_flag`](Self::separate_colour_plane_flag) is equal to `true`,
    ///   `CtbHeightC` is equal to 0.
    /// - Otherwise, `CtbHeightC` is derived as follows: `CtbHeightC = CtbSizeY / SubHeightC` (7-24)
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn ctb_height_c(&self) -> u64 {
        if self.chroma_format_idc == 0 || self.separate_colour_plane_flag {
            0
        } else {
            self.ctb_size_y().get() / self.sub_height_c() as u64
        }
    }

    /// `MinTbLog2SizeY` is set equal to [`log2_min_luma_transform_block_size_minus2 + 2`](Self::log2_min_luma_transform_block_size_minus2).
    ///
    /// The CVS shall not contain data that result in `MinTbLog2SizeY`
    /// greater than or equal to [`MinCbLog2SizeY`](Self::min_cb_log2_size_y).
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn min_tb_log2_size_y(&self) -> u64 {
        self.log2_min_luma_transform_block_size_minus2 + 2
    }

    /// `MaxTbLog2SizeY = log2_min_luma_transform_block_size_minus2 + 2 + log2_diff_max_min_luma_transform_block_size`
    ///
    /// The CVS shall not contain data that result in `MaxTbLog2SizeY` greater than [`Min(CtbLog2SizeY, 5)`](Self::ctb_log2_size_y).
    ///
    /// ISO/IEC 23008-2 - 7.4.3.2.1
    pub fn max_tb_log2_size_y(&self) -> u64 {
        self.log2_min_luma_transform_block_size_minus2 + 2 + self.log2_diff_max_min_luma_transform_block_size
    }

    /// `RawCtuBits = CtbSizeY * CtbSizeY * BitDepthY + 2 * (CtbWidthC * CtbHeightC) * BitDepthC` (A-1)
    ///
    /// ISO/IEC 23008-2 - A.3.1
    pub fn raw_ctu_bits(&self) -> u64 {
        let ctb_size_y = self.ctb_size_y().get();
        ctb_size_y * ctb_size_y * self.bit_depth_y() as u64
            + 2 * (self.ctb_width_c() * self.ctb_height_c()) * self.bit_depth_c() as u64
    }
}

#[cfg(test)]
#[cfg_attr(all(test, coverage_nightly), coverage(off))]
mod tests;
