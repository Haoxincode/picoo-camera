//! Draw completed opaque BGRA surfaces with the existing image shaders.
use super::*;

#[cfg(test)]
mod tests;

impl DirectXRenderer {
    pub(super) fn draw_surfaces(&mut self, surfaces: &[PaintSurface]) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;
        for surface in surfaces {
            // The supplier reserves access/completion owners before entering
            // this synchronous draw. Busy/failed images do not block the UI.
            unsafe {
                surface
                    .image_buffer
                    .with_read(&devices.device, &mut |view| {
                        draw_view(
                            &mut self.pipelines.surfaces,
                            &devices.device,
                            &devices.device_context,
                            &self.globals,
                            surface,
                            view,
                        )
                    })
                    .log_err();
            }
        }
        Ok(())
    }
}

unsafe fn draw_view(
    pipeline: &mut PipelineState<PolychromeSprite>,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    globals: &DirectXGlobalElements,
    surface: &PaintSurface,
    view: &ID3D11ShaderResourceView,
) -> Result<()> {
    unsafe {
        let size = surface.image_buffer.size();
        validate_view(view, device, size)?;
        let sprite = PolychromeSprite {
            order: surface.order,
            pad: 0,
            grayscale: false.into(),
            opacity: 1.0,
            bounds: surface.bounds,
            content_mask: surface.content_mask,
            corner_radii: Corners::default(),
            tile: AtlasTile {
                // Only geometry is used: the image never enters the CPU atlas.
                texture_id: AtlasTextureId {
                    index: 0,
                    kind: AtlasTextureKind::Polychrome,
                },
                tile_id: TileId(0),
                padding: 0,
                bounds: Bounds {
                    origin: point(DevicePixels(0), DevicePixels(0)),
                    size,
                },
            },
        };
        pipeline.update_buffer(device, context, &[sprite])?;
        let result = pipeline.draw_range_with_texture(
            context,
            &[Some(view.clone())],
            globals
                .batch_params_buffer
                .as_ref()
                .context("batch params missing")?,
            slice::from_ref(&globals.sampler),
            0,
            1,
        );
        // Later primitives cannot read this image past its completion boundary.
        context.VSSetShaderResources(0, Some(&[None]));
        context.PSSetShaderResources(0, Some(&[None]));
        result
    }
}

unsafe fn validate_view(
    view: &ID3D11ShaderResourceView,
    device: &ID3D11Device,
    size: Size<DevicePixels>,
) -> Result<()> {
    unsafe {
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "invalid surface size"
        );
        anyhow::ensure!(
            view.GetDevice()?.cast::<windows::core::IUnknown>()?
                == device.cast::<windows::core::IUnknown>()?,
            "surface belongs to another device"
        );
        let mut description = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
        view.GetDesc(&mut description);
        anyhow::ensure!(
            description.Format == DXGI_FORMAT_B8G8R8A8_UNORM
                && description.ViewDimension == D3D11_SRV_DIMENSION_TEXTURE2D,
            "surface requires a BGRA8 2D view"
        );
        let mip = description.Anonymous.Texture2D;
        anyhow::ensure!(
            mip.MostDetailedMip == 0 && mip.MipLevels == 1,
            "surface cannot expose mip levels"
        );
        let texture: ID3D11Texture2D = view.GetResource()?.cast()?;
        let mut texture_description = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut texture_description);
        anyhow::ensure!(
            texture_description.Width == size.width.0 as u32
                && texture_description.Height == size.height.0 as u32
                && texture_description.ArraySize == 1
                && texture_description.MipLevels == 1
                && texture_description.SampleDesc.Count == 1,
            "surface dimensions/storage differ from its description"
        );
        Ok(())
    }
}
