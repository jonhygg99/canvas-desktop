//! Menús de la aplicación.
//!
//! En Windows son menús nativos con `muda` enganchados al HWND de la ventana
//! (`Menu::init_for_hwnd`) y sondeados cada frame con
//! `MenuEvent::receiver().try_recv()`. Matiz importante: los aceleradores
//! NATIVOS de muda necesitarían `TranslateAcceleratorW` en el bucle de
//! mensajes, que eframe no expone — por eso los atajos de teclado los sigue
//! gestionando egui y el menú solo los muestra como texto decorativo.
//!
//! Fuera de Windows (muda exigiría GTK en Linux) el fallback es una barra de
//! menús egui con las mismas acciones.

use std::path::PathBuf;

/// Acción de menú, común a la implementación nativa y al fallback egui.
#[derive(Clone)]
pub enum MenuAction {
    NewDesign,
    OpenFile,
    OpenFolder,
    Save,
    SaveAs,
    OpenRecent(PathBuf),
    Quit,
    Undo,
    Redo,
    ZoomIn,
    ZoomOut,
    FitToWindow,
    ToggleGrid,
    ToggleRulers,
    FullScreen,
    Settings,
    About,
}

#[cfg(windows)]
pub use native::AppMenus;

#[cfg(windows)]
mod native {
    use super::MenuAction;
    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use std::path::PathBuf;

    pub struct AppMenus {
        /// El menú debe seguir vivo mientras la ventana exista.
        _menu: Menu,
        recent_menu: Submenu,
        recent_items: Vec<(MenuItem, PathBuf)>,
        /// Ítems que solo tienen sentido con un documento abierto.
        editor_items: Vec<MenuItem>,
        editor_enabled: bool,
    }

    fn accel(mods: Modifiers, code: Code) -> Option<Accelerator> {
        Some(Accelerator::new(Some(mods), code))
    }

    impl AppMenus {
        /// Construye e instala el menú nativo en la ventana. `None` si algo
        /// falla: la app funciona igual, solo que sin barra de menús.
        pub fn install(hwnd: isize) -> Option<Self> {
            match Self::build(hwnd) {
                Ok(menus) => Some(menus),
                Err(e) => {
                    tracing::warn!("no se pudo instalar el menú nativo: {e}");
                    None
                }
            }
        }

        fn build(hwnd: isize) -> Result<Self, muda::Error> {
            let ctrl = Modifiers::CONTROL;
            let ctrl_shift = Modifiers::CONTROL | Modifiers::SHIFT;

            let new_item = MenuItem::with_id("new", "New Design", true, accel(ctrl, Code::KeyN));
            let open_item = MenuItem::with_id("open", "Open…", true, accel(ctrl, Code::KeyO));
            let open_folder_item = MenuItem::with_id(
                "open_folder",
                "Open Folder…",
                true,
                accel(ctrl_shift, Code::KeyO),
            );
            let save_item = MenuItem::with_id("save", "Save", false, accel(ctrl, Code::KeyS));
            let save_as_item =
                MenuItem::with_id("save_as", "Save As…", false, accel(ctrl_shift, Code::KeyS));
            let export_item = MenuItem::with_id("export", "Export…", false, None);
            let recent_menu = Submenu::with_id("recent", "Open Recent", true);
            let quit_item = MenuItem::with_id("quit", "Quit", true, accel(ctrl, Code::KeyQ));

            let file = Submenu::with_items(
                "&File",
                true,
                &[
                    &new_item,
                    &open_item,
                    &open_folder_item,
                    &PredefinedMenuItem::separator(),
                    &save_item,
                    &save_as_item,
                    &export_item,
                    &PredefinedMenuItem::separator(),
                    &recent_menu,
                    &PredefinedMenuItem::separator(),
                    &quit_item,
                ],
            )?;

            let undo_item = MenuItem::with_id("undo", "Undo", false, accel(ctrl, Code::KeyZ));
            let redo_item = MenuItem::with_id("redo", "Redo", false, accel(ctrl, Code::KeyY));
            // Pendientes de sus fases (portapapeles, selección múltiple).
            let cut_item = MenuItem::with_id("cut", "Cut", false, accel(ctrl, Code::KeyX));
            let copy_item = MenuItem::with_id("copy", "Copy", false, accel(ctrl, Code::KeyC));
            let paste_item = MenuItem::with_id("paste", "Paste", false, accel(ctrl, Code::KeyV));
            let duplicate_item =
                MenuItem::with_id("duplicate", "Duplicate", false, accel(ctrl, Code::KeyD));
            let delete_item = MenuItem::with_id("delete", "Delete", false, None);
            let select_all_item =
                MenuItem::with_id("select_all", "Select All", false, accel(ctrl, Code::KeyA));

            let edit = Submenu::with_items(
                "&Edit",
                true,
                &[
                    &undo_item,
                    &redo_item,
                    &PredefinedMenuItem::separator(),
                    &cut_item,
                    &copy_item,
                    &paste_item,
                    &duplicate_item,
                    &delete_item,
                    &PredefinedMenuItem::separator(),
                    &select_all_item,
                ],
            )?;

            let zoom_in_item =
                MenuItem::with_id("zoom_in", "Zoom In", false, accel(ctrl, Code::Equal));
            let zoom_out_item =
                MenuItem::with_id("zoom_out", "Zoom Out", false, accel(ctrl, Code::Minus));
            let fit_item =
                MenuItem::with_id("fit", "Fit to Window", false, accel(ctrl, Code::Digit0));
            let grid_item = MenuItem::with_id("grid", "Grid", false, None);
            let rulers_item = MenuItem::with_id("rulers", "Rulers", false, None);
            let full_screen_item = MenuItem::with_id("full_screen", "Full Screen", true, None);

            let view = Submenu::with_items(
                "&View",
                true,
                &[
                    &zoom_in_item,
                    &zoom_out_item,
                    &fit_item,
                    &PredefinedMenuItem::separator(),
                    &grid_item,
                    &rulers_item,
                    &PredefinedMenuItem::separator(),
                    &full_screen_item,
                ],
            )?;

            let settings_item = MenuItem::with_id("settings", "Settings…", true, None);
            let about_item = MenuItem::with_id("about", "About Canvas Desktop", true, None);
            let help = Submenu::with_items("&Help", true, &[&settings_item, &about_item])?;

            let menu = Menu::with_items(&[&file, &edit, &view, &help])?;
            // SAFETY: el HWND viene de la ventana viva de eframe; muda
            // subclasea su WndProc para pintar y despachar el menú.
            unsafe { menu.init_for_hwnd(hwnd)? };

            let editor_items = vec![
                save_item,
                save_as_item,
                undo_item,
                redo_item,
                zoom_in_item,
                zoom_out_item,
                fit_item,
                grid_item,
                rulers_item,
            ];

            Ok(Self {
                _menu: menu,
                recent_menu,
                recent_items: Vec::new(),
                editor_items,
                editor_enabled: false,
            })
        }

        /// Un clic de menú pendiente, si lo hay (sondeado cada frame).
        pub fn poll(&self) -> Option<MenuAction> {
            let event = MenuEvent::receiver().try_recv().ok()?;
            if let Some((_, path)) = self
                .recent_items
                .iter()
                .find(|(item, _)| item.id() == &event.id)
            {
                return Some(MenuAction::OpenRecent(path.clone()));
            }
            match event.id.0.as_str() {
                "new" => Some(MenuAction::NewDesign),
                "open" => Some(MenuAction::OpenFile),
                "open_folder" => Some(MenuAction::OpenFolder),
                "save" => Some(MenuAction::Save),
                "save_as" => Some(MenuAction::SaveAs),
                "quit" => Some(MenuAction::Quit),
                "undo" => Some(MenuAction::Undo),
                "redo" => Some(MenuAction::Redo),
                "zoom_in" => Some(MenuAction::ZoomIn),
                "zoom_out" => Some(MenuAction::ZoomOut),
                "fit" => Some(MenuAction::FitToWindow),
                "grid" => Some(MenuAction::ToggleGrid),
                "rulers" => Some(MenuAction::ToggleRulers),
                "full_screen" => Some(MenuAction::FullScreen),
                "settings" => Some(MenuAction::Settings),
                "about" => Some(MenuAction::About),
                _ => None,
            }
        }

        /// Habilita/deshabilita los ítems que requieren un editor abierto.
        pub fn set_editor_enabled(&mut self, enabled: bool) {
            if self.editor_enabled == enabled {
                return;
            }
            self.editor_enabled = enabled;
            for item in &self.editor_items {
                item.set_enabled(enabled);
            }
        }

        /// Reconstruye el submenú «Open Recent».
        pub fn set_recents(&mut self, recents: &[PathBuf]) {
            for (item, _) in self.recent_items.drain(..) {
                let _ = self.recent_menu.remove(&item);
            }
            for (i, path) in recents.iter().enumerate() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let item = MenuItem::with_id(format!("recent_{i}"), name, true, None);
                if self.recent_menu.append(&item).is_ok() {
                    self.recent_items.push((item, path.clone()));
                }
            }
        }
    }
}

/// Fallback sin menú nativo (macOS/Linux hasta sus fases): la app pinta una
/// barra de menús egui con `menu_bar_ui`.
#[cfg(not(windows))]
pub struct AppMenus;

#[cfg(not(windows))]
impl AppMenus {
    pub fn install(_hwnd: isize) -> Option<Self> {
        None
    }
    pub fn poll(&self) -> Option<MenuAction> {
        None
    }
    pub fn set_editor_enabled(&mut self, _enabled: bool) {}
    pub fn set_recents(&mut self, _recents: &[PathBuf]) {}
}

/// Barra de menús egui con las mismas acciones (fallback no-Windows).
#[cfg(not(windows))]
pub fn menu_bar_ui(
    ui: &mut eframe::egui::Ui,
    editor_open: bool,
    recents: &[PathBuf],
) -> Option<MenuAction> {
    use eframe::egui;
    let mut action = None;
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("New Design").clicked() {
                action = Some(MenuAction::NewDesign);
            }
            if ui.button("Open…").clicked() {
                action = Some(MenuAction::OpenFile);
            }
            if ui.button("Open Folder…").clicked() {
                action = Some(MenuAction::OpenFolder);
            }
            ui.separator();
            if ui
                .add_enabled(editor_open, egui::Button::new("Save"))
                .clicked()
            {
                action = Some(MenuAction::Save);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Save As…"))
                .clicked()
            {
                action = Some(MenuAction::SaveAs);
            }
            ui.separator();
            ui.menu_button("Open Recent", |ui| {
                for path in recents {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    if ui.button(name).clicked() {
                        action = Some(MenuAction::OpenRecent(path.clone()));
                    }
                }
            });
            ui.separator();
            if ui.button("Quit").clicked() {
                action = Some(MenuAction::Quit);
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui
                .add_enabled(editor_open, egui::Button::new("Undo"))
                .clicked()
            {
                action = Some(MenuAction::Undo);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Redo"))
                .clicked()
            {
                action = Some(MenuAction::Redo);
            }
        });
        ui.menu_button("View", |ui| {
            if ui
                .add_enabled(editor_open, egui::Button::new("Zoom In"))
                .clicked()
            {
                action = Some(MenuAction::ZoomIn);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Zoom Out"))
                .clicked()
            {
                action = Some(MenuAction::ZoomOut);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Fit to Window"))
                .clicked()
            {
                action = Some(MenuAction::FitToWindow);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Grid"))
                .clicked()
            {
                action = Some(MenuAction::ToggleGrid);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Rulers"))
                .clicked()
            {
                action = Some(MenuAction::ToggleRulers);
            }
            if ui.button("Full Screen").clicked() {
                action = Some(MenuAction::FullScreen);
            }
        });
        ui.menu_button("Help", |ui| {
            if ui.button("Settings…").clicked() {
                action = Some(MenuAction::Settings);
            }
            if ui.button("About Canvas Desktop").clicked() {
                action = Some(MenuAction::About);
            }
        });
    });
    action
}
