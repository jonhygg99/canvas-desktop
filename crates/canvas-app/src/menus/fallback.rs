//! Fallback sin menu nativo (macOS/Linux hasta sus fases): una barra de menus
//! egui con las mismas acciones que el menu de Windows.

use std::path::PathBuf;

use super::MenuAction;

/// Fallback sin menú nativo (macOS/Linux hasta sus fases): la app pinta una
/// barra de menús egui con `menu_bar_ui`.
pub struct AppMenus;

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
