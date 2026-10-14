//! Pruebas del esquema `{carpeta}-NN`: slug, huecos y siguiente libre.

use super::*;
use std::collections::HashSet;

#[test]
fn folder_names_become_lowercase_dash_slugs() {
    assert_eq!(
        folder_stem(Some(Path::new("/v/Edwin Luna"))),
        Some("edwin-luna".to_owned())
    );
    assert_eq!(
        folder_stem(Some(Path::new("/v/Edwin Luna-Kimberly Flores"))),
        Some("edwin-luna-kimberly-flores".to_owned())
    );
    assert_eq!(folder_stem(None), None);
}

#[test]
fn only_exact_prefix_dash_digits_count_as_taken() {
    let taken = taken_numbers("edwin-luna", ["edwin-luna-03", "edwin-luna-3"]);
    assert!(taken.contains(&3));
    let ignored = taken_numbers(
        "edwin-luna",
        [
            "Pasted Image",
            "Web · www.saps.com.mx",
            "edwin-luna",
            "edwin-luna-0",
            "edwin-luna-03.png",
            "edwin-luna-x3",
            "clip-edwin-luna-01",
        ],
    );
    assert!(ignored.is_empty());
}

#[test]
fn next_name_uses_two_digits_and_reuses_gaps() {
    assert_eq!(next_name("edwin-luna", &HashSet::new()), "edwin-luna-01");
    let taken: HashSet<u32> = [1, 2, 4].into_iter().collect();
    assert_eq!(next_name("edwin-luna", &taken), "edwin-luna-03");
    let taken: HashSet<u32> = (1..=99).collect();
    assert_eq!(next_name("edwin-luna", &taken), "edwin-luna-100");
}

#[test]
fn disk_files_count_as_taken_numbers() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("edwin-luna-01.png"), b"x").unwrap();
    std::fs::write(dir.path().join("edwin-luna-02.png.canvas"), b"x").unwrap();
    std::fs::write(dir.path().join("23.png"), b"x").unwrap();
    let taken = taken_from_dir(dir.path(), "edwin-luna");
    assert_eq!(taken, HashSet::from([1, 2]));
    assert!(taken_from_dir(dir.path(), "clip-edwin-luna").is_empty());
}

#[test]
fn next_image_name_unions_layers_and_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("edwin-luna-01.png"), b"x").unwrap();
    // La carpeta del test tiene un nombre temporal: el esquema manda,
    // no el valor concreto del slug.
    let prefix = folder_stem(Some(dir.path())).unwrap();
    let name = next_image_name(dir.path(), ["Pasted Image", &format!("{prefix}-02")]);
    assert_eq!(name, format!("{prefix}-03"));
}
#[test]
fn clip_prefix_scopes_clips_to_the_folder() {
    assert_eq!(
        clip_prefix(Some(Path::new("/v/Edwin Luna"))),
        Some("clip-edwin-luna".to_owned())
    );
    assert_eq!(clip_prefix(None), None);
}
