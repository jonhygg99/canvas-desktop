//! Galería de carpeta: estado (lista de archivos, navegación entre
//! carpetas, ranura de copiar/pegar). El renderizado egui vive en `ui`.

mod ui;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use eframe::egui;

use crate::{deck::StripSide, settings::GallerySort};

pub use ui::{next_folder_panel_side, show};

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
    /// Número de diseños que se muestran por línea (no cambia los archivos).
    pub gallery_columns: usize,
    /// Última celda marcada con clic derecho: lo que copia Ctrl+C.
    pub selected: Option<PathBuf>,
    /// Renombrado en curso: ruta y texto editable (solo el nombre base, sin
    /// extensión — cambiarla rompería la detección de imagen/diseño).
    pub rename_edit: Option<(PathBuf, String)>,
    pub new_folder_inside: Option<String>,
    pub new_folder_sibling: Option<String>,
    pub folder_rename_edit: Option<(PathBuf, String)>,
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
            gallery_columns: 5,
            selected: None,
            rename_edit: None,
            new_folder_inside: None,
            new_folder_sibling: None,
            folder_rename_edit: None,
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
            gallery_columns: 5,
            selected: None,
            rename_edit: None,
            new_folder_inside: None,
            new_folder_sibling: None,
            folder_rename_edit: None,
            op_error: None,
        }
    }
    /// Vuelve a sondear las carpetas (Inside y Siblings). Útil tras crear
    /// o borrar una subcarpeta desde el panel.
    pub fn refresh_folder_lists(&mut self) {
        *self.folders = FolderLists {
            siblings: sibling_folders(&self.folder),
            children: child_folders(&self.folder),
        };
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
    CreateFolder(PathBuf, String),
    RenameFolder(PathBuf, String),
    DeleteFolder(PathBuf),
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

#[cfg(test)]
mod tests {
    use super::ui::gallery_cell_size;
    use super::{next_folder_panel_side, FolderNavigation};
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
        let size = gallery_cell_size(900.0, 5);
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
