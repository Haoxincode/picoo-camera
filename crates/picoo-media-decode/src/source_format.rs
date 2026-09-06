//! Shared admitted native AVC presentation; platforms verify their actual output.
use crate::DecodeError;
use picoo_bitstream::AvcSpsFacts;

pub(crate) fn validate_source(facts: &AvcSpsFacts) -> Result<(), DecodeError> {
    if facts.pixel_aspect_ratio.is_some_and(|(w, h)| w != h)
        || facts.chroma_location > 1
        || facts.color.is_some_and(|color| {
            color.full_range
                || !matches!(color.primaries, 1 | 2)
                || !matches!(color.transfer, 1 | 2)
                || !matches!(color.matrix, 1 | 2)
        })
    {
        return Err(DecodeError::Platform(
            "unsupported native AVC presentation or color".into(),
        ));
    }
    Ok(())
}
