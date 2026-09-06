//! CoreMedia owns native format creation; bitstream owns source facts and records.
use super::{check_status, Codec, CodecConfiguration, DecodeError};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMFormatDescription, CMVideoFormatDescriptionCreateFromH264ParameterSets,
    CMVideoFormatDescriptionCreateFromHEVCParameterSets,
};
use picoo_bitstream::{AccessUnit, VideoSpsFacts};
use picoo_protocol::control::StreamConfig;
use std::ptr::{self, NonNull};

pub(super) fn configuration(
    config: Option<&StreamConfig>,
    picture: &AccessUnit<'_>,
) -> Result<CodecConfiguration, DecodeError> {
    if let Some(config) = config {
        return crate::configured_picture::configuration(config);
    }
    // Explicit AVC diagnostic submissions can carry their own parameter sets.
    // Product submissions always carry a committed configuration in the token.
    if picture.codec() != Codec::Avc {
        return Err(DecodeError::NotInitialized);
    }
    let find = |kind| {
        picture
            .nals()
            .iter()
            .find(|nal| nal[0] & 0x1f == kind)
            .copied()
            .ok_or(DecodeError::NotInitialized)
    };
    CodecConfiguration::from_avc_parameter_sets(find(7)?, find(8)?)
        .map_err(|error| DecodeError::Platform(error.to_string()))
}

pub(super) fn source_facts(
    configuration: &CodecConfiguration,
) -> Result<VideoSpsFacts, DecodeError> {
    let parse = match configuration.codec() {
        Codec::Avc => VideoSpsFacts::parse_avc,
        Codec::Hevc => VideoSpsFacts::parse_hevc,
    };
    let mut facts = None;
    for sps in configuration.sps() {
        let current = parse(sps).map_err(|error| DecodeError::Platform(error.to_string()))?;
        if facts.is_some_and(|previous| previous != current) {
            return Err(DecodeError::ConfigurationMismatch);
        }
        facts = Some(current);
    }
    facts.ok_or(DecodeError::NotInitialized)
}

pub(super) fn create(
    configuration: &CodecConfiguration,
) -> Result<CFRetained<CMFormatDescription>, DecodeError> {
    let sets = configuration
        .vps()
        .iter()
        .chain(configuration.sps())
        .chain(configuration.pps());
    let mut pointers = Vec::new();
    let mut sizes = Vec::new();
    for set in sets {
        pointers.push(NonNull::new(set.as_ptr().cast_mut()).ok_or(DecodeError::NotInitialized)?);
        sizes.push(set.len());
    }
    if pointers.is_empty() {
        return Err(DecodeError::NotInitialized);
    }
    let mut raw: *const CMFormatDescription = ptr::null();
    // SAFETY: Validated bounded raw parameter sets and pointer arrays remain alive
    // for the synchronous call; CoreMedia owns the returned format's copied data.
    let status = unsafe {
        let pointers = NonNull::new(pointers.as_mut_ptr()).unwrap();
        let sizes = NonNull::new(sizes.as_mut_ptr()).unwrap();
        match configuration.codec() {
            Codec::Avc => CMVideoFormatDescriptionCreateFromH264ParameterSets(
                None,
                configuration.sps().len() + configuration.pps().len(),
                pointers,
                sizes,
                4,
                NonNull::from(&mut raw),
            ),
            Codec::Hevc => CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                None,
                configuration.vps().len() + configuration.sps().len() + configuration.pps().len(),
                pointers,
                sizes,
                4,
                None,
                NonNull::from(&mut raw),
            ),
        }
    };
    check_status("CoreMedia parameter-set format creation", status)?;
    let raw = NonNull::new(raw.cast_mut())
        .ok_or_else(|| DecodeError::Platform("CoreMedia returned a null format".into()))?;
    // SAFETY: Successful Create returns ownership of a +1 CoreFoundation object.
    Ok(unsafe { CFRetained::from_raw(raw) })
}
