use super::*;
use crate::windows::{
    render::completed_fixture,
    tests::{diagnostic_context, Runtime, GPU_WORK_TEST_LOCK},
};
use crate::{OutputColor, Rotation};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA,
    D3D11_USAGE_DEFAULT,
};

fn spec() -> RenderSpec {
    RenderSpec {
        width: 64,
        height: 64,
        rotation: Rotation::None,
        mirror: false,
        color: OutputColor::Bt709Limited,
        format: crate::OutputFormat::Nv12,
    }
}
struct Upload(Option<ID3D11Texture2D>);
// SAFETY: A diagnostic immutable D3D11 texture, retained through initial upload.
unsafe impl Send for Upload {}

fn fixture(gpu: &Arc<WindowsGpuContext>) -> (RenderedImage, Vec<u8>) {
    let spec = spec();
    let width = spec.width as usize;
    let height = spec.height as usize;
    let pitch = width + 16;
    let rows = height * 3 / 2;
    let mut padded = vec![0xee; pitch * rows];
    let mut expected = Vec::new();
    for row in 0..rows {
        for column in 0..width {
            let value = if row < height {
                16 + row as u8
            } else if column % 2 == 0 {
                80 + (row - height) as u8
            } else {
                190 - (row - height) as u8
            };
            padded[row * pitch + column] = value;
            expected.push(value);
        }
    }
    let upload = unsafe {
        gpu.submit_owned(Upload(None), |gpu, upload| {
            gpu.device.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: spec.width,
                    Height: spec.height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_NV12,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
                    ..Default::default()
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: padded.as_ptr().cast(),
                    SysMemPitch: pitch as u32,
                    SysMemSlicePitch: padded.len() as u32,
                }),
                Some(&mut upload.0),
            )?;
            Ok(())
        })
        .unwrap()
        .wait_on_worker()
        .unwrap()
    };
    (
        unsafe { completed_fixture(upload.0.unwrap(), spec) },
        expected,
    )
}

#[test]
fn native_readback_removes_padding_and_obeys_held_cpu_output_capacity() {
    let _serial = GPU_WORK_TEST_LOCK.lock().unwrap();
    let _runtime = Runtime::start();
    let gpu = diagnostic_context();
    let (image, expected) = fixture(&gpu);
    let mut exporter = CpuExporter::new(gpu, spec()).unwrap();
    assert!(exporter.staging.is_none());
    assert_eq!(exporter.exports(), 0);
    let first = exporter.export(&image).unwrap();
    assert_eq!(first.pixels(), expected);
    assert_eq!(first.stride(), spec().width);
    let identity = Arc::as_ptr(&first);
    let weak = Arc::downgrade(&first);
    let second = exporter.export(&image).unwrap();
    let third = exporter.export(&image).unwrap();
    let staging = exporter.staging.as_ref().unwrap().0.as_raw();
    assert!(matches!(
        exporter.export(&image),
        Err(RenderError::PoolFull)
    ));
    assert_eq!(exporter.exports(), 3);
    drop(first);
    assert!(
        matches!(exporter.export(&image), Err(RenderError::PoolFull)),
        "a Weak reader can still recover the old pixels"
    );
    drop(weak);
    let reused = exporter.export(&image).unwrap();
    assert_eq!(Arc::as_ptr(&reused), identity);
    assert_eq!(exporter.staging.as_ref().unwrap().0.as_raw(), staging);
    assert_eq!(reused.pixels(), expected);
    assert_eq!(second.pixels(), expected);
    assert_eq!(third.pixels(), expected);
    assert_eq!(exporter.exports(), 4);
}

#[test]
fn layout_and_device_mismatch_fail_before_staging_or_readback() {
    let _serial = GPU_WORK_TEST_LOCK.lock().unwrap();
    let _runtime = Runtime::start();
    let gpu = diagnostic_context();
    let (image, _) = fixture(&gpu);
    let mut layout = CpuExporter::new(
        gpu,
        RenderSpec {
            mirror: true,
            ..spec()
        },
    )
    .unwrap();
    assert!(matches!(
        layout.export(&image),
        Err(RenderError::OutputLayoutMismatch)
    ));
    assert!(layout.staging.is_none());
    assert_eq!(layout.exports(), 0);
    let mut device = CpuExporter::new(diagnostic_context(), spec()).unwrap();
    assert!(device.export(&image).is_err());
    assert!(device.staging.is_none());
    assert_eq!(device.exports(), 0);
}
