use super::*;
use crate::windows::render::pool::OutputPool;
use crate::windows::tests::{diagnostic_context, Runtime, GPU_WORK_TEST_LOCK};
use crate::{OutputColor, RenderSpec, Rotation};

// REQ-PICOO-GPU-008: busy access must not invoke UI drawing; failed images
// cannot return to the producer pool, even if the display drops its lease.
#[test]
fn display_busy_and_failed_surface_preserve_access_contract() {
    let _serial = GPU_WORK_TEST_LOCK.lock().unwrap();
    let _runtime = Runtime::start();
    let producer = diagnostic_context();
    let consumer = diagnostic_context();
    let spec = RenderSpec {
        width: 64,
        height: 32,
        rotation: Rotation::None,
        mirror: false,
        color: OutputColor::RgbFullG22Bt709,
        format: OutputFormat::Bgra8,
    };
    let mut pool = OutputPool::new(spec);
    unsafe {
        let surface = pool.acquire(&producer.device).unwrap();
        let writer = shared::SharedAccess::acquire(&surface).unwrap();
        let reader = Arc::new(WindowsDisplayReader {
            device: Mutex::new(Some(consumer.clone())),
        });
        let display = reader.image(RenderedImage { surface, spec }).unwrap();
        assert!(!display
            .with_read(&consumer.device, |_| panic!(
                "producer still owns the image"
            ))
            .unwrap());
        drop(writer);
        // Keep an earlier read alive deterministically, as a slow GPU would.
        // Repainting this same image must share its access, not AcquireSync again.
        let imported = display.imported.lock().unwrap().as_ref().unwrap().clone();
        shared::acquire_mutex(&imported.mutex).unwrap();
        let earlier_read = Arc::new(ReadAccess {
            image: display.image.clone(),
            imported: imported.clone(),
        });
        *imported.active.lock().unwrap() = Arc::downgrade(&earlier_read);
        // Validate the actual imported native view, without CPU pixel upload.
        assert!(display
            .with_read(&consumer.device, |view| {
                assert_eq!(
                    view.GetDevice().unwrap().cast::<IUnknown>().unwrap(),
                    consumer.device.cast::<IUnknown>().unwrap()
                );
                Ok(())
            })
            .unwrap());
        assert!(display.with_read(&consumer.device, |_| Ok(())).unwrap());
        drop(earlier_read);
        display.image.surface.failed.store(true, Ordering::Release);
        assert!(matches!(
            display.with_read(&consumer.device, |_| panic!("failed image")),
            Err(RenderError::DeviceUnavailable)
        ));
        drop(display);
        assert!(matches!(
            pool.acquire(&producer.device),
            Err(RenderError::DeviceUnavailable)
        ));
        // The production reader intentionally drops completion receivers. Keep
        // this test's serial lifetime until their native callbacks retire.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while consumer.completion_slots.load(Ordering::Acquire) != 0
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(consumer.completion_slots.load(Ordering::Acquire), 0);
    }
}
