//! Indicador compartido por rutas; nunca leer disco en cada frame del lienzo.
use eframe::egui;
use std::path::Path;

pub(crate) fn record_saved(ctx: &egui::Context, path: &Path, saved: bool) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(("framing-status", path)), Some(saved)));
}

pub(crate) fn has_saved(ctx: &egui::Context, path: &Path) -> bool {
    let id = egui::Id::new(("framing-status", path));
    let cached = ctx.data(|data| data.get_temp::<Option<bool>>(id));
    if let Some(saved) = cached {
        return saved.unwrap_or(false);
    }
    ctx.data_mut(|data| data.insert_temp::<Option<bool>>(id, None));
    let path = path.to_owned();
    let ctx = ctx.clone();
    rayon::spawn(move || {
        let saved = canvas_io::read_framing(&path).ok().flatten().is_some();
        // Un guardado puede haberse confirmado mientras la lectura seguía pendiente.
        ctx.data_mut(|data| {
            if data
                .get_temp::<Option<bool>>(id)
                .is_some_and(|value| value.is_none())
            {
                data.insert_temp(id, Some(saved));
            }
        });
        ctx.request_repaint();
    });
    false
}
