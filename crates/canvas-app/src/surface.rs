//! Textura offscreen donde vello pinta el lienzo, registrada en egui para
//! poder mostrarla como imagen.

use canvas_render::{CanvasRenderer, RenderError};
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use vello::wgpu;

pub struct CanvasSurface {
    texture: wgpu::Texture,
    egui_id: egui::TextureId,
    size: [u32; 2],
}

impl CanvasSurface {
    /// Garantiza que el slot contiene una textura del tamaño pedido,
    /// (re)registrándola en egui si hace falta.
    pub fn ensure<'a>(
        slot: &'a mut Option<CanvasSurface>,
        rs: &RenderState,
        width: u32,
        height: u32,
    ) -> &'a mut CanvasSurface {
        let width = width.max(1);
        let height = height.max(1);
        let stale = match slot {
            Some(s) => s.size != [width, height],
            None => true,
        };
        if stale {
            let texture = CanvasRenderer::create_target_texture(&rs.device, width, height);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut egui_renderer = rs.renderer.write();
            let egui_id = match slot.take() {
                Some(old) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        &rs.device,
                        &view,
                        wgpu::FilterMode::Nearest,
                        old.egui_id,
                    );
                    old.egui_id
                }
                None => egui_renderer.register_native_texture(
                    &rs.device,
                    &view,
                    wgpu::FilterMode::Nearest,
                ),
            };
            drop(egui_renderer);
            *slot = Some(CanvasSurface {
                texture,
                egui_id,
                size: [width, height],
            });
        }
        slot.as_mut()
            .unwrap_or_else(|| unreachable!("recién asegurado"))
    }

    pub fn render(
        &self,
        rs: &RenderState,
        renderer: &mut CanvasRenderer,
        scene: &vello::Scene,
    ) -> Result<(), RenderError> {
        let view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        renderer.render_to_texture(
            &rs.device,
            &rs.queue,
            scene,
            &view,
            self.size[0],
            self.size[1],
        )
    }

    pub fn egui_id(&self) -> egui::TextureId {
        self.egui_id
    }
}
