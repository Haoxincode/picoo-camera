use super::*;
use crate::windows::tests::{diagnostic_context, Runtime};
use picoo_frame_hub::VisibleRect;

fn spec(rotation: Rotation, mirror: bool) -> RenderSpec {
    RenderSpec {
        width: 1280,
        height: 720,
        rotation,
        mirror,
        color: OutputColor::Bt709Limited,
        format: crate::OutputFormat::Nv12,
    }
}
fn rect(rect: RECT) -> (i32, i32, i32, i32) {
    (rect.left, rect.top, rect.right, rect.bottom)
}

#[test]
fn native_crop_follows_rotation_then_mirror_before_processor_clipping() {
    let size = ImageSize {
        width: 192,
        height: 96,
    };
    let crop = VisibleRect {
        x: 16,
        y: 8,
        width: 128,
        height: 64,
    };
    for (rotation, unmirrored, mirrored, destination) in [
        (
            Rotation::None,
            (16, 8, 144, 72),
            (48, 8, 176, 72),
            (0, 40, 1280, 680),
        ),
        (
            Rotation::Clockwise90,
            (24, 16, 88, 144),
            (8, 16, 72, 144),
            (460, 0, 820, 720),
        ),
        (
            Rotation::Clockwise180,
            (48, 24, 176, 88),
            (16, 24, 144, 88),
            (0, 40, 1280, 680),
        ),
        (
            Rotation::Clockwise270,
            (8, 48, 72, 176),
            (24, 48, 88, 176),
            (460, 0, 820, 720),
        ),
    ] {
        for (mirror, expected) in [(false, unmirrored), (true, mirrored)] {
            let (source, target) =
                geometry::rectangles(size, crop, spec(rotation, mirror)).unwrap();
            assert_eq!(rect(source), expected);
            assert_eq!(rect(target), destination);
        }
    }
}

#[test]
fn rotated_1088_allocation_never_includes_the_eight_padding_rows() {
    let size = ImageSize {
        width: 1920,
        height: 1088,
    };
    let crop = VisibleRect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let (plain, _) = geometry::rectangles(size, crop, spec(Rotation::Clockwise90, false)).unwrap();
    let (mirror, _) = geometry::rectangles(size, crop, spec(Rotation::Clockwise90, true)).unwrap();
    assert_eq!(rect(plain), (8, 0, 1088, 1920));
    assert_eq!(rect(mirror), (0, 0, 1080, 1920));
    assert!(geometry::rectangles(
        size,
        VisibleRect {
            x: u32::MAX,
            ..crop
        },
        spec(Rotation::None, false)
    )
    .is_err());
}

#[test]
fn native_output_pool_cannot_reuse_a_retained_surface_or_expand_past_three() {
    let _runtime = Runtime::start();
    let gpu = diagnostic_context();
    let mut pool = OutputPool::new(RenderSpec {
        width: 64,
        height: 64,
        ..spec(Rotation::None, false)
    });
    unsafe {
        let first = pool.acquire(&gpu.device).unwrap();
        let identity = first.texture.as_raw();
        let second = pool.acquire(&gpu.device).unwrap();
        let third = pool.acquire(&gpu.device).unwrap();
        assert!(matches!(
            pool.acquire(&gpu.device),
            Err(RenderError::PoolFull)
        ));
        drop(first);
        let reused = pool.acquire(&gpu.device).unwrap();
        assert_eq!(reused.texture.as_raw(), identity);
        assert!(matches!(
            pool.acquire(&gpu.device),
            Err(RenderError::PoolFull)
        ));
        drop((second, third, reused));
    }
}
