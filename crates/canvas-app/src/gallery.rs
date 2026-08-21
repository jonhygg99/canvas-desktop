//! Galería de carpeta: cuadrícula de miniaturas con nombre. Las miniaturas
//! llegan por mensajes desde los hilos de trabajo; aquí solo se pintan.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use eframe::egui;

use crate::{deck::StripSide, settings::GallerySort};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Image,
    Design,
}

pub struct GalleryItem {
    pub path: PathBuf,
    pub name: String,
    pub mtime: Option<SystemTime>,
    pub kind: ItemKind,
    pub tex: Option<egui::TextureHandle>,
    pub failed: bool,
}

fn sibling_folders(folder: &Path) -> Vec<PathBuf> {
    let Some(parent) = folder.parent() else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    folders.sort_by(|a, b| {
        crate::settings::natural_cmp(
            &a.file_name().unwrap_or_default().to_string_lossy(),
            &b.file_name().unwrap_or_default().to_string_lossy(),
        )
    });
    folders
}
fn child_folders(folder: &Path) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    folders.sort_by(|a, b| {
        crate::settings::natural_cmp(
            &a.file_name().unwrap_or_default().to_string_lossy(),
            &b.file_name().unwrap_or_default().to_string_lossy(),
        )
    });
    folders
}
struct FolderLists {
    siblings: Vec<PathBuf>,
    children: Vec<PathBuf>,
}
pub struct GalleryState {
    pub folder: PathBuf,
    pub folder_panel_side: StripSide,
    navigation: FolderNavigation,
    folders: Box<FolderLists>,
    pub items: Vec<GalleryItem>,
    pub scanned: bool,
    pub sort: GallerySort,
    /// Última celda marcada con clic derecho: lo que copia Ctrl+C.
    pub selected: Option<PathBuf>,
    /// Renombrado en curso: ruta y texto editable (solo el nombre base, sin
    /// extensión — cambiarla rompería la detección de imagen/diseño).
    pub rename_edit: Option<(PathBuf, String)>,
    /// Último fallo de una operación de archivos (crear/duplicar/pegar/
    /// renombrar/borrar), visible hasta que el usuario lo descarta.
    pub op_error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct FolderNavigation {
    history: Vec<PathBuf>,
    current: usize,
}
impl FolderNavigation {
    fn new(folder: PathBuf) -> Self {
        Self {
            history: vec![folder],
            current: 0,
        }
    }
    fn push(&mut self, folder: PathBuf) {
        if self.history.get(self.current) != Some(&folder) {
            self.history.truncate(self.current + 1);
            self.history.push(folder);
            self.current = self.history.len() - 1;
        }
    }
    fn back(&mut self) -> Option<PathBuf> {
        self.current.checked_sub(1).map(|current| {
            self.current = current;
            self.history[current].clone()
        })
    }
    fn forward(&mut self) -> Option<PathBuf> {
        (self.current + 1 < self.history.len()).then(|| {
            self.current += 1;
            self.history[self.current].clone()
        })
    }
    pub fn can_back(&self) -> bool {
        self.current > 0
    }
    pub fn can_forward(&self) -> bool {
        self.current + 1 < self.history.len()
    }
}
impl GalleryState {
    pub fn new(folder: PathBuf, sort: GallerySort, folder_panel_side: StripSide) -> Self {
        Self {
            folder_panel_side,
            navigation: FolderNavigation::new(folder.clone()),
            folders: Box::new(FolderLists {
                siblings: sibling_folders(&folder),
                children: child_folders(&folder),
            }),
            folder,
            items: Vec::new(),
            scanned: false,
            sort,
            selected: None,
            rename_edit: None,
            op_error: None,
        }
    }

    pub fn with_navigation(
        folder: PathBuf,
        sort: GallerySort,
        navigation: FolderNavigation,
        folder_panel_side: StripSide,
    ) -> Self {
        Self {
            folder: folder.clone(),
            folder_panel_side,
            navigation,
            folders: Box::new(FolderLists {
                siblings: sibling_folders(&folder),
                children: child_folders(&folder),
            }),
            items: Vec::new(),
            scanned: false,
            sort,
            selected: None,
            rename_edit: None,
            op_error: None,
        }
    }
    pub fn navigation_to_folder(&mut self, folder: PathBuf) -> (PathBuf, FolderNavigation) {
        self.navigation.push(folder.clone());
        (folder, self.navigation.clone())
    }
    pub fn navigation_back(&mut self) -> Option<(PathBuf, FolderNavigation)> {
        self.navigation
            .back()
            .map(|folder| (folder, self.navigation.clone()))
    }
    pub fn navigation_forward(&mut self) -> Option<(PathBuf, FolderNavigation)> {
        self.navigation
            .forward()
            .map(|folder| (folder, self.navigation.clone()))
    }
    /// Sustituye la lista de archivos conservando las miniaturas ya
    /// cargadas (por ruta) y descartando los ítems que hayan desaparecido
    /// del disco: un rescaneo tras crear/duplicar/pegar no hace parpadear
    /// toda la cuadrícula de vuelta a ⏳.
    pub fn merge_files(&mut self, files: Vec<(PathBuf, Option<SystemTime>)>) {
        let mut old: HashMap<PathBuf, (Option<egui::TextureHandle>, bool)> = self
            .items
            .drain(..)
            .map(|i| (i.path, (i.tex, i.failed)))
            .collect();
        self.items = files
            .into_iter()
            .map(|(path, mtime)| {
                let (tex, failed) = old.remove(&path).unwrap_or((None, false));
                let kind = if canvas_io::is_canvas_file(&path) {
                    ItemKind::Design
                } else {
                    ItemKind::Image
                };
                GalleryItem {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    path,
                    mtime,
                    kind,
                    tex,
                    failed,
                }
            })
            .collect();
        self.scanned = true;
        self.apply_sort();
    }

    /// Reordena en memoria (sin reescanear el disco).
    pub fn apply_sort(&mut self) {
        match self.sort {
            // Natural/numérico — ver la doc de `natural_cmp` (settings.rs).
            // Mismo criterio que `Deck::apply_sort`, para que la rejilla y
            // la tira de la baraja siempre coincidan en el orden.
            GallerySort::Name => self
                .items
                .sort_by(|a, b| crate::settings::natural_cmp(&a.name, &b.name)),
            // Más recientes primero; sin fecha, al final.
            GallerySort::DateModified => self.items.sort_by(|a, b| {
                b.mtime
                    .cmp(&a.mtime)
                    .then_with(|| crate::settings::natural_cmp(&a.name, &b.name))
            }),
            // `Manual` es un estado de `Deck::sort` (flechas del panel del
            // lienzo, `deck.rs`): la galería nunca lo ofrece ni lo hereda, no
            // tiene un `order_hint` por ítem. Si llegara aquí (no debería),
            // cae al orden natural en vez de dejar la rejilla sin ordenar.
            GallerySort::Manual => self
                .items
                .sort_by(|a, b| crate::settings::natural_cmp(&a.name, &b.name)),
        }
    }

    /// Entrega una miniatura llegada de un hilo de trabajo (por ruta: el
    /// orden puede haber cambiado desde que se lanzó el escaneo).
    pub fn set_thumb(&mut self, path: &std::path::Path, tex: Option<egui::TextureHandle>) {
        if let Some(item) = self.items.iter_mut().find(|i| i.path == path) {
            match tex {
                Some(tex) => item.tex = Some(tex),
                None => item.failed = true,
            }
        }
    }
}

pub enum GalleryAction {
    Open(PathBuf),
    CycleFolderPanelSide,
    OpenFolder(PathBuf),
    Back,
    Forward,
    SortChanged(GallerySort),
    /// Botón «✚ New design» de la cabecera.
    NewDesign,
    /// Duplicar este archivo (y su sidecar, si es una imagen que tiene uno)
    /// dentro de la misma carpeta.
    Duplicate(PathBuf),
    /// Pegar en esta carpeta el archivo copiado (ruta de origen).
    PasteHere(PathBuf),
    /// Cambiar el nombre base de este archivo (ruta, nuevo nombre).
    Rename(PathBuf, String),
    /// Enviar este archivo a la Papelera de reciclaje. La confirmación ya
    /// ocurrió (diálogo nativo) antes de devolver esta acción.
    Delete(PathBuf),
}

/// Ruta copiada desde una galería. Ranura de proceso, como el portapapeles
/// de capas (`crate::clipboard`): sobrevive a cambiar de carpeta, que es
/// justo el caso de uso de copiar un diseño de una carpeta a otra. A
/// propósito NO se toca el portapapeles del SO: `arboard` no sabe escribir
/// `CF_HDROP` y machacaría el portapapeles de texto del usuario.
fn file_slot() -> &'static Mutex<Option<PathBuf>> {
    static SLOT: std::sync::OnceLock<Mutex<Option<PathBuf>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn copy_to_slot(path: PathBuf) {
    *file_slot().lock().unwrap() = Some(path);
}

fn slot_contents() -> Option<PathBuf> {
    file_slot().lock().unwrap().clone()
}

/// Abre el Explorador de Windows con `path` ya seleccionado. Mejor esfuerzo:
/// no hay nada sensato que hacer si falla, así que no se reporta.
#[cfg(windows)]
fn reveal_in_explorer(path: &Path) {
    if let Err(e) = std::process::Command::new("explorer")
        .arg("/select,")
        .arg(path)
        .spawn()
    {
        tracing::debug!("no se pudo abrir el Explorador en {}: {e}", path.display());
    }
}

#[cfg(not(windows))]
fn reveal_in_explorer(_path: &Path) {}

const MIN_CELL_WIDTH: f32 = 140.0;
const PREFERRED_CELL_WIDTH: f32 = 156.0;
const CELL_GAP: f32 = 8.0;
const ROW_GAP: f32 = 4.0;
const THUMB_INSET: f32 = 8.0;
const THUMB_ASPECT_RATIO: f32 = 16.0 / 9.0;
const TITLE_HEIGHT: f32 = 20.0;
const TITLE_TO_THUMB_GAP: f32 = 2.0;
const CARD_BOTTOM_PADDING: f32 = 6.0;

fn gallery_column_count(available_width: f32) -> usize {
    ((available_width + CELL_GAP) / (PREFERRED_CELL_WIDTH + CELL_GAP))
        .floor()
        .max(1.0) as usize
}

fn gallery_cell_size(available_width: f32, columns: usize) -> egui::Vec2 {
    let width = ((available_width - CELL_GAP * (columns.saturating_sub(1) as f32))
        / columns as f32)
        .max(MIN_CELL_WIDTH);
    let thumbnail_height = (width - THUMB_INSET * 2.0) / THUMB_ASPECT_RATIO;
    egui::vec2(
        width,
        TITLE_HEIGHT + TITLE_TO_THUMB_GAP + thumbnail_height + CARD_BOTTOM_PADDING,
    )
}

pub fn next_folder_panel_side(side: StripSide) -> StripSide {
    match side {
        StripSide::Left => StripSide::Bottom,
        StripSide::Bottom => StripSide::Right,
        StripSide::Right => StripSide::Top,
        StripSide::Top => StripSide::Left,
    }
}

fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
}

fn folder_button_list(
    ui: &mut egui::Ui,
    folders: &[PathBuf],
    current: Option<&Path>,
    action: &mut Option<GalleryAction>,
) {
    for folder in folders {
        let name = folder_name(folder);
        if current == Some(folder.as_path()) {
            ui.strong(name);
        } else if ui.button(name).clicked() {
            *action = Some(GalleryAction::OpenFolder(folder.clone()));
        }
    }
}

fn folder_panel_contents(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.strong("Folders");
        if ui
            .small_button("Change View")
            .on_hover_text(format!(
                "Change folders view to {}",
                next_folder_panel_side(state.folder_panel_side).label()
            ))
            .clicked()
        {
            action = Some(GalleryAction::CycleFolderPanelSide);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(state.navigation.can_back(), egui::Button::new("<"))
            .on_hover_text("Back to previous folder (Alt+Left)")
            .clicked()
        {
            action = Some(GalleryAction::Back);
        }
        if ui
            .add_enabled(state.navigation.can_forward(), egui::Button::new(">"))
            .on_hover_text("Forward to next folder (Alt+Right)")
            .clicked()
        {
            action = Some(GalleryAction::Forward);
        }
        if let Some(parent) = state.folder.parent() {
            if ui
                .small_button("Parent")
                .on_hover_text("Open parent folder (Alt+Up)")
                .clicked()
            {
                action = Some(GalleryAction::OpenFolder(parent.to_owned()));
            }
        }
    });
    ui.separator();

    let current_name = folder_name(&state.folder);
    if state.folder_panel_side.is_vertical_flow() {
        egui::ScrollArea::vertical()
            .id_salt("gallery_folder_tree_vertical")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.strong(format!("Inside {current_name}"));
                if state.folders.children.is_empty() {
                    ui.weak("No subfolders");
                } else {
                    folder_button_list(ui, &state.folders.children, None, &mut action);
                }
                ui.add_space(8.0);
                ui.separator();
                ui.strong("Siblings");
                folder_button_list(
                    ui,
                    &state.folders.siblings,
                    Some(state.folder.as_path()),
                    &mut action,
                );
            });
    } else {
        ui.strong(format!("Inside {current_name}"));
        egui::ScrollArea::horizontal()
            .id_salt("gallery_child_folders_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if state.folders.children.is_empty() {
                        ui.weak("No subfolders");
                    } else {
                        folder_button_list(ui, &state.folders.children, None, &mut action);
                    }
                });
            });
        ui.separator();
        ui.strong("Siblings");
        egui::ScrollArea::horizontal()
            .id_salt("gallery_sibling_folders_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    folder_button_list(
                        ui,
                        &state.folders.siblings,
                        Some(state.folder.as_path()),
                        &mut action,
                    );
                });
            });
    }
    action
}
fn show_folder_panel(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;
    match state.folder_panel_side {
        StripSide::Left => {
            egui::Panel::left("gallery_folders_left")
                .default_size(180.0)
                .size_range(140.0..=320.0)
                .resizable(true)
                .show(ui, |ui| action = folder_panel_contents(state, ui));
        }
        StripSide::Right => {
            egui::Panel::right("gallery_folders_right")
                .default_size(180.0)
                .size_range(140.0..=320.0)
                .resizable(true)
                .show(ui, |ui| action = folder_panel_contents(state, ui));
        }
        StripSide::Top => {
            egui::Panel::top("gallery_folders_top")
                .default_size(112.0)
                .size_range(78.0..=220.0)
                .resizable(true)
                .show(ui, |ui| action = folder_panel_contents(state, ui));
        }
        StripSide::Bottom => {
            egui::Panel::bottom("gallery_folders_bottom")
                .default_size(112.0)
                .size_range(78.0..=220.0)
                .resizable(true)
                .show(ui, |ui| action = folder_panel_contents(state, ui));
        }
    }
    action
}
pub fn show(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;

    if !ui.ctx().text_edit_focused() {
        let (back, forward, parent) = ui.ctx().input(|i| {
            (
                i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft),
                i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight),
                i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp),
            )
        });
        if back && state.navigation.can_back() {
            action = Some(GalleryAction::Back);
        } else if forward && state.navigation.can_forward() {
            action = Some(GalleryAction::Forward);
        } else if parent {
            if let Some(folder) = state.folder.parent() {
                action = Some(GalleryAction::OpenFolder(folder.to_owned()));
            }
        }
    }

    // Ctrl+C / Ctrl+V: copiar/pegar un diseño entre carpetas. winit no deja
    // pasar Ctrl+C/V como pulsaciones normales de tecla — los intercepta
    // para la integración con el portapapeles del SO y en su lugar egui los
    // entrega como `Event::Copy`/`Event::Paste(texto)`, así que
    // `consume_shortcut` nunca los ve; hay que mirar los eventos crudos.
    // Se ignoran mientras se edita texto (p. ej. un renombrado en curso)
    // para no robarle el atajo al campo — mismo guard que
    // `EditorState::handle_shortcuts`.
    let (want_copy, want_paste) = ui.ctx().input(|i| {
        let mut copy = false;
        let mut paste = false;
        for ev in &i.events {
            match ev {
                egui::Event::Copy => copy = true,
                egui::Event::Paste(_) => paste = true,
                _ => {}
            }
        }
        (copy, paste)
    });
    if !ui.ctx().text_edit_focused() {
        if want_copy {
            if let Some(path) = state.selected.clone() {
                copy_to_slot(path);
            }
        }
        if want_paste {
            if let Some(path) = slot_contents() {
                action = Some(GalleryAction::PasteHere(path));
            }
        }
    }

    if let Some(panel_action) = show_folder_panel(state, ui) {
        action = Some(panel_action);
    }

    egui::CentralPanel::default().show(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading(
                state
                    .folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| state.folder.display().to_string()),
            );
            ui.weak(format!("— {} items", state.items.len()));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut sort = state.sort;
                egui::ComboBox::from_id_salt("gallery_sort")
                    .selected_text(sort.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut sort,
                            GallerySort::Name,
                            GallerySort::Name.label(),
                        );
                        ui.selectable_value(
                            &mut sort,
                            GallerySort::DateModified,
                            GallerySort::DateModified.label(),
                        );
                    });
                ui.label("Sort by:");
                if sort != state.sort {
                    state.sort = sort;
                    state.apply_sort();
                    action = Some(GalleryAction::SortChanged(sort));
                }
                ui.add_space(12.0);
                if ui.button("✚ New design").clicked() {
                    action = Some(GalleryAction::NewDesign);
                }
            });
        });
        if let Some(error) = state.op_error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, &error);
                if ui.small_button("✕").clicked() {
                    state.op_error = None;
                }
            });
        }
        ui.add_space(4.0);
        ui.separator();

        if !state.scanned {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.add(egui::Spinner::new().size(28.0));
                ui.label("Scanning for images…");
            });
            return;
        }
        if state.items.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("This folder is empty.");
                ui.weak("Click ✚ New design to start one, or open an image.");
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            let columns = gallery_column_count(ui.available_width());
            let cell_size = gallery_cell_size(ui.available_width(), columns);
            ui.spacing_mut().item_spacing.y = ROW_GAP;
            for row in state.items.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = CELL_GAP;
                    for item in row {
                        if let Some(cell_action) = gallery_cell(
                            ui,
                            item,
                            cell_size,
                            &mut state.selected,
                            &mut state.rename_edit,
                        ) {
                            action = Some(cell_action);
                        }
                    }
                });
                ui.add_space(CELL_GAP);
            }
        });
    });
    action
}

fn begin_rename(
    item: &GalleryItem,
    rename_edit: &mut Option<(PathBuf, String)>,
    ctx: &egui::Context,
) {
    let stem = item
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    *rename_edit = Some((item.path.clone(), stem));
    ctx.memory_mut(|memory| {
        memory.request_focus(egui::Id::new(("gallery_rename", item.path.clone())))
    });
}

fn gallery_cell(
    ui: &mut egui::Ui,
    item: &GalleryItem,
    cell_size: egui::Vec2,
    selected: &mut Option<PathBuf>,
    rename_edit: &mut Option<(PathBuf, String)>,
) -> Option<GalleryAction> {
    let (rect, response) = ui.allocate_exact_size(cell_size, egui::Sense::click());
    let is_selected = selected.as_deref() == Some(item.path.as_path());
    let renaming = rename_edit
        .as_ref()
        .is_some_and(|(path, _)| path == &item.path);
    let name_rect = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(8.0, 4.0),
        egui::vec2(rect.width() - 16.0, TITLE_HEIGHT - 4.0),
    );
    let name_response = (!renaming).then(|| {
        ui.interact(
            name_rect,
            egui::Id::new(("gallery_name", item.path.clone())),
            egui::Sense::click(),
        )
    });

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        if is_selected {
            painter.rect_stroke(
                rect,
                6.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 122, 255)),
                egui::StrokeKind::Inside,
            );
        }

        let thumbnail_width = rect.width() - THUMB_INSET * 2.0;
        let thumbnail_height = thumbnail_width / THUMB_ASPECT_RATIO;
        let thumb_rect = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(THUMB_INSET, TITLE_HEIGHT + TITLE_TO_THUMB_GAP),
            egui::vec2(thumbnail_width, thumbnail_height),
        );
        match (&item.tex, item.failed) {
            (Some(tex), _) => {
                let size = tex.size_vec2();
                let scale = (thumbnail_width / size.x).max(thumbnail_height / size.y);
                let fitted = egui::Rect::from_center_size(thumb_rect.center(), size * scale);
                painter.image(
                    tex.id(),
                    fitted,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            (None, _) if item.kind == ItemKind::Design => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Document",
                    egui::FontId::proportional(14.0),
                    ui.visuals().weak_text_color(),
                );
            }
            (None, true) => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Failed to load",
                    egui::FontId::proportional(14.0),
                    ui.visuals().error_fg_color,
                );
            }
            (None, false) => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Loading",
                    egui::FontId::proportional(14.0),
                    ui.visuals().weak_text_color(),
                );
            }
        }

        if item.kind == ItemKind::Design {
            painter.text(
                thumb_rect.right_top() + egui::vec2(-2.0, 2.0),
                egui::Align2::RIGHT_TOP,
                "Design",
                egui::FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
        }

        if !renaming {
            let mut name = item.name.clone();
            if name.chars().count() > 30 {
                name = format!("{}...", name.chars().take(27).collect::<String>());
            }
            painter.text(
                name_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(12.5),
                ui.visuals().text_color(),
            );
        }
    }

    let mut action = None;
    let name_clicked = name_response.as_ref().is_some_and(|name| name.clicked());
    if let Some(name_response) = &name_response {
        if name_response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        if name_response.clicked() {
            begin_rename(item, rename_edit, ui.ctx());
        }
    }

    if renaming {
        let text_id = egui::Id::new(("gallery_rename", item.path.clone()));
        let mut cancel = false;
        let mut commit = false;
        if let Some((_, text)) = rename_edit.as_mut() {
            let edit_response = ui.put(
                name_rect,
                egui::TextEdit::singleline(text)
                    .id(text_id)
                    .horizontal_align(egui::Align::LEFT),
            );
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            } else if edit_response.lost_focus() {
                commit = true;
            }
        }
        if cancel {
            *rename_edit = None;
        } else if commit {
            if let Some((path, text)) = rename_edit.take() {
                let new_stem = text.trim().to_owned();
                let original_stem = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !new_stem.is_empty() && new_stem != original_stem {
                    action = Some(GalleryAction::Rename(path, new_stem));
                }
            }
        }
    } else {
        if !name_clicked && response.clicked() {
            action = Some(GalleryAction::Open(item.path.clone()));
        }
        if response.secondary_clicked() {
            *selected = Some(item.path.clone());
        }
        response.context_menu(|ui| {
            if ui.button("Open").clicked() {
                action = Some(GalleryAction::Open(item.path.clone()));
                ui.close();
            }
            if ui.button("Rename").clicked() {
                begin_rename(item, rename_edit, ui.ctx());
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                action = Some(GalleryAction::Duplicate(item.path.clone()));
                ui.close();
            }
            if ui.button("Copy").clicked() {
                *selected = Some(item.path.clone());
                copy_to_slot(item.path.clone());
                ui.close();
            }
            if ui.button("Reveal in Explorer").clicked() {
                reveal_in_explorer(&item.path);
                ui.close();
            }
            ui.separator();
            let delete_label = egui::RichText::new("Delete").color(ui.visuals().warn_fg_color);
            if ui.button(delete_label).clicked() {
                action = Some(GalleryAction::Delete(item.path.clone()));
                ui.close();
            }
        });
    }

    action
}
#[cfg(test)]
mod tests {
    use super::{
        gallery_cell_size, gallery_column_count, next_folder_panel_side, FolderNavigation,
    };
    use crate::deck::StripSide;
    use std::path::PathBuf;

    #[test]
    fn folder_panel_cycles_clockwise_from_left_to_bottom() {
        assert_eq!(next_folder_panel_side(StripSide::Left), StripSide::Bottom);
        assert_eq!(next_folder_panel_side(StripSide::Bottom), StripSide::Right);
        assert_eq!(next_folder_panel_side(StripSide::Right), StripSide::Top);
        assert_eq!(next_folder_panel_side(StripSide::Top), StripSide::Left);
    }

    #[test]
    fn responsive_grid_fills_the_available_width() {
        let columns = gallery_column_count(900.0);
        let size = gallery_cell_size(900.0, columns);
        assert_eq!(columns, 5);
        assert!((size.x - 173.6).abs() < f32::EPSILON);
        assert!(size.y < size.x);
    }
    #[test]
    fn folder_navigation_discards_forward_branch_after_new_visit() {
        let a = PathBuf::from("a");
        let b = PathBuf::from("b");
        let mut navigation = FolderNavigation::new(a.clone());
        navigation.push(b.clone());
        navigation.push(PathBuf::from("c"));
        assert_eq!(navigation.back(), Some(b.clone()));
        navigation.push(PathBuf::from("d"));
        assert!(!navigation.can_forward());
        assert_eq!(navigation.back(), Some(b.clone()));
        assert_eq!(navigation.back(), Some(a));
    }
}
