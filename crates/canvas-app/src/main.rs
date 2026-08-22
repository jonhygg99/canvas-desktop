//! Binario de Canvas Desktop: ventana eframe/egui con el lienzo vello.

// Subsistema GUI siempre (debug y release): evita la consola negra que
// Windows abre al lanzar la app desde el Explorador (doble clic, "Open
// with", menú contextual de carpeta) o al invocar
// `--register-shell`/`--unregister-shell` (instalador NSIS, botón
// Register/Unregister de Ajustes). No afecta a `cargo run` desde una
// terminal: los logs de `tracing` se siguen viendo porque Windows hereda
// el handle de stdout/stderr de la consola padre independientemente del
// subsistema — el subsistema solo decide si se CREA una consola nueva
// cuando no hay ninguna (el caso de Explorador).
#![windows_subsystem = "windows"]

mod app;
mod clipboard;
mod deck;
mod deck_strip;
mod editor;
mod export;
mod gallery;
mod layers_panel;
mod loader;
mod menus;
mod paste_hook;
mod settings;
mod surface;
mod watcher;
mod welcome;

use anyhow::{anyhow, Context, Result};
use app::App;
use canvas_shell::ShellIntegration as _;
use eframe::egui;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wgpu_core=warn,wgpu_hal=warn".into()),
        )
        .init();

    // Flags headless para el instalador: registran/quitan la integración con
    // el Explorador sin abrir ventana, sin tocar la instancia única. Deben
    // interceptarse antes que cualquier otra cosa en main.
    if let Some(register) = shell_registration_flag(std::env::args()) {
        let shell = canvas_shell::platform();
        let exe =
            std::env::current_exe().context("no se pudo resolver la ruta del ejecutable actual")?;
        if register {
            shell
                .register_file_associations(&exe)
                .map_err(|e| anyhow!("registro de integración con el Explorador fallido: {e}"))?;
            println!("Explorer integration registered.");
        } else {
            shell.unregister_file_associations().map_err(|e| {
                anyhow!("desregistro de integración con el Explorador fallido: {e}")
            })?;
            println!("Explorer integration removed.");
        }
        return Ok(());
    }

    // Identidad ante la barra de tareas (Jump List); antes de crear la ventana.
    canvas_shell::set_app_identity();

    let initial_paths = canvas_shell::open_paths_from_args(std::env::args());

    // Instancia única: si ya hay una app viva, se le envían las rutas por el
    // socket local y este proceso sale con código 0.
    let instance = match canvas_shell::acquire_instance(&initial_paths) {
        canvas_shell::InstanceRole::Secondary => {
            tracing::info!("instancia ya abierta: rutas reenviadas, saliendo");
            return Ok(());
        }
        canvas_shell::InstanceRole::Primary(listener) => Some(listener),
        canvas_shell::InstanceRole::Standalone => None,
    };
    let initial_path = initial_paths.into_iter().next();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([640.0, 480.0]);
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport,
        event_loop_builder: Some(Box::new(paste_hook::install)),
        ..Default::default()
    };

    eframe::run_native(
        "Canvas Desktop",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial_path, instance)?))),
    )
    .map_err(|e| anyhow!("no se pudo arrancar la ventana: {e}"))
}

/// Icono de la ventana (barra de título/alt-tab), generado desde
/// `assets/icon.svg` por `cargo run -p canvas-render --example gen_icons`.
const APP_ICON_PNG: &[u8] =
    include_bytes!("../../../assets/linux/hicolor/256x256/apps/canvas-desktop.png");

fn load_app_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(APP_ICON_PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

/// Busca `--register-shell`/`--unregister-shell` en argv; usado por el
/// instalador NSIS (`nsExec`) para escribir/limpiar el registro sin abrir la
/// app. `Some(true)` = registrar, `Some(false)` = quitar, `None` = ninguno.
fn shell_registration_flag(args: impl Iterator<Item = String>) -> Option<bool> {
    for arg in args {
        match arg.as_str() {
            "--register-shell" => return Some(true),
            "--unregister-shell" => return Some(false),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::shell_registration_flag;

    fn args(items: &[&str]) -> impl Iterator<Item = String> {
        items
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn detects_register_flag() {
        assert_eq!(
            shell_registration_flag(args(&["canvas-desktop.exe", "--register-shell"])),
            Some(true)
        );
    }

    #[test]
    fn detects_unregister_flag() {
        assert_eq!(
            shell_registration_flag(args(&["canvas-desktop.exe", "--unregister-shell"])),
            Some(false)
        );
    }

    #[test]
    fn ignores_unrelated_args() {
        assert_eq!(
            shell_registration_flag(args(&["canvas-desktop.exe", "C:\\photo.png"])),
            None
        );
    }
}
