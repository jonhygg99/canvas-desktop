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
    CloseProject,
    Save,
    SaveAs,
    /// Guarda todas las ranuras sucias de la baraja, no solo la activa.
    SaveAll,
    Export,
    OpenRecent(PathBuf),
    Quit,
    Undo,
    Redo,
    ZoomIn,
    ZoomOut,
    FitToWindow,
    ToggleGrid,
    ToggleRulers,
    /// Salta al siguiente/anterior lienzo de la baraja (equivale a
    /// `PageDown`/`PageUp`); no hace nada con un solo archivo abierto.
    NextCanvas,
    PrevCanvas,
    /// Muestra/oculta la tira lateral de miniaturas de la baraja.
    ToggleCanvasesPanel,
    /// Alterna el eje de apilado de la baraja (vertical/horizontal).
    ToggleCanvasesAxis,
    /// Mueve la tira de la baraja al siguiente lado de la ventana.
    CycleCanvasesSide,
    /// Añade un lienzo en blanco al final de la baraja (celda "+" de la
    /// tira, o aquí cuando la tira está oculta con un solo archivo).
    AddCanvas,
    FullScreen,
    Settings,
    About,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    SelectAll,
    Group,
    Ungroup,
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
        /// Guardados aparte (además de en `editor_items`) para poder
        /// habilitarlos/deshabilitarlos según el estado real del historial,
        /// no solo según si hay editor abierto.
        undo_item: MenuItem,
        redo_item: MenuItem,
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
            let ctrl_alt = Modifiers::CONTROL | Modifiers::ALT;

            let new_item = MenuItem::with_id("new", "New Design", true, accel(ctrl, Code::KeyN));
            let open_item = MenuItem::with_id("open", "Open…", true, accel(ctrl, Code::KeyO));
            let open_folder_item = MenuItem::with_id(
                "open_folder",
                "Open Folder…",
                true,
                accel(ctrl_shift, Code::KeyO),
            );
            let close_project_item =
                MenuItem::with_id("close_project", "Close Project", true, None);
            let save_item = MenuItem::with_id("save", "Save", false, accel(ctrl, Code::KeyS));
            let save_as_item =
                MenuItem::with_id("save_as", "Save As…", false, accel(ctrl_shift, Code::KeyS));
            let save_all_item =
                MenuItem::with_id("save_all", "Save All", false, accel(ctrl_alt, Code::KeyS));
            let export_item =
                MenuItem::with_id("export", "Export…", false, accel(ctrl_shift, Code::KeyE));
            let recent_menu = Submenu::with_id("recent", "Open Recent", true);
            let quit_item = MenuItem::with_id("quit", "Quit", true, accel(ctrl, Code::KeyQ));

            let file = Submenu::with_items(
                "&File",
                true,
                &[
                    &new_item,
                    &open_item,
                    &open_folder_item,
                    &recent_menu,
                    &close_project_item,
                    &PredefinedMenuItem::separator(),
                    &save_item,
                    &save_as_item,
                    &save_all_item,
                    &export_item,
                    &PredefinedMenuItem::separator(),
                    &quit_item,
                ],
            )?;

            let undo_item = MenuItem::with_id("undo", "Undo", false, accel(ctrl, Code::KeyZ));
            let redo_item = MenuItem::with_id("redo", "Redo", false, accel(ctrl, Code::KeyY));
            let cut_item = MenuItem::with_id("cut", "Cut", false, accel(ctrl, Code::KeyX));
            let copy_item = MenuItem::with_id("copy", "Copy", false, accel(ctrl, Code::KeyC));
            let paste_item = MenuItem::with_id("paste", "Paste", false, accel(ctrl, Code::KeyV));
            let duplicate_item =
                MenuItem::with_id("duplicate", "Duplicate", false, accel(ctrl, Code::KeyD));
            let delete_item = MenuItem::with_id("delete", "Delete", false, None);
            let select_all_item =
                MenuItem::with_id("select_all", "Select All", false, accel(ctrl, Code::KeyA));
            let group_item = MenuItem::with_id("group", "Group", false, accel(ctrl, Code::KeyG));
            let ungroup_item =
                MenuItem::with_id("ungroup", "Ungroup", false, accel(ctrl_shift, Code::KeyG));

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
                    &PredefinedMenuItem::separator(),
                    &group_item,
                    &ungroup_item,
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
            let next_canvas_item = MenuItem::with_id("next_canvas", "Next Canvas", false, None);
            let prev_canvas_item = MenuItem::with_id("prev_canvas", "Previous Canvas", false, None);
            let canvases_panel_item =
                MenuItem::with_id("canvases_panel", "Canvases Panel", false, None);
            let canvases_axis_item =
                MenuItem::with_id("canvases_axis", "Canvases Axis", false, None);
            let canvases_side_item =
                MenuItem::with_id("canvases_side", "Canvases Panel Side", false, None);
            let add_canvas_item = MenuItem::with_id("add_canvas", "Add Canvas", false, None);
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
                    &prev_canvas_item,
                    &next_canvas_item,
                    &canvases_panel_item,
                    &canvases_axis_item,
                    &canvases_side_item,
                    &add_canvas_item,
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
                save_all_item,
                export_item,
                undo_item.clone(),
                redo_item.clone(),
                zoom_in_item,
                zoom_out_item,
                fit_item,
                grid_item,
                rulers_item,
                next_canvas_item,
                prev_canvas_item,
                canvases_panel_item,
                canvases_axis_item,
                canvases_side_item,
                add_canvas_item,
                cut_item,
                copy_item,
                paste_item,
                duplicate_item,
                delete_item,
                select_all_item,
                group_item,
                ungroup_item,
            ];

            Ok(Self {
                _menu: menu,
                recent_menu,
                recent_items: Vec::new(),
                editor_items,
                editor_enabled: false,
                undo_item,
                redo_item,
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
                "close_project" => Some(MenuAction::CloseProject),
                "save" => Some(MenuAction::Save),
                "save_as" => Some(MenuAction::SaveAs),
                "save_all" => Some(MenuAction::SaveAll),
                "export" => Some(MenuAction::Export),
                "quit" => Some(MenuAction::Quit),
                "undo" => Some(MenuAction::Undo),
                "redo" => Some(MenuAction::Redo),
                "zoom_in" => Some(MenuAction::ZoomIn),
                "zoom_out" => Some(MenuAction::ZoomOut),
                "fit" => Some(MenuAction::FitToWindow),
                "grid" => Some(MenuAction::ToggleGrid),
                "rulers" => Some(MenuAction::ToggleRulers),
                "next_canvas" => Some(MenuAction::NextCanvas),
                "prev_canvas" => Some(MenuAction::PrevCanvas),
                "canvases_panel" => Some(MenuAction::ToggleCanvasesPanel),
                "canvases_axis" => Some(MenuAction::ToggleCanvasesAxis),
                "canvases_side" => Some(MenuAction::CycleCanvasesSide),
                "add_canvas" => Some(MenuAction::AddCanvas),
                "full_screen" => Some(MenuAction::FullScreen),
                "cut" => Some(MenuAction::Cut),
                "copy" => Some(MenuAction::Copy),
                "paste" => Some(MenuAction::Paste),
                "duplicate" => Some(MenuAction::Duplicate),
                "delete" => Some(MenuAction::Delete),
                "select_all" => Some(MenuAction::SelectAll),
                "group" => Some(MenuAction::Group),
                "ungroup" => Some(MenuAction::Ungroup),
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

        /// Habilita/deshabilita Undo/Redo según lo que de verdad haya en el
        /// historial del editor activo (llamado cada vez que cambia, no solo
        /// al abrir/cerrar el editor). `editor_enabled` sigue ganando: sin
        /// editor abierto ambos quedan deshabilitados pase lo que pase aquí.
        pub fn set_undo_redo(&mut self, can_undo: bool, can_redo: bool) {
            self.undo_item.set_enabled(self.editor_enabled && can_undo);
            self.redo_item.set_enabled(self.editor_enabled && can_redo);
        }

        /// Reconstruye el submenú «Open Recent».
        pub fn set_recents(&mut self, recents: &[PathBuf]) {
            for (item, _) in self.recent_items.drain(..) {
                let _ = self.recent_menu.remove(&item);
            }
            let mut idx = 0;
            for path in recents {
                if !path.is_dir() {
                    continue;
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let item = MenuItem::with_id(format!("recent_{idx}"), name, true, None);
                if self.recent_menu.append(&item).is_ok() {
                    self.recent_items.push((item, path.clone()));
                }
                idx += 1;
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
    pub fn set_undo_redo(&mut self, _can_undo: bool, _can_redo: bool) {}
    pub fn set_recents(&mut self, _recents: &[PathBuf]) {}
}

/// Barra de menús egui con las mismas acciones (fallback no-Windows).
#[cfg(not(windows))]
pub fn menu_bar_ui(
    ui: &mut eframe::egui::Ui,
    editor_open: bool,
    can_undo: bool,
    can_redo: bool,
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
            ui.menu_button("Open Recent", |ui| {
                let mut shown = false;
                for path in recents {
                    if !path.is_dir() {
                        continue;
                    }
                    shown = true;
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    if ui.button(name).clicked() {
                        action = Some(MenuAction::OpenRecent(path.clone()));
                    }
                }
                if !shown {
                    ui.add_enabled(false, egui::Button::new("No recent folders"));
                }
            });
            if ui.button("Close Project").clicked() {
                action = Some(MenuAction::CloseProject);
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
            if ui
                .add_enabled(editor_open, egui::Button::new("Save All"))
                .clicked()
            {
                action = Some(MenuAction::SaveAll);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Export…"))
                .clicked()
            {
                action = Some(MenuAction::Export);
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                action = Some(MenuAction::Quit);
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui
                .add_enabled(editor_open && can_undo, egui::Button::new("Undo"))
                .clicked()
            {
                action = Some(MenuAction::Undo);
            }
            if ui
                .add_enabled(editor_open && can_redo, egui::Button::new("Redo"))
                .clicked()
            {
                action = Some(MenuAction::Redo);
            }
            ui.separator();
            if ui
                .add_enabled(editor_open, egui::Button::new("Cut"))
                .clicked()
            {
                action = Some(MenuAction::Cut);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Copy"))
                .clicked()
            {
                action = Some(MenuAction::Copy);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Paste"))
                .clicked()
            {
                action = Some(MenuAction::Paste);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Duplicate"))
                .clicked()
            {
                action = Some(MenuAction::Duplicate);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Delete"))
                .clicked()
            {
                action = Some(MenuAction::Delete);
            }
            ui.separator();
            if ui
                .add_enabled(editor_open, egui::Button::new("Select All"))
                .clicked()
            {
                action = Some(MenuAction::SelectAll);
            }
            ui.separator();
            if ui
                .add_enabled(editor_open, egui::Button::new("Group"))
                .clicked()
            {
                action = Some(MenuAction::Group);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Ungroup"))
                .clicked()
            {
                action = Some(MenuAction::Ungroup);
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
            ui.separator();
            if ui
                .add_enabled(editor_open, egui::Button::new("Previous Canvas"))
                .clicked()
            {
                action = Some(MenuAction::PrevCanvas);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Next Canvas"))
                .clicked()
            {
                action = Some(MenuAction::NextCanvas);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Canvases Panel"))
                .clicked()
            {
                action = Some(MenuAction::ToggleCanvasesPanel);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Canvases Axis"))
                .clicked()
            {
                action = Some(MenuAction::ToggleCanvasesAxis);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Canvases Panel Side"))
                .clicked()
            {
                action = Some(MenuAction::CycleCanvasesSide);
            }
            if ui
                .add_enabled(editor_open, egui::Button::new("Add Canvas"))
                .clicked()
            {
                action = Some(MenuAction::AddCanvas);
            }
            ui.separator();
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
