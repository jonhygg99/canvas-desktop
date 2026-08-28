//! Alta y sustitucion de capas: insertar una imagen recien cargada, cambiarle
//! los pixeles a una capa existente, y colocar una capa nueva centrada en la
//! pagina.

use std::path::PathBuf;

use canvas_core::{
    contain_transform, cover_transform, ImageContent, InsertLayer, Layer, LayerContent, LayerId,
    RemoveLayer, Selection, Transform,
};
use canvas_io::LoadedImage;
use canvas_render::image_data_from_rgba;

use super::EditorState;

/// Dónde debe quedar la imagen al insertarla.
enum ImagePlacement {
    /// Centrada en la página; sobre lienzo vacío, «contain» + fondo.
    Centered,
    /// Centrada en un punto concreto (arrastre); sin fondo automático.
    At((f64, f64)),
}

impl EditorState {
    /// Añade una imagen como capa nueva (deshacible) y la selecciona.
    /// `source` es `None` cuando la imagen viene del portapapeles del
    /// sistema (no tiene un archivo de origen en disco).
    ///
    /// Sobre un lienzo VACÍO (sin ninguna capa, el caso de un diseño nuevo),
    /// la imagen se AMPLÍA para tocar el borde que antes llegue («contain»,
    /// estilo CapCut/Canva) en vez de solo encajarla si es mayor que la
    /// página; si con eso no cubre la página entera, se añade también un
    /// fondo desenfocado automático — misma receta que el checkbox «Blurred
    /// background» (`set_blurred_background`), en el mismo paso de deshacer.
    /// Sobre un lienzo con contenido, el comportamiento es el de siempre:
    /// centrada, sin ampliar y sin fondo.
    pub fn add_image_layer(
        &mut self,
        name: impl Into<String>,
        source: Option<PathBuf>,
        img: LoadedImage,
    ) {
        self.insert_image_layer(name.into(), source, img, ImagePlacement::Centered);
    }

    /// Añade una imagen como capa nueva en un punto concreto de la página
    /// (arrastre desde el panel de Unsplash), centrada en `pos`, con el
    /// mismo ajuste de escala que el clic (encoger si supera la página).
    /// Sin fondo automático: el usuario eligió dónde va.
    pub fn add_image_layer_at(
        &mut self,
        name: impl Into<String>,
        pos: (f64, f64),
        img: LoadedImage,
    ) {
        self.insert_image_layer(name.into(), None, img, ImagePlacement::At(pos));
    }

    fn insert_image_layer(
        &mut self,
        name: String,
        source: Option<PathBuf>,
        img: LoadedImage,
        placement: ImagePlacement,
    ) {
        let Ok(page) = self.doc.page() else { return };
        let (pw, ph) = (page.width, page.height);
        let empty = page.layers.is_empty();
        let index = page.layers.len();

        let (nw, nh) = (f64::from(img.width), f64::from(img.height));
        let transform = match placement {
            ImagePlacement::Centered => {
                if empty {
                    contain_transform(nw, nh, pw, ph)
                } else {
                    let scale = (pw / nw).min(ph / nh).min(1.0);
                    let (w, h) = (nw * scale, nh * scale);
                    Transform::new((pw - w) / 2.0, (ph - h) / 2.0, w, h)
                }
            }
            ImagePlacement::At((x, y)) => {
                let scale = (pw / nw).min(ph / nh).min(1.0);
                let (w, h) = (nw * scale, nh * scale);
                Transform::new(x - w / 2.0, y - h / 2.0, w, h)
            }
        };
        // Con el mismo aspecto que la página, "contain" ya la cubre entera:
        // ese margen es solo tolerancia de redondeo, no hueco real.
        let needs_background = matches!(placement, ImagePlacement::Centered)
            && empty
            && !(transform.width >= pw * 0.999 && transform.height >= ph * 0.999);

        let content = ImageContent {
            source_path: source,
            natural_width: img.width,
            natural_height: img.height,
            crop: None,
        };
        let pixels = image_data_from_rgba(img.rgba, img.width, img.height);
        let id = self.doc.allocate_layer_id();
        let layer = Layer::new(id, name, transform, LayerContent::Image(content.clone()));

        let mut commands: Vec<Box<dyn canvas_core::Command>> = Vec::new();
        let mut bg_id = None;
        if needs_background {
            let new_bg_id = self.doc.allocate_layer_id();
            let mut bg = Layer::new(
                new_bg_id,
                "Blurred background",
                cover_transform(nw, nh, pw, ph),
                LayerContent::Image(content),
            );
            bg.effects.blur_radius = 50.0;
            commands.push(Box::new(InsertLayer {
                index: 0,
                layer: bg,
            }));
            bg_id = Some(new_bg_id);
        }
        commands.push(Box::new(InsertLayer {
            index: index + usize::from(bg_id.is_some()),
            layer,
        }));

        if let Err(e) =
            self.apply_undo_step(Box::new(canvas_core::Composite::new("Add image", commands)))
        {
            tracing::error!("añadir capa falló: {e}");
            return;
        }
        if let Some(bg_id) = bg_id {
            self.images.insert(bg_id, pixels.clone());
            self.background_layer = Some(bg_id);
        }
        self.images.insert(id, pixels);
        self.selection = Selection::single(id);
    }

    fn replace_image_content(
        &mut self,
        target: LayerId,
        content: ImageContent,
        pixels: vello::peniko::ImageData,
    ) -> Result<(), String> {
        let (index, old_layer) = {
            let page = self.doc.page().map_err(|e| e.to_string())?;
            let index = page
                .index_of(target)
                .ok_or_else(|| "Selected image was not found".to_owned())?;
            let layer = page.layers[index].clone();
            if !matches!(layer.content, LayerContent::Image(_)) {
                return Err("Selected layer is not an image".to_owned());
            }
            (index, layer)
        };

        let new_id = self.doc.allocate_layer_id();
        let mut new_layer = old_layer;
        new_layer.id = new_id;
        new_layer.content = LayerContent::Image(content);

        self.apply_undo_step(Box::new(canvas_core::Composite::new(
            "Replace image",
            vec![
                Box::new(RemoveLayer::new(target)),
                Box::new(InsertLayer {
                    index,
                    layer: new_layer,
                }),
            ],
        )))
        .map_err(|e| e.to_string())?;

        self.images.insert(new_id, pixels);
        if self.background_layer == Some(target) {
            self.background_layer = Some(new_id);
        }
        self.selection = Selection::single(new_id);
        self.crop_mode = false;
        Ok(())
    }

    pub fn replace_image_layer(
        &mut self,
        target: LayerId,
        source: Option<PathBuf>,
        img: LoadedImage,
    ) -> Result<(), String> {
        let content = ImageContent {
            source_path: source,
            natural_width: img.width,
            natural_height: img.height,
            crop: None,
        };
        let pixels = image_data_from_rgba(img.rgba, img.width, img.height);
        self.replace_image_content(target, content, pixels)
    }

    pub(in crate::editor) fn replace_image_from_layer(
        &mut self,
        target: LayerId,
        source: LayerId,
    ) -> Result<(), String> {
        let (content, pixels) = {
            let layer = self.doc.layer(source).map_err(|e| e.to_string())?;
            let LayerContent::Image(content) = &layer.content else {
                return Err("Source layer is not an image".to_owned());
            };
            let pixels = self
                .images
                .get(&source)
                .cloned()
                .ok_or_else(|| "Source image pixels are not loaded".to_owned())?;
            (content.clone(), pixels)
        };
        self.replace_image_content(target, content, pixels)
    }

    /// Inserta una capa nueva (texto o forma) centrada en la página,
    /// deshacible, y la selecciona.
    pub fn insert_layer_centered(&mut self, name: &str, w: f64, h: f64, content: LayerContent) {
        let Ok(page) = self.doc.page() else { return };
        let (pw, ph) = (page.width, page.height);
        let index = page.layers.len();
        let id = self.doc.allocate_layer_id();
        let layer = Layer::new(
            id,
            name,
            Transform::new((pw - w) / 2.0, (ph - h) / 2.0, w, h),
            content,
        );
        if let Err(e) = self.apply_undo_step(Box::new(InsertLayer { index, layer })) {
            tracing::error!("insertar capa falló: {e}");
            return;
        }
        self.selection = Selection::single(id);
        self.crop_mode = false;
    }
}
