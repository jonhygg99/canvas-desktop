//! Vista de carga: mientras el hilo de disco abre un archivo o escanea una
//! carpeta.

use eframe::egui;

/// Vista de carga: solo un spinner mientras el archivo/diseño elegido
/// termina de abrirse en segundo plano.
pub(in crate::app) fn loading_view_ui(ui: &mut egui::Ui, path: &std::path::Path) {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.4);
            ui.add(egui::Spinner::new().size(28.0));
            ui.add_space(8.0);
            ui.label(format!("Loading {name}…"));
        });
    });
}
