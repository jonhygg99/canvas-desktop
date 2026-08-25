//! Menus nativos de Windows: `muda` enganchado al HWND de la ventana
//! (`Menu::init_for_hwnd`), sondeado cada frame con
//! `MenuEvent::receiver().try_recv()`.

use std::path::PathBuf;

use muda::{Menu, MenuEvent, MenuItem, Submenu};

use super::MenuAction;

mod build;

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
            "new_window" => Some(MenuAction::NewWindow),
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
            "layers_panel" => Some(MenuAction::ToggleLayersPanel),
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
