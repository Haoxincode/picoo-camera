use super::*;

impl Window {
    /// Paint an immutable BGRA Direct3D image at the current scene z-index.
    /// Call only during an element's paint phase.
    pub fn paint_surface(&mut self, bounds: Bounds<Pixels>, image_buffer: crate::Direct3DSurface) {
        self.invalidator.debug_assert_paint();
        let bounds = self.snap_bounds(bounds);
        let content_mask = self.snapped_content_mask();
        self.next_frame.scene.insert_primitive(crate::PaintSurface {
            order: 0,
            bounds,
            content_mask,
            image_buffer,
        });
    }
}
