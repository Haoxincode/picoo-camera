use picoo_bitstream::{AccessUnit, Codec, CodecConfiguration, NalFormat};

pub(crate) fn fixtures() -> Vec<(CodecConfiguration, Vec<u8>)> {
    let avc = include_bytes!("../../picoo-testkit/fixtures/avc-1280x720-bt709-idr.h264");
    let picture = AccessUnit::parse(Codec::Avc, NalFormat::AnnexB, avc).unwrap();
    let sps = picture.nals().iter().find(|nal| nal[0] & 31 == 7).unwrap();
    let pps = picture.nals().iter().find(|nal| nal[0] & 31 == 8).unwrap();
    let avc_config = CodecConfiguration::from_avc_parameter_sets(sps, pps).unwrap();
    let avc = picture.to_length_prefixed().unwrap();
    let hevc_config = CodecConfiguration::parse(
        Codec::Hevc,
        include_bytes!("../../picoo-testkit/fixtures/hevc-1280x720-bt709-config.bin")
            .as_slice()
            .to_vec()
            .into(),
    )
    .unwrap();
    let hevc = include_bytes!("../../picoo-testkit/fixtures/hevc-1280x720-bt709-idr.bin").to_vec();
    vec![(avc_config, avc), (hevc_config, hevc)]
}
