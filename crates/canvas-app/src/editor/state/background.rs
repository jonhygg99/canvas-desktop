//! La capa de «fondo desenfocado»: se genera a partir de otra capa, se
//! mantiene siempre cubriendo la pagina, y se rehace cuando su origen cambia.

use canvas_core::{
    cover_transform, InsertLayer, Layer, LayerContent, LayerId, RemoveLayer, SetTransform,
    Transform,
};

use super::EditorState;

impl EditorState {
    /// ¿Está activa (y viva, tras posibles deshacer) la capa de fondo?
    pub(in crate::editor) fn background_active(&self) -> bool {
        self.background_layer
            .is_some_and(|id| self.doc.layer(id).is_ok())
    }

    /// Capa de imagen que serviría de fuente para el fondo desenfocado.
    pub(in crate::editor) fn background_source(&self) -> Option<LayerId> {
        let is_candidate = |l: &Layer| {
            matches!(l.content, LayerContent::Image(_)) && Some(l.id) != self.background_layer
        };
        // La seleccionada si vale; si no, la capa de imagen más alta.
        if let Some(sel) = self.selection.primary() {
            if let Ok(l) = self.doc.layer(sel) {
                if is_candidate(l) {
                    return Some(sel);
                }
            }
        }
        self.doc
            .page()
            .ok()?
            .layers
            .iter()
            .rev()
            .find(|l| is_candidate(l))
            .map(|l| l.id)
    }

    /// Activa/desactiva el fondo desenfocado (capa «cover» de la imagen
    /// fuente con blur 50 por defecto, insertada en el fondo de la pila).
    pub(in crate::editor) fn set_blurred_background(&mut self, on: bool) {
        if !on {
            if let Some(id) = self.background_layer.take() {
                if let Err(e) = self.apply_undo_step(Box::new(RemoveLayer::new(id))) {
                    tracing::error!("quitar fondo falló: {e}");
                }
                // El ImageData se queda en el mapa a propósito: deshacer el
                // RemoveLayer recupera la capa y necesita sus píxeles.
            }
            return;
        }

        let Some(source_id) = self.background_source() else {
            return;
        };
        let Ok(source) = self.doc.layer(source_id) else {
            return;
        };
        let LayerContent::Image(content) = source.content.clone() else {
            return;
        };
        let source_t = source.transform;
        let Some(pixels) = self.images.get(&source_id).cloned() else {
            return;
        };
        let Ok(page) = self.doc.page() else { return };
        let (pw, ph) = (page.width, page.height);

        let mut commands: Vec<Box<dyn canvas_core::Command>> = Vec::new();

        // Si la imagen fuente tapa la página entera, el fondo no se vería:
        // encájala centrada (estilo CapCut) como parte del mismo paso.
        let covers_page = source_t.x <= 0.0
            && source_t.y <= 0.0
            && source_t.x + source_t.width >= pw
            && source_t.y + source_t.height >= ph;
        if covers_page {
            let (nw, nh) = (
                f64::from(content.natural_width),
                f64::from(content.natural_height),
            );
            let mut scale = (pw / nw).min(ph / nh);
            // Si el aspecto coincide con la página, «contain» seguiría
            // tapándola entera y el fondo no se vería: deja un margen.
            if nw * scale >= pw * 0.999 && nh * scale >= ph * 0.999 {
                scale *= 0.85;
            }
            let (w, h) = (nw * scale, nh * scale);
            commands.push(Box::new(SetTransform {
                layer: source_id,
                before: source_t,
                after: Transform::new((pw - w) / 2.0, (ph - h) / 2.0, w, h),
            }));
        }

        let transform = cover_transform(
            f64::from(content.natural_width),
            f64::from(content.natural_height),
            pw,
            ph,
        );
        let id = self.doc.allocate_layer_id();
        let mut layer = Layer::new(
            id,
            "Blurred background",
            transform,
            LayerContent::Image(content),
        );
        layer.effects.blur_radius = 50.0;
        commands.push(Box::new(InsertLayer { index: 0, layer }));

        if let Err(e) = self.apply_undo_step(Box::new(canvas_core::Composite::new(
            "Blurred background",
            commands,
        ))) {
            tracing::error!("añadir fondo falló: {e}");
            return;
        }
        self.images.insert(id, pixels);
        self.background_layer = Some(id);
    }

    /// Recoloca la capa de fondo para que cubra la página actual. Devuelve el
    /// comando (ya aplicado al documento) para integrarlo en un `Composite`.
    pub(in crate::editor) fn resync_background_cover(
        &mut self,
    ) -> Option<Box<dyn canvas_core::Command>> {
        let id = self.background_layer.filter(|_| self.background_active())?;
        let (pw, ph) = self.doc.page().map(|p| (p.width, p.height)).ok()?;
        let layer = self.doc.layer(id).ok()?;
        let LayerContent::Image(img) = &layer.content else {
            return None;
        };
        let before = layer.transform;
        let after = cover_transform(
            f64::from(img.natural_width),
            f64::from(img.natural_height),
            pw,
            ph,
        );
        if after == before {
            return None;
        }
        self.doc.layer_mut(id).ok()?.transform = after;
        Some(Box::new(SetTransform {
            layer: id,
            before,
            after,
        }))
    }
}
