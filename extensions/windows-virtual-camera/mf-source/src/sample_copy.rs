//! Final prepared-frame copy boundary — REQ-PICOO-VCAM-010/012.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleCopyError {
    DestinationTooSmall { required: usize, available: usize },
}

/// Copy one already-prepared frame into its final Media Foundation buffer.
///
/// This is intentionally the only pixel traversal on the RequestSample path.
#[doc(hidden)]
pub fn copy_prepared_frame(
    source: &[u8],
    destination: &mut [u8],
) -> Result<usize, SampleCopyError> {
    if destination.len() < source.len() {
        return Err(SampleCopyError::DestinationTooSmall {
            required: source.len(),
            available: destination.len(),
        });
    }
    destination[..source.len()].copy_from_slice(source);
    Ok(source.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_exactly_the_frame_and_preserves_allocator_tail() {
        let mut destination = [0xaa; 8];
        assert_eq!(copy_prepared_frame(&[1, 2, 3, 4], &mut destination), Ok(4));
        assert_eq!(destination, [1, 2, 3, 4, 0xaa, 0xaa, 0xaa, 0xaa]);
    }

    #[test]
    fn rejects_short_allocator_buffer_without_partial_copy() {
        let mut destination = [9; 3];
        assert_eq!(
            copy_prepared_frame(&[1, 2, 3, 4], &mut destination),
            Err(SampleCopyError::DestinationTooSmall {
                required: 4,
                available: 3,
            })
        );
        assert_eq!(destination, [9; 3]);
    }
}
