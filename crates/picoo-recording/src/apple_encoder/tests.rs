use super::*;
use picoo_frame_hub::{ApplePixelBufferLease, NativeImage};
use picoo_gpu::{AppleRenderer, RenderSpec, Rotation};

#[test]
fn unsupported_requests_and_wrong_target_do_not_submit_to_encoder() {
    assert!(AppleEncoder::new(Codec::Avc, 640, 480, 30, 8_000_000).is_err());
    assert!(AppleEncoder::new(Codec::Hevc, 1280, 720, 25, 8_000_000).is_err());
    let mut encoder = AppleEncoder::new(Codec::Avc, 1920, 1080, 30, 8_000_000).unwrap();
    assert!(encoder.encode(rendered(1280, 720), 0, true).is_err());
    // Rejected input did not mutate the session or consume its first timestamp.
    assert!(encoder.encode(rendered(1920, 1080), 0, true).is_ok());
}

fn rendered(width: u32, height: u32) -> RenderedImage {
    unsafe {
        let surface = CFDictionary::<CFString, CFType>::empty();
        let attributes = CFDictionary::<CFString, CFType>::from_slices(
            &[kCVPixelBufferIOSurfacePropertiesKey],
            &[surface.as_ref()],
        );
        let mut raw = ptr::null_mut();
        assert_eq!(
            CVPixelBufferCreate(
                None,
                width as usize,
                height as usize,
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                Some(attributes.as_opaque()),
                NonNull::from(&mut raw)
            ),
            0
        );
        let buffer = CFRetained::from_raw(NonNull::new(raw).unwrap());
        // Synthetic color only. CPU initialization is limited to this fixture.
        assert_eq!(
            CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            0
        );
        for plane in 0..2 {
            let base = CVPixelBufferGetBaseAddressOfPlane(&buffer, plane).cast::<u8>();
            let size = CVPixelBufferGetBytesPerRowOfPlane(&buffer, plane)
                * CVPixelBufferGetHeightOfPlane(&buffer, plane);
            ptr::write_bytes(base, if plane == 0 { 80 } else { 128 }, size);
        }
        assert_eq!(
            CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            0
        );
        for (key, value) in [
            (
                kCVImageBufferYCbCrMatrixKey,
                kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            ),
            (
                kCVImageBufferColorPrimariesKey,
                kCVImageBufferColorPrimaries_ITU_R_709_2,
            ),
            (
                kCVImageBufferTransferFunctionKey,
                kCVImageBufferTransferFunction_ITU_R_709_2,
            ),
        ] {
            buffer.set_attachment(key, value.as_ref(), CVAttachmentMode::ShouldPropagate);
        }
        let source = NativeImage::Apple(ApplePixelBufferLease::retain_completed(&buffer).unwrap());
        AppleRenderer::new(RenderSpec {
            width,
            height,
            rotation: Rotation::None,
            mirror: false,
            color: OutputColor::Bt709Limited,
            format: OutputFormat::Nv12,
        })
        .unwrap()
        .render(&source)
        .unwrap()
    }
}

#[test]
fn gpu_targets_use_hardware_codec_and_mux_preserves_output() {
    use crate::apple::{AppendOutcome, AppleSegment};
    for (width, height) in [(1280, 720), (1920, 1080)] {
        let image = rendered(width, height);
        for codec in [Codec::Avc, Codec::Hevc] {
            for fps in [30, 60] {
                let mut encoder = AppleEncoder::new(codec, width, height, fps, 8_000_000).unwrap();
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("encoded.mp4");
                let mut segment = None;
                let mut expected = Vec::new();
                for index in 0..3 {
                    let pts = index * 1_000_000 / u64::from(fps);
                    let output = encoder.encode(image.clone(), pts, index == 0).unwrap();
                    output
                        .configuration
                        .validate_visible_size(width, height)
                        .unwrap();
                    if segment.is_none() {
                        segment =
                            Some(AppleSegment::new(&path, output.configuration, fps).unwrap());
                    }
                    let start = std::time::Instant::now();
                    while segment.as_mut().unwrap().append(&output.data, pts).unwrap()
                        == AppendOutcome::Busy
                    {
                        assert!(start.elapsed() < Duration::from_millis(250));
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    expected.push((output.data, pts as i64));
                }
                segment.unwrap().finish().unwrap();
                assert_eq!(crate::apple::tests::read(&path), expected);
                // Opt-in synthetic evidence for an independent file decoder.
                // Never overwrite a previous run or export real camera input.
                if let Some(root) = std::env::var_os("PICOO_ENCODER_PROBE_OUTPUT") {
                    let name = format!("{codec:?}-{height}-{fps}.mp4");
                    let mut output =
                        std::fs::File::create_new(std::path::Path::new(&root).join(name)).unwrap();
                    std::io::copy(&mut std::fs::File::open(&path).unwrap(), &mut output).unwrap();
                }
                assert!(encoder.encode(image.clone(), 0, false).is_err());
            }
        }
    }
}
