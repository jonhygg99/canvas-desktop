//! Galería de carpeta: cuadrícula de miniaturas con nombre. Las miniaturas
//! llegan por mensajes desde los hilos de trabajo; aquí solo se pintan.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use eframe::egui;

use crate::settings::GallerySort;

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

pub struct GalleryState {
    pub folder: PathBuf,
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

impl GalleryState {
    pub fn new(folder: PathBuf, sort: GallerySort) -> Self {
        Self {
            folder,
            items: Vec::new(),
            scanned: false,
            sort,
            selected: None,
            rename_edit: None,
            op_error: None,
        }
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

const CELL: egui::Vec2 = egui::vec2(172.0, 200.0);
const THUMB: f32 = 156.0;

pub fn show(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;

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
        ui.weak(
            "Right-click a tile for Duplicate · Ctrl+C / Ctrl+V to copy designs between folders",
        );
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
            ui.horizontal_wrapped(|ui| {
                for item in &state.items {
                    if let Some(cell_action) =
                        gallery_cell(ui, item, &mut state.selected, &mut state.rename_edit)
                    {
                        action = Some(cell_action);
                    }
                }
            });
        });
    });
    action
}

fn gallery_cell(
    ui: &mut egui::Ui,
    item: &GalleryItem,
    selected: &mut Option<PathBuf>,
    rename_edit: &mut Option<(PathBuf, String)>,
) -> Option<GalleryAction> {
    let (rect, response) = ui.allocate_exact_size(CELL, egui::Sense::click());
    let is_selected = selected.as_deref() == Some(item.path.as_path());
    let renaming = rename_edit.as_ref().is_some_and(|(p, _)| p == &item.path);
    // Posición del nombre bajo la miniatura: la necesita tanto el pintado
    // (fuera del scroll no se pinta) como el campo de renombrado (que si
    // hace falta se coloca aunque la celda esté al borde del scroll).
    let name_pos = egui::pos2(rect.center().x, rect.bottom() - 18.0);
    // Fuera del scroll: no pintamos nada (la cuadrícula sigue fluida aunque
    // haya cientos de ítems), pero SÍ devolvemos `response` — un menú
    // contextual enganchado a ella no debe morir al hacer scroll.
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        if response.hovered() {
            painter.rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if is_selected {
            painter.rect_stroke(
                rect,
                6.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 122, 255)),
                egui::StrokeKind::Inside,
            );
        }

        let thumb_rect = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 8.0 + THUMB / 2.0),
            egui::Vec2::splat(THUMB),
        );
        match (&item.tex, item.failed) {
            (Some(tex), _) => {
                // Encaja la miniatura en su celda conservando la proporción.
                let size = tex.size_vec2();
                let scale = (THUMB / size.x).min(THUMB / size.y).min(1.0);
                let fitted = egui::Rect::from_center_size(thumb_rect.center(), size * scale);
                painter.image(
                    tex.id(),
                    fitted,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            // Un diseño sin miniatura (recién creado, o ilegible) se marca
            // con el icono de documento, no el ⚠ alarmante de una imagen
            // que no cargó: no es un error del usuario.
            (None, _) if item.kind == ItemKind::Design => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "🖹",
                    egui::FontId::proportional(28.0),
                    ui.visuals().weak_text_color(),
                );
            }
            (None, true) => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "⚠",
                    egui::FontId::proportional(28.0),
                    ui.visuals().error_fg_color,
                );
            }
            (None, false) => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "⏳",
                    egui::FontId::proportional(24.0),
                    ui.visuals().weak_text_color(),
                );
            }
        }

        // Distintivo de esquina: un diseño se lee distinto de una foto
        // incluso cuando su miniatura ya cargó.
        if item.kind == ItemKind::Design {
            painter.text(
                thumb_rect.right_top() + egui::vec2(-2.0, 2.0),
                egui::Align2::RIGHT_TOP,
                "🖹",
                egui::FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
        }

        // Nombre truncado bajo la miniatura — o el campo de renombrado si
        // esta celda está en edición.
        if !renaming {
            let font = egui::FontId::proportional(12.5);
            let mut name = item.name.clone();
            if name.chars().count() > 24 {
                name = format!("{}…", name.chars().take(23).collect::<String>());
            }
            painter.text(
                name_pos,
                egui::Align2::CENTER_CENTER,
                name,
                font,
                ui.visuals().text_color(),
            );
        }
    }

    let mut action = None;

    if renaming {
        // Mismo patrón que `layers_panel.rs::rename_edit_ui`: Escape
        // cancela, perder el foco confirma (comprobado antes que
        // `lost_focus` para que el propio Escape no dispare un commit).
        let text_id = egui::Id::new(("gallery_rename", item.path.clone()));
        let name_rect = egui::Rect::from_center_size(name_pos, egui::vec2(CELL.x - 8.0, 20.0));
        let mut cancel = false;
        let mut commit = false;
        if let Some((_, text)) = rename_edit.as_mut() {
            let resp = ui.put(
                name_rect,
                egui::TextEdit::singleline(text)
                    .id(text_id)
                    .horizontal_align(egui::Align::Center),
            );
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            } else if resp.lost_focus() {
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
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !new_stem.is_empty() && new_stem != original_stem {
                    action = Some(GalleryAction::Rename(path, new_stem));
                }
            }
        }
    } else {
        action = response
            .clicked()
            .then(|| GalleryAction::Open(item.path.clone()));

        if response.secondary_clicked() {
            *selected = Some(item.path.clone());
        }
        response.context_menu(|ui| {
            if ui.button("Open").clicked() {
                action = Some(GalleryAction::Open(item.path.clone()));
                ui.close();
            }
            if ui.button("Rename").clicked() {
                let stem = item
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                *rename_edit = Some((item.path.clone(), stem));
                ui.memory_mut(|m| {
                    m.request_focus(egui::Id::new(("gallery_rename", item.path.clone())))
                });
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
            // Va a la Papelera de reciclaje (`trash::delete`), no borrado
            // permanente: recuperable si el usuario se equivoca, así que
            // no hace falta pedir confirmación aparte.
            let delete_label = egui::RichText::new("Delete").color(ui.visuals().warn_fg_color);
            if ui.button(delete_label).clicked() {
                action = Some(GalleryAction::Delete(item.path.clone()));
                ui.close();
            }
        });
    }

    action
}
