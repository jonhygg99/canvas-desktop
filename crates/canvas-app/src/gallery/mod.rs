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

/// Espera minima tras abrir Ajustes antes de fiarnos del foco: descarta el
/// parpadeo de foco que provoca el propio `open` al lanzar Ajustes.
const PERMISSION_RETRY_MIN: std::time::Duration = std::time::Duration::from_secs(2);

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

pub(crate) fn normalize_folder(folder: PathBuf) -> PathBuf {
    std::fs::canonicalize(&folder).unwrap_or(folder)
}

/// Lee las subcarpetas directas de `folder`: read_dir resiliente, ocultos
/// fuera, orden natural. El error sale como String listo para la UI.
pub(crate) fn read_child_folders(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = canvas_io::read_dir_resilient(folder)
        .map_err(|e| canvas_io::describe_read_dir_error(folder, &e))?;
    let mut folders: Vec<PathBuf> = entries
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
    Ok(folders)
}

/// Lista las subcarpetas de `folder`. Ante un fallo de lectura (p. ej. un
/// montaje de nube que responde EPERM mientras hidrata contenido
/// solo-en-linea) NO devuelve una lista vacia silenciosa: guarda el error
/// para que el panel pueda explicar por que no hay carpetas.
fn child_folders(folder: &Path) -> FolderLists {
    match read_child_folders(folder) {
        Ok(children) => FolderLists {
            children,
            read_error: None,
        },
        Err(error) => {
            tracing::warn!(folder = %folder.display(), error = %error, "folder listing failed");
            FolderLists {
                children: Vec::new(),
                read_error: Some(error),
            }
        }
    }
}
struct FolderLists {
    children: Vec<PathBuf>,
    /// Por que no se pudo listar (carpetas vacias ≠ carpeta ilegible).
    read_error: Option<String>,
}
pub struct GalleryState {
    pub folder: PathBuf,
    pub folder_panel_side: StripSide,
    navigation: FolderNavigation,
    folders: Box<FolderLists>,
    pub items: Vec<GalleryItem>,
    pub scanned: bool,
    /// Error de lectura de la carpeta, si el escaneo no pudo abrirla.
    pub scan_error: Option<String>,
    pub sort: GallerySort,
    /// Número de diseños que se muestran por línea (no cambia los archivos).
    pub gallery_columns: usize,
    /// Última celda marcada con clic derecho: lo que copia Ctrl+C.
    pub selected: Option<PathBuf>,
    /// Renombrado en curso: ruta y texto editable (solo el nombre base, sin
    /// extensión — cambiarla rompería la detección de imagen/diseño).
    pub rename_edit: Option<(PathBuf, String)>,
    pub new_folder_inside: Option<String>,
    pub folder_rename_edit: Option<(PathBuf, String)>,
    /// Último fallo de una operación de archivos (crear/duplicar/pegar/
    /// renombrar/borrar), visible hasta que el usuario lo descarta.
    pub op_error: Option<String>,
    /// Instante en que el usuario abrio Ajustes para dar permiso (macOS):
    /// al recuperar la ventana el foco se reintenta el escaneo una sola vez.
    pub(crate) settings_opened_at: Option<std::time::Instant>,
    /// Foco de la ventana en el frame anterior (detecta el regreso).
    focus_was_up: bool,
    /// Bucle automatico de listado (montajes de nube) en marcha.
    pub(crate) folders_auto_refreshing: bool,
    /// Ciclos automaticos disponibles: se rearma con acciones del usuario
    /// (Retry / ↻ / abrir carpeta) para no ciclar eternamente en segundo plano.
    folders_auto_refresh_armed: bool,
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
        let folder = normalize_folder(folder);
        Self {
            folder_panel_side,
            navigation: FolderNavigation::new(folder.clone()),
            folders: Box::new(child_folders(&folder)),
            folder,
            items: Vec::new(),
            scanned: false,
            scan_error: None,
            sort,
            gallery_columns: 5,
            selected: None,
            rename_edit: None,
            new_folder_inside: None,
            folder_rename_edit: None,
            op_error: None,
            settings_opened_at: None,
            focus_was_up: true,
            folders_auto_refreshing: false,
            folders_auto_refresh_armed: true,
        }
    }

    pub fn with_navigation(
        folder: PathBuf,
        sort: GallerySort,
        navigation: FolderNavigation,
        folder_panel_side: StripSide,
    ) -> Self {
        let folder = normalize_folder(folder);
        Self {
            folder: folder.clone(),
            folder_panel_side,
            navigation,
            folders: Box::new(child_folders(&folder)),
            items: Vec::new(),
            scanned: false,
            scan_error: None,
            sort,
            gallery_columns: 5,
            selected: None,
            rename_edit: None,
            new_folder_inside: None,
            folder_rename_edit: None,
            op_error: None,
            settings_opened_at: None,
            focus_was_up: true,
            folders_auto_refreshing: false,
            folders_auto_refresh_armed: true,
        }
    }
    /// ¿Tiene que rescanearse esta galería porque cambió algo en `changed`?
    pub fn is_affected_by(&self, changed: &Path) -> bool {
        self.folder == changed || self.folder.parent() == Some(changed)
    }

    pub fn refresh_folder_lists(&mut self) {
        // Accion del usuario: rearma los ciclos automaticos de listado.
        self.folders_auto_refresh_armed = true;
        *self.folders = child_folders(&self.folder);
    }

    /// Condiciones para lanzar el bucle automatico de listado (montaje de
    /// nube, error presente, armado y sin bucle en marcha). Al dispararse
    /// desarma y marca el bucle: un ciclo acotado por visita, sin relanzarse
    /// eternamente; `refresh_folder_lists` lo rearma tras accion del usuario.
    pub(crate) fn take_folder_auto_refresh(&mut self, cloud_folder: bool) -> bool {
        let due = cloud_folder
            && self.folders_auto_refresh_armed
            && !self.folders_auto_refreshing
            && self.folders.read_error.is_some();
        if due {
            self.folders_auto_refresh_armed = false;
            self.folders_auto_refreshing = true;
        }
        due
    }

    /// Aplica el resultado de un reintento en segundo plano del listado
    /// (children + error) y libera el bucle. Encapsula `FolderLists`, que es
    /// privado para los modulos fuera del arbol de `gallery`.
    pub(crate) fn apply_folders_refresh(&mut self, children: Vec<PathBuf>, error: Option<String>) {
        self.folders.children = children;
        self.folders.read_error = error;
        self.folders_auto_refreshing = false;
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
    pub fn set_scan_error(&mut self, error: String) {
        self.items.clear();
        self.scanned = true;
        self.scan_error = Some(error);
    }

    pub fn merge_files(&mut self, files: Vec<(PathBuf, Option<SystemTime>)>) {
        self.scan_error = None;
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

    /// El usuario abrio Ajustes desde esta galeria para dar permiso.
    pub(crate) fn note_settings_opened(&mut self) {
        self.settings_opened_at = Some(std::time::Instant::now());
    }

    /// Un solo disparo: true la PRIMERA vez que, tras abrir Ajustes, la
    /// ventana recupera el foco (borde de subida) pasada la espera minima.
    /// Actualiza siempre el seguimiento de foco del frame.
    pub(crate) fn take_permission_retry_if_due(&mut self, focused_now: bool) -> bool {
        let due = matches!(self.settings_opened_at, Some(t) if t.elapsed() >= PERMISSION_RETRY_MIN)
            && focused_now
            && !self.focus_was_up;
        if due {
            self.settings_opened_at = None;
        }
        self.focus_was_up = focused_now;
        due
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
    /// Reabrir la carpeta actual tras conceder permisos en Ajustes (macOS).
    RetryScan,
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
    use super::{next_folder_panel_side, FolderNavigation, GalleryState};
    use crate::deck::StripSide;
    use crate::settings::GallerySort;
    use std::path::{Path, PathBuf};

    /// Una galería abierta en `folder`. `GalleryState::new` sondea el disco
    /// para las listas de carpetas; con una ruta que no existe salen vacías,
    /// que es justo lo que hace falta aquí.
    fn open_at(folder: &str) -> GalleryState {
        GalleryState::new(PathBuf::from(folder), GallerySort::Name, StripSide::Left)
    }

    #[test]
    fn a_gallery_rescans_when_its_own_folder_changes() {
        let g = open_at("raiz/hijo");
        assert!(g.is_affected_by(Path::new("raiz/hijo")));
    }

    #[test]
    fn a_gallery_rescans_when_its_parent_changes() {
        // El panel lateral lista las carpetas HERMANAS, que salen del padre:
        // crear o borrar una ahí también se ve desde aquí.
        let g = open_at("raiz/hijo");
        assert!(g.is_affected_by(Path::new("raiz")));
    }

    #[test]
    fn a_gallery_ignores_a_sibling_folder() {
        let g = open_at("raiz/hijo");
        assert!(!g.is_affected_by(Path::new("raiz/otro")));
    }

    #[test]
    fn a_gallery_ignores_a_grandparent() {
        // Dos niveles arriba no se ve desde aquí: ni es la carpeta abierta ni
        // aporta la lista de hermanas.
        let g = open_at("raiz/hijo/nieto");
        assert!(!g.is_affected_by(Path::new("raiz")));
    }

    #[test]
    fn a_gallery_ignores_its_own_subfolder() {
        let g = open_at("raiz");
        assert!(!g.is_affected_by(Path::new("raiz/hijo")));
    }

    #[test]
    fn folder_listing_reports_unreadable_folders_instead_of_silent_empty() {
        let g = open_at("no/such/folder");
        assert!(g.folders.children.is_empty());
        let error = g
            .folders
            .read_error
            .as_deref()
            .expect("an unreadable folder must report why");
        assert!(error.contains("no/such/folder"));
    }

    #[test]
    fn folder_listing_lists_real_subfolders_sorted_naturally() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("b2")).unwrap();
        std::fs::create_dir(dir.path().join("b10")).unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();

        let g = GalleryState::new(dir.path().to_path_buf(), GallerySort::Name, StripSide::Left);
        let names: Vec<_> = g
            .folders
            .children
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["b2", "b10"]);
        assert!(g.folders.read_error.is_none());
    }

    #[test]
    fn permission_retry_fires_once_on_focus_regain_after_delay() {
        let mut g = open_at("x");
        assert!(!g.take_permission_retry_if_due(true));

        g.note_settings_opened();
        // Ajustes roba el foco a la ventana:
        assert!(!g.take_permission_retry_if_due(false));
        // Envejece la marca mas alla de la espera minima:
        g.settings_opened_at = Some(std::time::Instant::now() - super::PERMISSION_RETRY_MIN);
        assert!(!g.take_permission_retry_if_due(false));
        // La ventana recupera el foco: UN disparo.
        assert!(g.take_permission_retry_if_due(true));
        assert!(!g.take_permission_retry_if_due(true));
        assert!(!g.take_permission_retry_if_due(false));
    }

    #[test]
    fn folder_auto_refresh_runs_one_bounded_cycle_until_user_rearms() {
        let mut g = open_at("x"); // carpeta inexistente: read_error presente
        let cloud = true;

        assert!(g.take_folder_auto_refresh(cloud));
        // Sin bucle duplicado mientras esta en marcha:
        assert!(!g.take_folder_auto_refresh(cloud));
        // Un intento FALLIDO del bucle libera el bucle pero NO rearma:
        g.apply_folders_refresh(Vec::new(), Some("still denied".to_owned()));
        assert!(g.folders.read_error.is_some());
        assert!(!g.folders_auto_refreshing);
        // Agotado el ciclo NO se relanza solo...
        assert!(!g.take_folder_auto_refresh(cloud));
        // ...hasta que una accion del usuario rearma (Retry / ↻ / abrir).
        g.refresh_folder_lists();
        assert!(g.take_folder_auto_refresh(cloud));

        // En carpetas locales no hay bucle automatico.
        let mut local = open_at("x");
        local.refresh_folder_lists();
        assert!(!local.take_folder_auto_refresh(false));
    }

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
