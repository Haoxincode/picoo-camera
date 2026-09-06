use super::*;

fn spec(color: OutputColor) -> RenderSpec {
    RenderSpec {
        width: 1280,
        height: 720,
        rotation: Rotation::None,
        mirror: false,
        color,
    }
}

/// CPU writes here produce a diagnostic source fixture, never product frames.
fn red_source(declare_color: bool) -> NativeImage {
    source_fixture(declare_color, false)
}

fn source_fixture(declare_color: bool, quadrants: bool) -> NativeImage {
    let mut pool = OutputPool::new(spec(OutputColor::Bt709Limited)).unwrap();
    let buffer = pool.acquire().unwrap();
    unsafe {
        assert_eq!(
            CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            0
        );
        for plane in 0..2 {
            let base = CVPixelBufferGetBaseAddressOfPlane(&buffer, plane).cast::<u8>();
            let stride = CVPixelBufferGetBytesPerRowOfPlane(&buffer, plane);
            let height = CVPixelBufferGetHeightOfPlane(&buffer, plane);
            for y in 0..height {
                for x in 0..1280 {
                    *base.add(y * stride + x) = if quadrants {
                        if plane == 0 {
                            [40, 80, 160, 220][usize::from(y >= 360) * 2 + usize::from(x >= 640)]
                        } else {
                            128
                        }
                    } else if plane == 0 {
                        63
                    } else if x % 2 == 0 {
                        102
                    } else {
                        240
                    };
                }
            }
        }
        assert_eq!(
            CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            0
        );
        if declare_color {
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
        }
        // No mutations after publishing; the native owner outlives this pool.
        NativeImage::retain_completed(&buffer).unwrap()
    }
}

fn sample(image: &RenderedImage, x: usize, y: usize) -> [u8; 3] {
    // Diagnostic readback only, after the GPU task's completion.
    unsafe {
        let buffer = image.pixel_buffer();
        assert_eq!(
            CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly),
            0
        );
        let luma = CVPixelBufferGetBaseAddressOfPlane(buffer, 0).cast::<u8>();
        let chroma = CVPixelBufferGetBaseAddressOfPlane(buffer, 1).cast::<u8>();
        let luma_offset = y * CVPixelBufferGetBytesPerRowOfPlane(buffer, 0) + x;
        let chroma_offset = y / 2 * CVPixelBufferGetBytesPerRowOfPlane(buffer, 1) + x / 2 * 2;
        let result = [
            *luma.add(luma_offset),
            *chroma.add(chroma_offset),
            *chroma.add(chroma_offset + 1),
        ];
        assert_eq!(
            CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly),
            0
        );
        result
    }
}

#[test]
fn metal_converts_native_709_source_to_both_output_contracts() {
    let source = red_source(true);
    for (color, expected) in [
        (OutputColor::Bt601Full, [76, 85, 255]),
        (OutputColor::Bt709Limited, [63, 102, 240]),
    ] {
        let mut renderer = AppleRenderer::new(spec(color)).unwrap();
        let output = renderer.render(&source).unwrap();
        let actual = sample(&output, 640, 360);
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (i32::from(actual) - expected).abs() <= 3,
                "{color:?}: {actual} vs {expected}"
            );
        }
    }
}

#[test]
fn slow_consumer_cannot_expand_pool_or_overwrite_retained_output() {
    let source = red_source(true);
    let mut renderer = AppleRenderer::new(spec(OutputColor::Bt601Full)).unwrap();
    let first = renderer.render(&source).unwrap();
    let clone = first.clone();
    let second = renderer.render(&source).unwrap();
    let third = renderer.render(&source).unwrap();
    assert!(matches!(
        renderer.render(&source),
        Err(RenderError::PoolFull)
    ));
    drop(first);
    assert!(matches!(
        renderer.render(&source),
        Err(RenderError::PoolFull)
    ));
    let old_pixels = sample(&clone, 640, 360);
    drop(second);
    let fourth = renderer.render(&source).unwrap();
    assert_eq!(sample(&clone, 640, 360), old_pixels);
    assert_eq!(sample(&fourth, 640, 360), old_pixels);
    drop((clone, third, fourth));
    assert!(renderer.render(&source).is_ok());
}

#[test]
fn contain_keeps_source_aspect_and_fills_opaque_black_margins() {
    let source = red_source(true);
    let mut output_spec = spec(OutputColor::Bt709Limited);
    output_spec.width = 720;
    let mut renderer = AppleRenderer::new(output_spec).unwrap();
    let output = renderer.render(&source).unwrap();
    assert_eq!(sample(&output, 360, 30), [16, 128, 128]);
    assert_eq!(sample(&output, 360, 690), [16, 128, 128]);
    assert!((i32::from(sample(&output, 360, 360)[0]) - 63).abs() <= 3);
}

#[test]
fn unknown_source_color_is_rejected_before_render() {
    let mut renderer = AppleRenderer::new(spec(OutputColor::Bt601Full)).unwrap();
    assert!(matches!(
        renderer.render(&red_source(false)),
        Err(RenderError::UnsupportedSourceColor)
    ));
}

#[test]
fn completed_output_outlives_renderer_on_another_consumer_thread() {
    let source = red_source(true);
    let mut renderer = AppleRenderer::new(spec(OutputColor::Bt709Limited)).unwrap();
    let output = renderer.render(&source).unwrap();
    drop((renderer, source));
    let pixels = std::thread::spawn(move || sample(&output, 640, 360))
        .join()
        .unwrap();
    assert!((i32::from(pixels[0]) - 63).abs() <= 3);
}

#[test]
fn cpu_export_materializes_only_the_completed_output_and_is_bounded() {
    let source = red_source(true);
    let output_spec = spec(OutputColor::Bt709Limited);
    let mut renderer = AppleRenderer::new(output_spec).unwrap();
    let rendered = renderer.render(&source).unwrap();
    let mut exporter = CpuExporter::new(output_spec).unwrap();
    assert_eq!(exporter.exports(), 0);
    let first = exporter.export(&rendered).unwrap();
    let clone = first.clone();
    let second = exporter.export(&rendered).unwrap();
    let third = exporter.export(&rendered).unwrap();
    assert_eq!(exporter.exports(), 3);
    assert!(matches!(
        exporter.export(&rendered),
        Err(RenderError::PoolFull)
    ));
    drop(first);
    assert!(matches!(
        exporter.export(&rendered),
        Err(RenderError::PoolFull)
    ));
    assert_eq!(exporter.exports(), 3);
    let expected = sample(&rendered, 640, 360);
    let y = clone.pixels()[360 * 1280 + 640];
    let uv_offset = 1280 * 720 + 180 * 1280 + 640;
    assert_eq!(
        [y, clone.pixels()[uv_offset], clone.pixels()[uv_offset + 1]],
        expected
    );
    assert_eq!(clone.stride(), 1280);
    assert_eq!(clone.pixels().len(), 1280 * 720 * 3 / 2);
    drop(second);
    let fourth = exporter.export(&rendered).unwrap();
    assert_eq!(exporter.exports(), 4);
    assert_eq!(fourth.pixels(), clone.pixels());
    drop((third, fourth, rendered, renderer, source, exporter));
    assert_eq!(clone.pixels()[360 * 1280 + 640], y);
}

#[test]
fn cpu_export_rejects_different_output_color_without_readback() {
    let source = red_source(true);
    let mut renderer = AppleRenderer::new(spec(OutputColor::Bt601Full)).unwrap();
    let rendered = renderer.render(&source).unwrap();
    let mut exporter = CpuExporter::new(spec(OutputColor::Bt709Limited)).unwrap();
    assert!(matches!(
        exporter.export(&rendered),
        Err(RenderError::OutputLayoutMismatch)
    ));
    assert_eq!(exporter.exports(), 0);
}

#[test]
fn rotation_then_mirror_matches_source_quadrants() {
    let source = source_fixture(true, true);
    for (rotation, expected) in [
        (Rotation::None, [40, 80, 160, 220]),
        (Rotation::Clockwise90, [160, 40, 220, 80]),
        (Rotation::Clockwise180, [220, 160, 80, 40]),
        (Rotation::Clockwise270, [80, 220, 40, 160]),
    ] {
        for mirror in [false, true] {
            let mut output_spec = spec(OutputColor::Bt709Limited);
            output_spec.rotation = rotation;
            output_spec.mirror = mirror;
            if matches!(rotation, Rotation::Clockwise90 | Rotation::Clockwise270) {
                std::mem::swap(&mut output_spec.width, &mut output_spec.height);
            }
            let mut renderer = AppleRenderer::new(output_spec).unwrap();
            let output = renderer.render(&source).unwrap();
            let mut expected = expected;
            if mirror {
                expected.swap(0, 1);
                expected.swap(2, 3);
            }
            for (ix, expected) in expected.into_iter().enumerate() {
                let x = output_spec.width as usize * (if ix % 2 == 0 { 1 } else { 3 }) / 4;
                let y = output_spec.height as usize * (if ix < 2 { 1 } else { 3 }) / 4;
                let actual = sample(&output, x, y)[0];
                assert!(
                    (i32::from(actual) - expected).abs() <= 2,
                    "{rotation:?} mirror={mirror} corner={ix}: {actual} vs {expected}"
                );
            }
        }
    }
}
