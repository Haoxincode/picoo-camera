use super::*;

impl MetalRenderer {
    pub(super) fn draw_primitives_to_texture(
        &mut self,
        scene: &Scene,
        instance_bindings: &InstanceBindings,
        writer: &mut InstanceBufferWriter,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let mut surface_leases = Vec::new();
        let alpha = if self.opaque { 1. } else { 0. };

        let mut command_encoder = new_command_encoder_for_texture(
            command_buffer,
            texture,
            viewport_size,
            Some(metal::MTLClearColor::new(0., 0., 0., alpha)),
        );

        for batch in scene.batches() {
            match batch {
                PrimitiveBatch::Shadows(range) => {
                    self.draw_shadows(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::Quads(range) => {
                    self.draw_quads(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::Paths(range) => {
                    let paths = &scene.paths[range];
                    command_encoder.end_encoding();

                    let did_draw = self.draw_paths_to_intermediate(
                        paths,
                        writer,
                        viewport_size,
                        command_buffer,
                    )?;

                    command_encoder = new_command_encoder_for_texture(
                        command_buffer,
                        texture,
                        viewport_size,
                        None,
                    );

                    if did_draw {
                        if let Err(error) = self.draw_paths_from_intermediate(
                            paths,
                            writer,
                            viewport_size,
                            command_encoder,
                        ) {
                            command_encoder.end_encoding();
                            return Err(error);
                        }
                    }
                }
                PrimitiveBatch::Underlines(range) => {
                    self.draw_underlines(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::MonochromeSprites { texture_id, range } => self
                    .draw_monochrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::PolychromeSprites { texture_id, range } => self
                    .draw_polychrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::Surfaces(range) => self.draw_surfaces(
                    &scene.surfaces[range.clone()],
                    &mut surface_leases,
                    range.start,
                    instance_bindings,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::SubpixelSprites { .. } => unreachable!(),
            }
        }

        command_encoder.end_encoding();

        // CVMetalTexture and its source CVPixelBuffer must outlive GPU reads.
        // Retaining just the MTLTexture does not satisfy CoreVideo's contract.
        if !surface_leases.is_empty() {
            let leases = Cell::new(Some(surface_leases));
            let completed = ConcreteBlock::new(move |_| drop(leases.take())).copy();
            command_buffer.add_completed_handler(&completed);
        }
        Ok(command_buffer.to_owned())
    }

    fn draw_paths_to_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) -> Result<bool> {
        if paths.is_empty() {
            return Ok(false);
        }
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        let mut vertices = Vec::new();
        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.bounds.intersect(&path.content_mask.bounds),
            }));
        }
        let vertex_instance_bindings = writer.write(&vertices)?;

        let render_pass_descriptor = metal::RenderPassDescriptor::new();
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .unwrap();
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., 0.));

        if let Some(msaa_texture) = &self.path_intermediate_msaa_texture {
            color_attachment.set_texture(Some(msaa_texture));
            color_attachment.set_resolve_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::MultisampleResolve);
        } else {
            color_attachment.set_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::Store);
        }

        let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        command_encoder.set_render_pipeline_state(&self.paths_rasterization_pipeline_state);
        command_encoder.set_vertex_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            PathRasterizationInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.draw_primitives(
            metal::MTLPrimitiveType::Triangle,
            0,
            vertices.len() as u64,
        );

        command_encoder.end_encoding();
        Ok(true)
    }

    fn draw_shadows(
        &self,
        shadows: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if shadows.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.shadows_pipeline_state);
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            ShadowInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            shadows.len() as u64,
            shadows.start as u64,
        );
    }

    fn draw_quads(
        &self,
        quads: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if quads.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.quads_pipeline_state);
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            QuadInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            quads.len() as u64,
            quads.start as u64,
        );
    }

    fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> Result<()> {
        let Some(first_path) = paths.first() else {
            return Ok(());
        };
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        command_encoder.set_render_pipeline_state(&self.path_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(intermediate_texture),
        );

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites;
        if paths.last().unwrap().order == first_path.order {
            sprites = paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect();
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            sprites = vec![PathSprite { bounds }];
        }

        let sprite_instance_bindings = writer.write(&sprites)?;
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&sprite_instance_bindings.buffer),
            sprite_instance_bindings.offset as u64,
        );

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        Ok(())
    }

    fn draw_underlines(
        &self,
        underlines: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if underlines.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.underlines_pipeline_state);
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            UnderlineInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            underlines.len() as u64,
            underlines.start as u64,
        );
    }

    fn draw_monochrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let texture = self.sprite_atlas.metal_texture(texture_id);
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.monochrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_polychrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let texture = self.sprite_atlas.metal_texture(texture_id);
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.polychrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        leases: &mut Vec<(
            core_video::pixel_buffer::CVPixelBuffer,
            core_video::metal_texture::CVMetalTexture,
            core_video::metal_texture::CVMetalTexture,
        )>,
        first_surface: usize,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if surfaces.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.surfaces_pipeline_state);
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Surfaces as u64,
            Some(&instance_bindings.surfaces.buffer),
            instance_bindings.surfaces.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        for (index, surface) in surfaces.iter().enumerate() {
            let texture_size = size(
                DevicePixels::from(surface.image_buffer.get_width() as i32),
                DevicePixels::from(surface.image_buffer.get_height() as i32),
            );

            assert_eq!(
                surface.image_buffer.get_pixel_format(),
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            );

            let y_texture = self
                .core_video_texture_cache
                .create_texture_from_image(
                    surface.image_buffer.as_concrete_TypeRef(),
                    None,
                    MTLPixelFormat::R8Unorm,
                    surface.image_buffer.get_width_of_plane(0),
                    surface.image_buffer.get_height_of_plane(0),
                    0,
                )
                .unwrap();
            let cb_cr_texture = self
                .core_video_texture_cache
                .create_texture_from_image(
                    surface.image_buffer.as_concrete_TypeRef(),
                    None,
                    MTLPixelFormat::RG8Unorm,
                    surface.image_buffer.get_width_of_plane(1),
                    surface.image_buffer.get_height_of_plane(1),
                    1,
                )
                .unwrap();

            command_encoder.set_vertex_bytes(
                SurfaceInputIndex::TextureSize as u64,
                mem::size_of_val(&texture_size) as u64,
                &texture_size as *const Size<DevicePixels> as *const _,
            );
            // let y_texture = y_texture.get_texture().unwrap().
            command_encoder.set_fragment_texture(SurfaceInputIndex::YTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(y_texture.as_concrete_TypeRef());
                Some(metal::TextureRef::from_ptr(texture as *mut _))
            });
            command_encoder.set_fragment_texture(SurfaceInputIndex::CbCrTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(cb_cr_texture.as_concrete_TypeRef());
                Some(metal::TextureRef::from_ptr(texture as *mut _))
            });

            command_encoder.draw_primitives_instanced_base_instance(
                metal::MTLPrimitiveType::Triangle,
                0,
                6,
                1,
                (first_surface + index) as u64,
            );
            leases.push((surface.image_buffer.clone(), y_texture, cb_cr_texture));
        }
    }
}

pub(super) fn new_command_encoder_for_texture<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    texture: &'a metal::TextureRef,
    viewport_size: Size<DevicePixels>,
    clear_color: Option<metal::MTLClearColor>,
) -> &'a metal::RenderCommandEncoderRef {
    let render_pass_descriptor = metal::RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .unwrap();
    color_attachment.set_texture(Some(texture));
    color_attachment.set_store_action(metal::MTLStoreAction::Store);
    if let Some(clear_color) = clear_color {
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(clear_color);
    } else {
        color_attachment.set_load_action(metal::MTLLoadAction::Load);
    }

    let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    command_encoder.set_viewport(metal::MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    command_encoder
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
pub(super) fn read_texture_to_image(texture: &metal::TextureRef) -> Result<RgbaImage> {
    let width = texture.width() as u32;
    let height = texture.height() as u32;
    let bytes_per_row = width as usize * 4;
    let mut pixels = vec![0u8; height as usize * bytes_per_row];

    let region = metal::MTLRegion {
        origin: metal::MTLOrigin { x: 0, y: 0, z: 0 },
        size: metal::MTLSize {
            width: width as u64,
            height: height as u64,
            depth: 1,
        },
    };
    texture.get_bytes(
        pixels.as_mut_ptr() as *mut std::ffi::c_void,
        bytes_per_row as u64,
        region,
        0,
    );

    // Convert BGRA to RGBA (swap B and R channels)
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    RgbaImage::from_raw(width, height, pixels).context("failed to create RgbaImage from pixel data")
}

pub(super) fn build_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::One);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

pub(super) fn build_path_sprite_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::One);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

pub(super) fn build_path_rasterization_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    path_sample_count: u32,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.set_raster_sample_count(path_sample_count as _);
        descriptor.set_alpha_to_coverage_enabled(false);
    }
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}
