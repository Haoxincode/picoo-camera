use super::*;

fn facts() -> AvcSpsFacts {
    AvcSpsFacts {
        coded_width: 192,
        coded_height: 96,
        visible_x: 0,
        visible_y: 0,
        visible_width: 64,
        visible_height: 64,
        pixel_aspect_ratio: Some((1, 1)),
        color: None,
        chroma_location: 0,
    }
}
unsafe fn media() -> IMFMediaType {
    let media = MFCreateMediaType().unwrap();
    media.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).unwrap();
    media
        .SetUINT64(&MF_MT_FRAME_SIZE, (192_u64 << 32) | 96)
        .unwrap();
    media
        .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1_u64 << 32) | 1)
        .unwrap();
    for (key, value) in [
        (MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0),
        (MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0),
        (MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0),
        (MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0),
        (MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0),
    ] {
        media.SetUINT32(&key, value as u32).unwrap();
    }
    media
}
unsafe fn aperture(media: &IMFMediaType, fraction: u16) {
    let area = MFVideoArea {
        OffsetX: MFOffset {
            fract: fraction,
            value: 32,
        },
        OffsetY: MFOffset {
            fract: 0,
            value: 16,
        },
        Area: windows::Win32::Foundation::SIZE { cx: 64, cy: 64 },
    };
    let bytes = std::slice::from_raw_parts(
        (&area as *const MFVideoArea).cast::<u8>(),
        std::mem::size_of::<MFVideoArea>(),
    );
    media
        .SetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, bytes)
        .unwrap();
}

#[test]
fn native_aperture_comes_from_mf_instead_of_sps_crop_guessing() {
    let image = crate::native_fixture::upload(192, 96, 192, &vec![128; 192 * 96 * 3 / 2]).unwrap();
    unsafe {
        let media = media();
        aperture(&media, 0);
        let described = describe(&media, &facts(), &image).unwrap();
        assert_eq!(
            described.visible_rect,
            VisibleRect {
                x: 32,
                y: 16,
                width: 64,
                height: 64
            }
        );
        assert_eq!(
            described.coded_size,
            ImageSize {
                width: 192,
                height: 96
            }
        );
        assert_eq!(described.chroma_siting, ChromaSiting::Left);
    }
}

#[test]
fn native_description_rejects_missing_crop_fractional_aperture_and_missing_color() {
    let image = crate::native_fixture::upload(192, 96, 192, &vec![128; 192 * 96 * 3 / 2]).unwrap();
    unsafe {
        let media = media();
        assert!(describe(&media, &facts(), &image).is_err());
        aperture(&media, 1);
        assert!(describe(&media, &facts(), &image).is_err());
        aperture(&media, 0);
        media.DeleteItem(&MF_MT_YUV_MATRIX).unwrap();
        assert!(describe(&media, &facts(), &image).is_err());
    }
}

#[test]
fn native_cropped_allocation_keeps_coded_size_and_needs_no_remaining_aperture() {
    let image = crate::native_fixture::upload(64, 64, 64, &vec![128; 64 * 64 * 3 / 2]).unwrap();
    unsafe {
        let media = media();
        media
            .SetUINT64(&MF_MT_FRAME_SIZE, (64_u64 << 32) | 64)
            .unwrap();
        let described = describe(&media, &facts(), &image).unwrap();
        assert_eq!(
            described.coded_size,
            ImageSize {
                width: 192,
                height: 96
            }
        );
        assert_eq!(
            described.visible_rect,
            VisibleRect {
                x: 0,
                y: 0,
                width: 64,
                height: 64
            }
        );
        media
            .SetUINT64(&MF_MT_FRAME_SIZE, (192_u64 << 32) | 96)
            .unwrap();
        assert!(describe(&media, &facts(), &image).is_err());
    }
}
