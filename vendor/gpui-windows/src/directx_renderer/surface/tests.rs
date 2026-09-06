use super::*;

#[derive(Debug)]
struct Dimensions;
// SAFETY: This geometry-only fixture never supplies a view or invokes a read.
unsafe impl Direct3DSurfaceSource for Dimensions {
    fn size(&self) -> Size<DevicePixels> {
        size(DevicePixels(8), DevicePixels(8))
    }
    unsafe fn with_read(
        &self,
        _: &ID3D11Device,
        _: &mut dyn FnMut(&ID3D11ShaderResourceView) -> Result<()>,
    ) -> Result<bool> {
        Ok(false)
    }
}

unsafe fn device() -> (ID3D11Device, ID3D11DeviceContext) {
    unsafe {
        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .unwrap();
        (device.unwrap(), context.unwrap())
    }
}

unsafe fn texture(device: &ID3D11Device, staging: bool) -> ID3D11Texture2D {
    unsafe {
        let mut texture = None;
        device
            .CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: 8,
                    Height: 8,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: if staging {
                        D3D11_USAGE_STAGING
                    } else {
                        D3D11_USAGE_DEFAULT
                    },
                    BindFlags: if staging {
                        0
                    } else {
                        (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32
                    },
                    CPUAccessFlags: if staging {
                        D3D11_CPU_ACCESS_READ.0 as u32
                    } else {
                        0
                    },
                    ..Default::default()
                },
                None,
                Some(&mut texture),
            )
            .unwrap();
        texture.unwrap()
    }
}

// REQ-PICOO-NEXT-016 / GPU-008: actual BGRA shader pixels and content-mask
// clipping, independent of a visible HWND or the application's Decoder.
#[::core::prelude::v1::test]
fn native_surface_shader_draws_bgra_and_respects_clip() {
    unsafe {
        let (device, context) = device();
        let source = texture(&device, false);
        let target = texture(&device, false);
        let staging = texture(&device, true);
        let mut source_rtv = None;
        let mut target_rtv = None;
        let mut view = None;
        device
            .CreateRenderTargetView(&source, None, Some(&mut source_rtv))
            .unwrap();
        device
            .CreateRenderTargetView(&target, None, Some(&mut target_rtv))
            .unwrap();
        device
            .CreateShaderResourceView(&source, None, Some(&mut view))
            .unwrap();
        let view = view.unwrap();
        context.ClearRenderTargetView(source_rtv.as_ref().unwrap(), &[1.0, 0.0, 0.0, 1.0]);
        context.ClearRenderTargetView(target_rtv.as_ref().unwrap(), &[0.0, 0.0, 0.0, 1.0]);
        context.OMSetRenderTargets(Some(slice::from_ref(&target_rtv)), None);
        context.RSSetViewports(Some(&[D3D11_VIEWPORT {
            Width: 8.0,
            Height: 8.0,
            MaxDepth: 1.0,
            ..Default::default()
        }]));
        let globals = DirectXGlobalElements::new(&device).unwrap();
        update_buffer(
            &context,
            globals.global_params_buffer.as_ref().unwrap(),
            &[GlobalParams {
                viewport_size: [8.0, 8.0],
                ..Default::default()
            }],
        )
        .unwrap();
        context.VSSetConstantBuffers(0, Some(slice::from_ref(&globals.global_params_buffer)));
        context.VSSetConstantBuffers(1, Some(slice::from_ref(&globals.batch_params_buffer)));
        context.PSSetConstantBuffers(0, Some(slice::from_ref(&globals.global_params_buffer)));
        let mut pipeline = PipelineState::new(
            &device,
            "surface_test",
            ShaderModule::PolychromeSprite,
            1,
            create_blend_state(&device).unwrap(),
        )
        .unwrap();
        let surface = PaintSurface {
            order: 0,
            bounds: Bounds {
                origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
                size: size(ScaledPixels(8.0), ScaledPixels(8.0)),
            },
            content_mask: ContentMask {
                bounds: Bounds {
                    origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
                    size: size(ScaledPixels(4.0), ScaledPixels(8.0)),
                },
            },
            image_buffer: Direct3DSurface::new(Dimensions),
        };
        draw_view(&mut pipeline, &device, &context, &globals, &surface, &view).unwrap();
        let mut bound = [None];
        context.PSGetShaderResources(0, Some(&mut bound));
        assert!(bound[0].is_none());
        context.VSGetShaderResources(0, Some(&mut bound));
        assert!(bound[0].is_none());
        context.CopyResource(&staging, &target);
        // Blocking readback is diagnostic only; all source owners remain here.
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .unwrap();
        for y in 0..8usize {
            let row = std::slice::from_raw_parts(
                mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                32,
            );
            for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
                assert_eq!(
                    *pixel,
                    if x < 4 {
                        [0, 0, 255, 255]
                    } else {
                        [0, 0, 0, 255]
                    }
                );
            }
        }
        context.Unmap(&staging, 0);
        assert!(validate_view(&view, &device, size(DevicePixels(7), DevicePixels(8))).is_err());
        let (other, _) = self::device();
        assert!(validate_view(&view, &other, Dimensions.size()).is_err());
        context.ClearState();
    }
}
