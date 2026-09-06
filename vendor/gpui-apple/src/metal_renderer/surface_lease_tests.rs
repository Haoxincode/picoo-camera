//! Actual Metal queue + CoreVideo pool lease regression (REQ-PICOO-NEXT-016).
use super::*;
use core_foundation::{dictionary::CFDictionary, number::CFNumber, string::CFString};
use core_video::{pixel_buffer::*, pixel_buffer_pool::*};

#[test]
fn stalled_surface_read_holds_pool_allocation_until_metal_completion() {
    objc::rc::autoreleasepool(|| {
        let attributes = CFDictionary::from_CFType_pairs(&[
            (
                unsafe { CFString::wrap_under_get_rule(kCVPixelBufferWidthKey) },
                CFNumber::from(64).as_CFType(),
            ),
            (
                unsafe { CFString::wrap_under_get_rule(kCVPixelBufferHeightKey) },
                CFNumber::from(64).as_CFType(),
            ),
            (
                unsafe { CFString::wrap_under_get_rule(kCVPixelBufferPixelFormatTypeKey) },
                CFNumber::from(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange as i64).as_CFType(),
            ),
            (
                unsafe { CFString::wrap_under_get_rule(kCVPixelBufferIOSurfacePropertiesKey) },
                CFDictionary::<CFString, core_foundation::base::CFType>::from_CFType_pairs(&[])
                    .as_CFType(),
            ),
        ]);
        let limits = CFDictionary::from_CFType_pairs(&[(
            unsafe { CFString::wrap_under_get_rule(kCVPixelBufferPoolAllocationThresholdKey) },
            CFNumber::from(1).as_CFType(),
        )]);
        let pool = CVPixelBufferPool::new(None, Some(&attributes)).unwrap();
        let image = pool
            .create_pixel_buffer_with_aux_attributes(Some(&limits))
            .unwrap();
        let mut renderer =
            MetalRenderer::new_headless(Arc::new(Mutex::new(InstanceBufferPool::default())));
        let event = renderer.device.new_shared_event();
        struct SignalOnDrop(metal::SharedEvent);
        impl Drop for SignalOnDrop {
            fn drop(&mut self) {
                self.0.set_signaled_value(1);
            }
        }
        let release = SignalOnDrop(event.to_owned());
        let blocker = renderer.command_queue.new_command_buffer();
        blocker.encode_wait_for_event(&event, 1);
        blocker.commit();

        let mut scene = Scene::default();
        let bounds = Bounds {
            origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size: size(ScaledPixels(64.0), ScaledPixels(64.0)),
        };
        scene.insert_primitive(PaintSurface {
            order: Default::default(),
            bounds,
            content_mask: ContentMask { bounds },
            image_buffer: image,
        });
        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(64);
        descriptor.set_height(64);
        descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        descriptor.set_usage(metal::MTLTextureUsage::RenderTarget);
        let target = renderer.device.new_texture(&descriptor);
        let command = renderer
            .render_frame(&scene, &target, size(DevicePixels(64), DevicePixels(64)))
            .unwrap();
        command.commit();
        drop(scene);
        renderer.core_video_texture_cache.flush(0);
        assert!(
            matches!(
                pool.create_pixel_buffer_with_aux_attributes(Some(&limits)),
                Err(core_video::r#return::kCVReturnWouldExceedAllocationThreshold)
            ),
            "pending GPU reads must prevent pool reuse after Scene releases its source"
        );
        drop(release);
        command.wait_until_completed();
        assert_eq!(command.status(), metal::MTLCommandBufferStatus::Completed);
        renderer.core_video_texture_cache.flush(0);
        assert!(
            pool.create_pixel_buffer_with_aux_attributes(Some(&limits))
                .is_ok(),
            "completed reads must release the source allocation"
        );
    });
}
