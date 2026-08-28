//! Tests del estado de la galería (navegación, merge de archivos,
//! orden, tamaño de celda). Movidos del `mod.rs`.

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
