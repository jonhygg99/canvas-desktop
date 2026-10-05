//! Fallback sin menú nativo (macOS/Linux): una barra de menús egui con las
//! mismas acciones que el menú de Windows. Cada menú (File/Edit/View/Help)
//! vive en su propio helper; `menu_bar_ui` solo orquesta.

use std::path::PathBuf;

use super::MenuAction;

/// Fallback sin menú nativo (macOS/Linux): la app pinta una barra de menús
/// egui con `menu_bar_ui`. Solo existe fuera de Windows (en Windows el menú
/// nativo es `native::AppMenus`); el struct se retira del cfg para no
/// compilar código muerto en Windows.
#[cfg(not(windows))]
pub struct AppMenus;

#[cfg(not(windows))]
impl AppMenus {
    #[allow(dead_code)] // solo se llama en Windows (muda); aquí existe para
                        // reflejar la misma API pública.
    pub fn install(_hwnd: isize) -> Option<Self> {
        None
    }
    pub fn poll(&self) -> Option<MenuAction> {
        None
    }
    pub fn set_editor_enabled(&mut self, _enabled: bool) {}
    pub fn set_undo_redo(&mut self, _can_undo: bool, _can_redo: bool) {}
    pub fn set_recents(&mut self, _recents: &[PathBuf]) {}
    pub fn set_language(&mut self, _language: crate::i18n::Language) {}
}

/// Menú File: proyecto/archivo + guardar/exportar + salir.
fn file_menu_ui(
    ui: &mut eframe::egui::Ui,
    editor_open: bool,
    recents: &[PathBuf],
    action: &mut Option<MenuAction>,
) {
    use eframe::egui;
    ui.menu_button(crate::i18n::tr("File"), |ui| {
        if ui.button(crate::i18n::tr("New Window")).clicked() {
            *action = Some(MenuAction::NewWindow);
        }
        if ui.button(crate::i18n::tr("New Design")).clicked() {
            *action = Some(MenuAction::NewDesign);
        }
        if ui.button(crate::i18n::tr("Open…")).clicked() {
            *action = Some(MenuAction::OpenFile);
        }
        if ui.button(crate::i18n::tr("Open Folder…")).clicked() {
            *action = Some(MenuAction::OpenFolder);
        }
        ui.menu_button(crate::i18n::tr("Open Recent"), |ui| {
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
                    *action = Some(MenuAction::OpenRecent(path.clone()));
                }
            }
            if !shown {
                ui.add_enabled(
                    false,
                    egui::Button::new(crate::i18n::tr("No recent folders")),
                );
            }
        });
        if ui.button(crate::i18n::tr("Close Project")).clicked() {
            *action = Some(MenuAction::CloseProject);
        }
        ui.separator();
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Save")))
            .clicked()
        {
            *action = Some(MenuAction::Save);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Save As…")))
            .clicked()
        {
            *action = Some(MenuAction::SaveAs);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Save All")))
            .clicked()
        {
            *action = Some(MenuAction::SaveAll);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Export…")))
            .clicked()
        {
            *action = Some(MenuAction::Export);
        }
        ui.separator();
        if ui.button(crate::i18n::tr("Quit")).clicked() {
            *action = Some(MenuAction::Quit);
        }
    });
}

/// Menú Edit: deshacer/rehacer + portapapeles + agrupación.
fn edit_menu_ui(
    ui: &mut eframe::egui::Ui,
    editor_open: bool,
    can_undo: bool,
    can_redo: bool,
    action: &mut Option<MenuAction>,
) {
    use eframe::egui;
    ui.menu_button(crate::i18n::tr("Edit"), |ui| {
        if ui
            .add_enabled(
                editor_open && can_undo,
                egui::Button::new(crate::i18n::tr("Undo")),
            )
            .clicked()
        {
            *action = Some(MenuAction::Undo);
        }
        if ui
            .add_enabled(
                editor_open && can_redo,
                egui::Button::new(crate::i18n::tr("Redo")),
            )
            .clicked()
        {
            *action = Some(MenuAction::Redo);
        }
        ui.separator();
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Cut")))
            .clicked()
        {
            *action = Some(MenuAction::Cut);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Copy")))
            .clicked()
        {
            *action = Some(MenuAction::Copy);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Paste")))
            .clicked()
        {
            *action = Some(MenuAction::Paste);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Duplicate")))
            .clicked()
        {
            *action = Some(MenuAction::Duplicate);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Delete")))
            .clicked()
        {
            *action = Some(MenuAction::Delete);
        }
        ui.separator();
        if ui
            .add_enabled(
                editor_open,
                egui::Button::new(crate::i18n::tr("Select All")),
            )
            .clicked()
        {
            *action = Some(MenuAction::SelectAll);
        }
        ui.separator();
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Group")))
            .clicked()
        {
            *action = Some(MenuAction::Group);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Ungroup")))
            .clicked()
        {
            *action = Some(MenuAction::Ungroup);
        }
    });
}

/// Menú View: zoom, ajuste, cuadrícula/reglas, navegación de lienzos y paneles.
fn view_menu_ui(ui: &mut eframe::egui::Ui, editor_open: bool, action: &mut Option<MenuAction>) {
    use eframe::egui;
    ui.menu_button(crate::i18n::tr("View"), |ui| {
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Zoom In")))
            .clicked()
        {
            *action = Some(MenuAction::ZoomIn);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Zoom Out")))
            .clicked()
        {
            *action = Some(MenuAction::ZoomOut);
        }
        if ui
            .add_enabled(
                editor_open,
                egui::Button::new(crate::i18n::tr("Fit to Window")),
            )
            .clicked()
        {
            *action = Some(MenuAction::FitToWindow);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Grid")))
            .clicked()
        {
            *action = Some(MenuAction::ToggleGrid);
        }
        if ui
            .add_enabled(editor_open, egui::Button::new(crate::i18n::tr("Rulers")))
            .clicked()
        {
            *action = Some(MenuAction::ToggleRulers);
        }
        ui.separator();
        if ui
            .add_enabled(
                editor_open,
                egui::Button::new(crate::i18n::tr("Previous Canvas")),
            )
            .clicked()
        {
            *action = Some(MenuAction::PrevCanvas);
        }
        if ui
            .add_enabled(
                editor_open,
                egui::Button::new(crate::i18n::tr("Next Canvas")),
            )
            .clicked()
        {
            *action = Some(MenuAction::NextCanvas);
        }
        if ui
            .add_enabled(
                editor_open,
                egui::Button::new(crate::i18n::tr("Canvases Panel")),
            )
            .clicked()
        {
            *action = Some(MenuAction::ToggleCanvasesPanel);
        }
        if ui
            .add_enabled(
                editor_open,
                egui::Button::new(crate::i18n::tr("Canvases Axis")),
            )
            .clicked()
        {
            *action = Some(MenuAction::ToggleCanvasesAxis);
        }
        if ui
            .add_enabled(
                editor_open,
                egui::Button::new(crate::i18n::tr("Canvases Panel Side")),
            )
            .clicked()
        {
            *action = Some(MenuAction::CycleCanvasesSide);
        }
        if ui
            .add_enabled(
                editor_open,
                egui::Button::new(crate::i18n::tr("Layers Panel")),
            )
            .clicked()
        {
            *action = Some(MenuAction::ToggleLayersPanel);
        }
        if ui
            .add_enabled(
                editor_open,
                egui::Button::new(crate::i18n::tr("Add Canvas")),
            )
            .clicked()
        {
            *action = Some(MenuAction::AddCanvas);
        }
        ui.separator();
        if ui.button(crate::i18n::tr("Full Screen")).clicked() {
            *action = Some(MenuAction::FullScreen);
        }
    });
}

/// Menú Help: ajustes y acerca de.
fn help_menu_ui(ui: &mut eframe::egui::Ui, action: &mut Option<MenuAction>) {
    ui.menu_button(crate::i18n::tr("Help"), |ui| {
        if ui.button(crate::i18n::tr("Settings…")).clicked() {
            *action = Some(MenuAction::Settings);
        }
        if ui.button(crate::i18n::tr("About Canvas Desktop")).clicked() {
            *action = Some(MenuAction::About);
        }
    });
}

/// Barra de menús egui con las mismas acciones (fallback no-Windows).
/// Orquestador: delega cada menú a su helper en orden File/Edit/View/Help.
pub fn menu_bar_ui(
    ui: &mut eframe::egui::Ui,
    editor_open: bool,
    can_undo: bool,
    can_redo: bool,
    recents: &[PathBuf],
) -> Option<MenuAction> {
    let mut action = None;
    egui::MenuBar::new().ui(ui, |ui| {
        file_menu_ui(ui, editor_open, recents, &mut action);
        edit_menu_ui(ui, editor_open, can_undo, can_redo, &mut action);
        view_menu_ui(ui, editor_open, &mut action);
        help_menu_ui(ui, &mut action);
    });
    action
}

use eframe::egui;
