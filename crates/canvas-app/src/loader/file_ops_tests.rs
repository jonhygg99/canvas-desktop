//! Tests de `file_ops`: rutas que mueven la pareja imagen + sidecar y no
//! deben dejar nunca a uno de los dos huérfano. Hermano de `file_ops.rs`
//! (convención `*_tests.rs`).

use super::{
    rename_with_sidecar, restore_one, trash_locally_with_sidecar, trash_with_sidecar, PairOutcome,
};
use canvas_io::sidecar_path;
use std::fs;
use std::path::Path;

/// Escribe una imagen y su sidecar (carpeta `.canvas` incluida) en `dir`.
fn image_with_sidecar(dir: &Path, name: &str) -> std::path::PathBuf {
    let image = dir.join(name);
    fs::write(&image, b"img").unwrap();
    fs::create_dir_all(dir.join(canvas_io::SIDECAR_DIR)).unwrap();
    fs::write(sidecar_path(&image), b"sidecar").unwrap();
    image
}

/// Renombrar con las capas editables detrás mueve las dos cosas juntas.
#[test]
fn rename_moves_the_sidecar_along_with_the_image() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_with_sidecar(dir.path(), "1.png");

    let dst = rename_with_sidecar(&image, "portada").unwrap();

    assert_eq!(dst, dir.path().join("portada.png"));
    assert!(!image.exists(), "la imagen original ya no debe existir");
    assert!(sidecar_path(&dst).exists(), "el sidecar acompaña la imagen");
    assert!(!sidecar_path(&image).exists(), "no queda sidecar huérfano");
}

/// Renombrar sin sidecar funciona igual (no hay nada que mover).
#[test]
fn rename_works_without_a_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("2.png");
    fs::write(&image, b"img").unwrap();

    let dst = rename_with_sidecar(&image, "dos").unwrap();

    assert!(dst.exists());
    assert!(!image.exists());
}

/// Un destino que ya existe rechaza el renombrado antes de tocar nada: ni la
/// imagen se mueve ni el sidecar se queda atrás.
#[test]
fn rename_refuses_an_existing_destination() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_with_sidecar(dir.path(), "3.png");
    fs::write(dir.path().join("ocupado.png"), b"x").unwrap();

    let err = rename_with_sidecar(&image, "ocupado").unwrap_err();

    assert!(image.exists(), "la imagen no se toca");
    assert!(sidecar_path(&image).exists(), "el sidecar tampoco");
    let msg = err.to_string();
    assert!(msg.contains("already exists"), "mensaje: {msg}");
}

/// El sidecar no se puede renombrar (un DIRECTORIO ocupa su destino, que ni
/// Windows ni Unix permiten sobrescribir con un `rename`): el rename de la
/// imagen se deshace y la imagen vuelve a su nombre original, con su sidecar
/// intacto. Sin el rollback quedaría una foto renombrada y sus capas
/// editables huérfanas bajo el nombre viejo.
#[test]
fn rename_rolls_the_image_back_when_the_sidecar_cannot_move() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_with_sidecar(dir.path(), "4.png");
    // Bloquea el destino del sidecar: `.canvas/portada.png.canvas` como
    // carpeta hace que `fs::rename(sidecar, destino)` falle.
    fs::create_dir_all(sidecar_path(&dir.path().join("portada.png"))).unwrap();

    let err = rename_with_sidecar(&image, "portada").unwrap_err();

    assert!(image.exists(), "la imagen vuelve a su nombre original");
    assert!(
        !dir.path().join("portada.png").exists(),
        "no debe quedar la imagen renombrada"
    );
    assert!(
        sidecar_path(&image).exists(),
        "el sidecar sigue donde estaba, junto a la imagen"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("editable layers could not be moved"),
        "el error debe explicar el fallo del sidecar: {msg}"
    );
}

/// El caso del sidecar HERMANO legacy: no vive en `.canvas/`, así que
/// renombrarlo exige crear esa carpeta primero. Si no se puede crear, el
/// sidecar tampoco se puede mover: la imagen debe volver a su nombre en vez
/// de quedarse renombrada y sin capas editables. Aquí `.canvas` es un
/// ARCHIVO, no una carpeta: `create_dir` falla con algo distinto de
/// AlreadyExists y el sidecar legacy queda imposible de situar.
#[test]
fn rename_rolls_the_image_back_when_the_sidecar_folder_cannot_be_created() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("5.png");
    fs::write(&image, b"img").unwrap();
    // Sidecar hermano legacy (la ubicación antigua, junto a la imagen).
    let legacy = dir.path().join("5.png.canvas");
    fs::write(&legacy, b"sidecar").unwrap();
    // `.canvas` como archivo: ni es carpeta ni se puede crear encima.
    fs::write(
        dir.path().join(canvas_io::SIDECAR_DIR),
        b"no soy una carpeta",
    )
    .unwrap();

    let err = rename_with_sidecar(&image, "cinco").unwrap_err();

    assert!(image.exists(), "la imagen vuelve a su nombre original");
    assert!(
        !dir.path().join("cinco.png").exists(),
        "no debe quedar la imagen renombrada"
    );
    assert!(
        legacy.exists(),
        "el sidecar legacy no se ha movido ni perdido"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("editable layers could not be moved"),
        "el error debe explicar que no se pudieron mover las capas: {msg}"
    );
}

// ——— resultados a medias: el archivo se mueve, el sidecar no ———

/// Un resultado con aviso no es un éxito silencioso ni un error: la
/// operación principal ocurrió, y el aviso explica lo que quedó pendiente
/// (antes esto era un `tracing::warn!` y el usuario veía éxito).
fn assert_partial(outcome: PairOutcome, needle: &str) {
    assert!(
        outcome.result.is_ok(),
        "la operación principal salió bien: {:?}",
        outcome.result
    );
    let warning = outcome.warning.expect("debe haber aviso");
    assert!(
        warning.contains(needle),
        "el aviso debe mencionar «{needle}»: {warning}"
    );
}

/// Trastear una imagen con sidecar mueve las dos cosas y no avisa de nada.
/// Caso sin trampas: el destino del sidecar se desambigua si hace falta, así
/// que aquí nunca hay operación a medias.
#[test]
fn trashing_locally_an_image_with_its_sidecar_reports_no_warning() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_with_sidecar(dir.path(), "6.png");

    let outcome = trash_locally_with_sidecar(&image);
    assert!(outcome.result.is_ok());
    assert_eq!(outcome.warning, None, "no hay nada que avisar");
}

/// Trastear sin sidecar es igual de limpio: no hay parte editable que fallar.
#[test]
fn trashing_locally_without_a_sidecar_reports_no_warning() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("7.png");
    fs::write(&image, b"img").unwrap();

    let outcome = trash_locally_with_sidecar(&image);
    assert!(outcome.result.is_ok());
    assert_eq!(outcome.warning, None);
}

/// Lo mismo por la papelera del sistema: con sidecar válido no hay aviso.
#[test]
fn trashing_with_a_deletable_sidecar_reports_no_warning() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_with_sidecar(dir.path(), "8.png");

    let outcome = trash_with_sidecar(&image);
    assert!(outcome.result.is_ok());
    assert_eq!(outcome.warning, None);
}

/// El modo de fallo real, sin depender del sistema de archivos: la función
/// que compone el resultado. Si el sidecar no se pudo mover, el aviso se
/// genera y la operación principal sigue siendo `Ok`.
#[test]
fn a_warning_travels_with_a_successful_result() {
    let outcome = PairOutcome::warned("las capas se quedaron".to_owned());
    assert_partial(outcome, "se quedaron");
}

/// Y al revés: un fallo de la operación principal viaja como error, sin
/// aviso (no hay operación a medias que explicar).
#[test]
fn a_failed_main_operation_carries_no_warning() {
    let outcome: PairOutcome = PairOutcome {
        result: Err(canvas_io::IoError::Message {
            message: "no se pudo mover".to_owned(),
        }),
        warning: None,
    };
    assert!(outcome.warning.is_none());
    assert!(outcome.result.is_err());
}

/// Restaurar busca la entrada trasteada por nombre: con la papelera vacía
/// devuelve error, no un falso éxito.
#[test]
fn restoring_with_an_empty_trash_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("9.png");
    fs::write(&image, b"img").unwrap();

    let outcome = restore_one(&image, &dir.path().join("missing-trash-entry"));
    assert!(outcome.result.is_err(), "no hay nada que restaurar");
    assert_eq!(outcome.warning, None);
}

#[test]
fn undo_restores_the_specific_deletion_not_an_older_copy() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("repeat.png");
    fs::write(&image, b"first").unwrap();
    let first = canvas_io::move_to_local_trash(&image).unwrap();
    fs::write(&image, b"second").unwrap();
    let second = canvas_io::move_to_local_trash(&image).unwrap();
    let outcome = restore_one(&image, &second);
    assert!(outcome.result.is_ok());
    assert_eq!(fs::read(&image).unwrap(), b"second");
    assert!(first.exists());
    assert!(!second.exists());
}

#[test]
fn trash_record_keeps_the_matching_image_and_editable_layers() {
    let dir = tempfile::tempdir().unwrap();
    let image = image_with_sidecar(dir.path(), "pair.png");
    let first = trash_locally_with_sidecar(&image).result.unwrap();
    fs::write(&image, b"new image").unwrap();
    fs::write(sidecar_path(&image), b"new layers").unwrap();
    let second = trash_locally_with_sidecar(&image).result.unwrap();
    assert_ne!(first.staged, second.staged);
    assert!(restore_one(&second.path, &second.staged).result.is_ok());
    let (original, staged) = second.sidecar.unwrap();
    assert!(restore_one(&original, &staged).result.is_ok());
    assert_eq!(fs::read(&image).unwrap(), b"new image");
    assert_eq!(fs::read(original).unwrap(), b"new layers");
    assert!(first.staged.exists());
    assert!(first.sidecar.unwrap().1.exists());
}
