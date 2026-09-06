use std::io;

use crate::SpsNALUnit;

// To compare the results to an independent source, you can use: https://github.com/chemag/h265nal

#[test]
fn test_sps_parse() {
    let data = b"B\x01\x01\x01@\0\0\x03\0\x90\0\0\x03\0\0\x03\0\x99\xa0\x01@ \x05\xa1e\x95R\x90\x84d_\xf8\xc0Z\x80\x80\x80\x82\0\0\x03\0\x02\0\0\x03\x01 \xc0\x0b\xbc\xa2\0\x02bX\0\x011-\x08";

    let nalu = SpsNALUnit::parse(io::Cursor::new(data)).unwrap();
    let sps = &nalu.rbsp;

    assert_eq!(sps.cropped_width(), 2560);
    assert_eq!(sps.cropped_height(), 1440);
    assert_eq!(sps.chroma_array_type(), 1);
    assert_eq!(sps.sub_width_c(), 2);
    assert_eq!(sps.sub_height_c(), 2);
    assert_eq!(sps.bit_depth_y(), 8);
    assert_eq!(sps.qp_bd_offset_y(), 48);
    assert_eq!(sps.bit_depth_c(), 8);
    assert_eq!(sps.qp_bd_offset_c(), 48);
    assert_eq!(sps.max_pic_order_cnt_lsb(), 256);
    assert_eq!(sps.min_cb_log2_size_y(), 4);
    assert_eq!(sps.ctb_log2_size_y(), 5);
    assert_eq!(sps.min_cb_size_y(), 16);
    assert_eq!(sps.ctb_size_y().get(), 32);
    assert_eq!(sps.pic_width_in_min_cbs_y(), 160);
    assert_eq!(sps.pic_width_in_ctbs_y(), 81);
    assert_eq!(sps.pic_height_in_min_cbs_y(), 90);
    assert_eq!(sps.pic_height_in_ctbs_y(), 46);
    assert_eq!(sps.pic_size_in_min_cbs_y(), 14400);
    assert_eq!(sps.pic_size_in_ctbs_y(), 3726);
    assert_eq!(sps.pic_size_in_samples_y(), 3686400);
    assert_eq!(sps.pic_width_in_samples_c(), 1280);
    assert_eq!(sps.pic_height_in_samples_c(), 720);
    assert_eq!(sps.ctb_width_c(), 16);
    assert_eq!(sps.ctb_height_c(), 16);
    assert_eq!(sps.min_tb_log2_size_y(), 2);
    assert_eq!(sps.max_tb_log2_size_y(), 5);
    assert_eq!(sps.raw_ctu_bits(), 12288);
    insta::assert_debug_snapshot!(nalu);
}

#[test]
fn test_sps_parse2() {
    // This is a real SPS from an mp4 video file recorded with OBS.
    let data = b"\x42\x01\x01\x01\x40\x00\x00\x03\x00\x90\x00\x00\x03\x00\x00\x03\x00\x78\xa0\x03\xc0\x80\x11\x07\xcb\x96\xb4\xa4\x25\x92\xe3\x01\x6a\x02\x02\x02\x08\x00\x00\x03\x00\x08\x00\x00\x03\x00\xf3\x00\x2e\xf2\x88\x00\x02\x62\x5a\x00\x00\x13\x12\xd0\x20";

    let nalu = SpsNALUnit::parse(io::Cursor::new(data)).unwrap();
    let sps = &nalu.rbsp;

    assert_eq!(sps.cropped_width(), 1920);
    assert_eq!(sps.cropped_height(), 1080);
    assert_eq!(sps.chroma_array_type(), 1);
    assert_eq!(sps.sub_width_c(), 2);
    assert_eq!(sps.sub_height_c(), 2);
    assert_eq!(sps.bit_depth_y(), 8);
    assert_eq!(sps.qp_bd_offset_y(), 48);
    assert_eq!(sps.bit_depth_c(), 8);
    assert_eq!(sps.qp_bd_offset_c(), 48);
    assert_eq!(sps.max_pic_order_cnt_lsb(), 256);
    assert_eq!(sps.min_cb_log2_size_y(), 4);
    assert_eq!(sps.ctb_log2_size_y(), 5);
    assert_eq!(sps.min_cb_size_y(), 16);
    assert_eq!(sps.ctb_size_y().get(), 32);
    assert_eq!(sps.pic_width_in_min_cbs_y(), 120);
    assert_eq!(sps.pic_width_in_ctbs_y(), 61);
    assert_eq!(sps.pic_height_in_min_cbs_y(), 68);
    assert_eq!(sps.pic_height_in_ctbs_y(), 35);
    assert_eq!(sps.pic_size_in_min_cbs_y(), 8160);
    assert_eq!(sps.pic_size_in_ctbs_y(), 2135);
    assert_eq!(sps.pic_size_in_samples_y(), 2088960);
    assert_eq!(sps.pic_width_in_samples_c(), 960);
    assert_eq!(sps.pic_height_in_samples_c(), 544);
    assert_eq!(sps.ctb_width_c(), 16);
    assert_eq!(sps.ctb_height_c(), 16);
    assert_eq!(sps.min_tb_log2_size_y(), 2);
    assert_eq!(sps.max_tb_log2_size_y(), 5);
    assert_eq!(sps.raw_ctu_bits(), 12288);
    insta::assert_debug_snapshot!(nalu);
}

#[test]
fn test_sps_parse3() {
    // This is a real SPS from here: https://kodi.wiki/view/Samples
    let data = b"\x42\x01\x01\x22\x20\x00\x00\x03\x00\x90\x00\x00\x03\x00\x00\x03\x00\x99\xA0\x01\xE0\x20\x02\x1C\x4D\x8D\x35\x92\x4F\x84\x14\x70\xF1\xC0\x90\x3B\x0E\x18\x36\x1A\x08\x42\xF0\x81\x21\x00\x88\x40\x10\x06\xE1\xA3\x06\xC3\x41\x08\x5C\xA0\xA0\x21\x04\x41\x70\xB0\x2A\x0A\xC2\x80\x35\x40\x70\x80\xE0\x07\xD0\x2B\x41\x80\xA8\x20\x0B\x85\x81\x50\x56\x14\x01\xAA\x03\x84\x07\x00\x3E\x81\x58\xA1\x0D\x35\xE9\xE8\x60\xD7\x43\x03\x41\xB1\xB8\xC0\xD0\x70\x3A\x1B\x1B\x18\x1A\x0E\x43\x21\x30\xC8\x60\x24\x18\x10\x1F\x1F\x1C\x1E\x30\x74\x26\x12\x0E\x0C\x04\x30\x40\x38\x10\x82\x00\x94\x0F\xF0\x86\x9A\xF2\x17\x20\x48\x26\x59\x02\x41\x20\x98\x4F\x09\x04\x83\x81\xD0\x98\x4E\x12\x09\x07\x21\x90\x98\x5C\x2C\x12\x0C\x08\x0F\x8F\x8E\x0F\x18\x3A\x13\x09\x07\x06\x02\x18\x20\x1C\x08\x41\x00\x4A\x07\xF2\x86\x89\x4D\x08\x2C\x83\x8E\x52\x18\x17\x02\xF2\xC8\x0B\x80\xDC\x06\xB0\x5F\x82\xE0\x35\x03\xA0\x66\x06\xB0\x63\x06\x00\x6A\x06\x40\xE0\x0B\x20\x73\x06\x60\xC8\x0E\x40\x58\x03\x90\x0A\xB0\x77\x07\x40\x2A\x81\xC7\xFF\xC1\x24\x34\x49\x8E\x61\x82\x62\x0C\x72\x90\xC0\xB8\x17\x96\x40\x5C\x06\xE0\x35\x82\xFC\x17\x01\xA8\x1D\x03\x30\x35\x83\x18\x30\x03\x50\x32\x07\x00\x59\x03\x98\x33\x06\x40\x72\x02\xC0\x1C\x80\x55\x83\xB8\x3A\x01\x54\x0E\x3F\xFE\x09\x0A\x10\xE9\xAF\x4F\x43\x06\xBA\x18\x1A\x0D\x8D\xC6\x06\x83\x81\xD0\xD8\xD8\xC0\xD0\x72\x19\x09\x86\x43\x01\x20\xC0\x80\xF8\xF8\xE0\xF1\x83\xA1\x30\x90\x70\x60\x21\x82\x01\xC0\x84\x10\x04\xA0\x7F\x84\x3A\x6B\xC8\x5C\x81\x20\x99\x64\x09\x04\x82\x61\x3C\x24\x12\x0E\x07\x42\x61\x38\x48\x24\x1C\x86\x42\x61\x70\xB0\x48\x30\x20\x3E\x3E\x38\x3C\x60\xE8\x4C\x24\x1C\x18\x08\x60\x80\x70\x21\x04\x01\x28\x1F\xCA\x1A\x92\x9A\x10\x59\x07\x1C\xA4\x30\x2E\x05\xE5\x90\x17\x01\xB8\x0D\x60\xBF\x05\xC0\x6A\x07\x40\xCC\x0D\x60\xC6\x0C\x00\xD4\x0C\x81\xC0\x16\x40\xE6\x0C\xC1\x90\x1C\x80\xB0\x07\x20\x15\x60\xEE\x0E\x80\x55\x03\x8F\xFF\x82\x48\x6A\x49\x8E\x61\x82\x62\x0C\x72\x90\xC0\xB8\x17\x96\x40\x5C\x06\xE0\x35\x82\xFC\x17\x01\xA8\x1D\x03\x30\x35\x83\x18\x30\x03\x50\x32\x07\x00\x59\x03\x98\x33\x06\x40\x72\x02\xC0\x1C\x80\x55\x83\xB8\x3A\x01\x54\x0E\x3F\xFE\x09\x0A\x10\xE9\xAF\x4F\x43\x06\xBA\x18\x1A\x0D\x8D\xC6\x06\x83\x81\xD0\xD8\xD8\xC0\xD0\x72\x19\x09\x86\x43\x01\x20\xC0\x80\xF8\xF8\xE0\xF1\x83\xA1\x30\x90\x70\x60\x21\x82\x01\xC0\x84\x10\x04\xA0\x7F\x86\xA4\x98\xE6\x18\x26\x20\xC7\x29\x0C\x0B\x81\x79\x64\x05\xC0\x6E\x03\x58\x2F\xC1\x70\x1A\x81\xD0\x33\x03\x58\x31\x83\x00\x35\x03\x20\x70\x05\x90\x39\x83\x30\x64\x07\x20\x2C\x01\xC8\x05\x58\x3B\x83\xA0\x15\x40\xE3\xFF\xE0\x91\x11\x5C\x96\xA5\xDE\x02\xD4\x24\x40\x26\xD9\x40\x00\x07\xD2\x00\x01\xD4\xC0\x3E\x46\x81\x8D\xC0\x00\x26\x25\xA0\x00\x13\x12\xD0\x00\x04\xC4\xB4\x00\x02\x62\x5A\x8B\x84\x02\x08\xA2\x00\x01\x00\x08\x44\x01\xC1\x72\x43\x8D\x62\x24\x00\x00\x00\x14";

    let nalu = SpsNALUnit::parse(io::Cursor::new(data)).unwrap();
    let sps = &nalu.rbsp;

    assert_eq!(sps.cropped_width(), 3840);
    assert_eq!(sps.cropped_height(), 2160);
    assert_eq!(sps.chroma_array_type(), 1);
    assert_eq!(sps.sub_width_c(), 2);
    assert_eq!(sps.sub_height_c(), 2);
    assert_eq!(sps.bit_depth_y(), 10);
    assert_eq!(sps.qp_bd_offset_y(), 60);
    assert_eq!(sps.bit_depth_c(), 10);
    assert_eq!(sps.qp_bd_offset_c(), 60);
    assert_eq!(sps.max_pic_order_cnt_lsb(), 65536);
    assert_eq!(sps.min_cb_log2_size_y(), 3);
    assert_eq!(sps.ctb_log2_size_y(), 6);
    assert_eq!(sps.min_cb_size_y(), 8);
    assert_eq!(sps.ctb_size_y().get(), 64);
    assert_eq!(sps.pic_width_in_min_cbs_y(), 480);
    assert_eq!(sps.pic_width_in_ctbs_y(), 61);
    assert_eq!(sps.pic_height_in_min_cbs_y(), 270);
    assert_eq!(sps.pic_height_in_ctbs_y(), 34);
    assert_eq!(sps.pic_size_in_min_cbs_y(), 129600);
    assert_eq!(sps.pic_size_in_ctbs_y(), 2074);
    assert_eq!(sps.pic_size_in_samples_y(), 8294400);
    assert_eq!(sps.pic_width_in_samples_c(), 1920);
    assert_eq!(sps.pic_height_in_samples_c(), 1080);
    assert_eq!(sps.ctb_width_c(), 32);
    assert_eq!(sps.ctb_height_c(), 32);
    assert_eq!(sps.min_tb_log2_size_y(), 2);
    assert_eq!(sps.max_tb_log2_size_y(), 5);
    assert_eq!(sps.raw_ctu_bits(), 61440);
    insta::assert_debug_snapshot!(nalu);
}

#[test]
fn test_sps_parse4() {
    // This is a real SPS from here: https://lf-tk-sg.ibytedtos.com/obj/tcs-client-sg/resources/video_demo_hevc.html#main-bt709-sample-5
    let data = b"\x42\x01\x01\x01\x60\x00\x00\x03\x00\x90\x00\x00\x03\x00\x00\x03\x00\xB4\xA0\x00\xF0\x08\x00\x43\x85\x96\x56\x69\x24\xC2\xB0\x16\x80\x80\x00\x00\x03\x00\x80\x00\x00\x05\x04\x22\x00\x01";

    let nalu = SpsNALUnit::parse(io::Cursor::new(data)).unwrap();
    let sps = &nalu.rbsp;

    assert_eq!(sps.cropped_width(), 7680);
    assert_eq!(sps.cropped_height(), 4320);
    assert_eq!(sps.chroma_array_type(), 1);
    assert_eq!(sps.sub_width_c(), 2);
    assert_eq!(sps.sub_height_c(), 2);
    assert_eq!(sps.bit_depth_y(), 8);
    assert_eq!(sps.qp_bd_offset_y(), 48);
    assert_eq!(sps.bit_depth_c(), 8);
    assert_eq!(sps.qp_bd_offset_c(), 48);
    assert_eq!(sps.max_pic_order_cnt_lsb(), 256);
    assert_eq!(sps.min_cb_log2_size_y(), 3);
    assert_eq!(sps.ctb_log2_size_y(), 6);
    assert_eq!(sps.min_cb_size_y(), 8);
    assert_eq!(sps.ctb_size_y().get(), 64);
    assert_eq!(sps.pic_width_in_min_cbs_y(), 960);
    assert_eq!(sps.pic_width_in_ctbs_y(), 121);
    assert_eq!(sps.pic_height_in_min_cbs_y(), 540);
    assert_eq!(sps.pic_height_in_ctbs_y(), 68);
    assert_eq!(sps.pic_size_in_min_cbs_y(), 518400);
    assert_eq!(sps.pic_size_in_ctbs_y(), 8228);
    assert_eq!(sps.pic_size_in_samples_y(), 33177600);
    assert_eq!(sps.pic_width_in_samples_c(), 3840);
    assert_eq!(sps.pic_height_in_samples_c(), 2160);
    assert_eq!(sps.ctb_width_c(), 32);
    assert_eq!(sps.ctb_height_c(), 32);
    assert_eq!(sps.min_tb_log2_size_y(), 2);
    assert_eq!(sps.max_tb_log2_size_y(), 5);
    assert_eq!(sps.raw_ctu_bits(), 49152);
    insta::assert_debug_snapshot!(nalu);
}

#[test]
fn test_sps_parse5() {
    // This is a real SPS from here: https://lf-tk-sg.ibytedtos.com/obj/tcs-client-sg/resources/video_demo_hevc.html#msp-bt709-sample-1
    let data = b"\x42\x01\x01\x03\x70\x00\x00\x03\x00\x00\x03\x00\x00\x03\x00\x00\x03\x00\x78\xA0\x03\xC0\x80\x10\xE7\xF9\x7E\x49\x1B\x65\xB2\x22\x00\x01\x00\x07\x44\x01\xC1\x90\x95\x81\x12\x00\x00\x00\x14";

    let nalu = SpsNALUnit::parse(io::Cursor::new(data)).unwrap();
    let sps = &nalu.rbsp;

    assert_eq!(sps.cropped_width(), 1920);
    assert_eq!(sps.cropped_height(), 1080);
    assert_eq!(sps.chroma_array_type(), 1);
    assert_eq!(sps.sub_width_c(), 2);
    assert_eq!(sps.sub_height_c(), 2);
    assert_eq!(sps.bit_depth_y(), 8);
    assert_eq!(sps.qp_bd_offset_y(), 48);
    assert_eq!(sps.bit_depth_c(), 8);
    assert_eq!(sps.qp_bd_offset_c(), 48);
    assert_eq!(sps.max_pic_order_cnt_lsb(), 256);
    assert_eq!(sps.min_cb_log2_size_y(), 3);
    assert_eq!(sps.ctb_log2_size_y(), 6);
    assert_eq!(sps.min_cb_size_y(), 8);
    assert_eq!(sps.ctb_size_y().get(), 64);
    assert_eq!(sps.pic_width_in_min_cbs_y(), 240);
    assert_eq!(sps.pic_width_in_ctbs_y(), 31);
    assert_eq!(sps.pic_height_in_min_cbs_y(), 135);
    assert_eq!(sps.pic_height_in_ctbs_y(), 17);
    assert_eq!(sps.pic_size_in_min_cbs_y(), 32400);
    assert_eq!(sps.pic_size_in_ctbs_y(), 527);
    assert_eq!(sps.pic_size_in_samples_y(), 2073600);
    assert_eq!(sps.pic_width_in_samples_c(), 960);
    assert_eq!(sps.pic_height_in_samples_c(), 540);
    assert_eq!(sps.ctb_width_c(), 32);
    assert_eq!(sps.ctb_height_c(), 32);
    assert_eq!(sps.min_tb_log2_size_y(), 2);
    assert_eq!(sps.max_tb_log2_size_y(), 5);
    assert_eq!(sps.raw_ctu_bits(), 49152);
    insta::assert_debug_snapshot!(nalu);
}

#[test]
fn test_sps_parse6() {
    // This is a real SPS from here: https://lf-tk-sg.ibytedtos.com/obj/tcs-client-sg/resources/video_demo_hevc.html#rext-bt709-sample-1
    let data = b"\x42\x01\x01\x24\x08\x00\x00\x03\x00\x9D\x08\x00\x00\x03\x00\x00\x99\xB0\x01\xE0\x20\x02\x1C\x4D\x94\xD6\xED\xBE\x41\x12\x64\xEB\x25\x11\x44\x1A\x6C\x9D\x64\xA2\x29\x09\x26\xBA\xF5\xFF\xEB\xFA\xFD\x7F\xEB\xF5\x44\x51\x04\x93\x5D\x7A\xFF\xF5\xFD\x7E\xBF\xF5\xFA\xC8\xA4\x92\x4D\x75\xEB\xFF\xD7\xF5\xFA\xFF\xD7\xEA\x88\xA2\x24\x93\x5D\x7A\xFF\xF5\xFD\x7E\xBF\xF5\xFA\xC8\x94\x08\x53\x49\x29\x24\x89\x55\x12\xA5\x2A\x94\xC1\x35\x01\x01\x01\x03\xB8\x40\x20\x80\xA2\x00\x01\x00\x07\x44\x01\xC0\x72\xB0\x3C\x90\x00\x00\x00\x13\x63\x6F\x6C\x72\x6E\x63\x6C\x78\x00\x01\x00\x01\x00\x01\x00\x00\x00\x00\x18";

    let nalu = SpsNALUnit::parse(io::Cursor::new(data)).unwrap();
    let sps = &nalu.rbsp;

    assert_eq!(sps.cropped_width(), 3840);
    assert_eq!(sps.cropped_height(), 2160);
    assert_eq!(sps.chroma_array_type(), 2);
    assert_eq!(sps.sub_width_c(), 2);
    assert_eq!(sps.sub_height_c(), 1);
    assert_eq!(sps.bit_depth_y(), 10);
    assert_eq!(sps.qp_bd_offset_y(), 60);
    assert_eq!(sps.bit_depth_c(), 10);
    assert_eq!(sps.qp_bd_offset_c(), 60);
    assert_eq!(sps.max_pic_order_cnt_lsb(), 256);
    assert_eq!(sps.min_cb_log2_size_y(), 3);
    assert_eq!(sps.ctb_log2_size_y(), 5);
    assert_eq!(sps.min_cb_size_y(), 8);
    assert_eq!(sps.ctb_size_y().get(), 32);
    assert_eq!(sps.pic_width_in_min_cbs_y(), 480);
    assert_eq!(sps.pic_width_in_ctbs_y(), 121);
    assert_eq!(sps.pic_height_in_min_cbs_y(), 270);
    assert_eq!(sps.pic_height_in_ctbs_y(), 68);
    assert_eq!(sps.pic_size_in_min_cbs_y(), 129600);
    assert_eq!(sps.pic_size_in_ctbs_y(), 8228);
    assert_eq!(sps.pic_size_in_samples_y(), 8294400);
    assert_eq!(sps.pic_width_in_samples_c(), 1920);
    assert_eq!(sps.pic_height_in_samples_c(), 2160);
    assert_eq!(sps.ctb_width_c(), 16);
    assert_eq!(sps.ctb_height_c(), 32);
    assert_eq!(sps.min_tb_log2_size_y(), 2);
    assert_eq!(sps.max_tb_log2_size_y(), 4);
    assert_eq!(sps.raw_ctu_bits(), 20480);
    insta::assert_debug_snapshot!(nalu);
}

#[test]
fn test_sps_parse_inter_ref_prediction() {
    // I generated this sample using the reference encoder https://vcgit.hhi.fraunhofer.de/jvet/HM
    let data = b"\x42\x01\x01\x01\x60\x00\x00\x03\x00\x00\x03\x00\x00\x03\x00\x00\x03\x00\x00\xA0\x0B\x08\x04\x85\x96\x5E\x49\x1B\x60\xD9\x78\x88\x88\x8F\xE7\x9F\xCF\xE7\xF3\xF9\xFC\xF2\xFF\xFF\xFF\xCF\xE7\xF3\xF9\xFC\xFE\x7F\x3F\x3F\x9F\xCF\xE7\xF3\xF9\xDB\x20";

    let nalu = SpsNALUnit::parse(io::Cursor::new(data)).unwrap();
    insta::assert_debug_snapshot!(nalu);
}

#[test]
fn test_forbidden_zero_bit() {
    // 0x80 = 1000 0000: forbidden_zero_bit (first bit) is 1.
    let data = [0x80];
    let err = SpsNALUnit::parse(io::Cursor::new(data)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(err.to_string(), "forbidden_zero_bit is not zero");
}

#[test]
fn test_invalid_nalu_type() {
    // 1 forbidden_zero_bit = 0
    // nal_unit_type (100000) = 32 ≠ 33
    // nuh_layer_id (000000) = 0
    // nuh_temporal_id_plus1 (001) = 1
    #[allow(clippy::unusual_byte_groupings)]
    let data = [0b0_100000_0, 0b00000_001];
    let err = SpsNALUnit::parse(io::Cursor::new(data)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(err.to_string(), "nal_unit_type is not SPS_NUT");
}
