//! Criterio de aceptación 9: matar el proceso a mitad de un guardado deja el
//! archivo original intacto y decodificable, nunca a medias.
//!
//! Cómo funciona: el test re-invoca este mismo binario de tests filtrando el
//! test `helper_slow_save`, que (solo cuando las variables de entorno están
//! puestas) ejecuta un `save_rgba` real con la ventana entre fsync y
//! sustitución alargada artificialmente. El padre espera a que aparezca el
//! temporal `.canvas-desktop-*`, mata al hijo en ese punto y comprueba que el
//! original sigue siendo el de antes, byte a byte decodificable.

use std::path::Path;
use std::time::{Duration, Instant};

/// Rol de hijo: corre un guardado lento sobre la ruta indicada por env var.
/// Sin la variable (ejecución normal de `cargo test`) no hace nada.
#[test]
fn helper_slow_save() {
    let Ok(target) = std::env::var("CANVAS_KILL_TEST_TARGET") else {
        return;
    };
    // Payload con ruido para que la codificación PNG no lo colapse.
    let (w, h) = (256u32, 256u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    let mut seed: u32 = 0xDEAD_BEEF;
    for _ in 0..w * h {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        rgba.extend_from_slice(&[
            (seed >> 24) as u8,
            (seed >> 16) as u8,
            (seed >> 8) as u8,
            255,
        ]);
    }
    // El gancho CANVAS_IO_TEST_SLEEP_BEFORE_REPLACE_MS (heredado del padre)
    // mantiene el temporal en disco el tiempo suficiente para el kill.
    let _ = canvas_io::save_rgba(Path::new(&target), rgba, w, h, 92, None);
}

#[test]
fn killing_mid_save_leaves_original_intact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("foto.png");

    // Siembra un original válido y conocido.
    let seed_img = image::RgbaImage::from_pixel(32, 32, image::Rgba([10, 200, 30, 255]));
    seed_img.save(&target).expect("sembrar original");
    let original_bytes = std::fs::read(&target).expect("leer original");

    // Lanza el hijo: este mismo binario de tests, solo el helper, con el
    // guardado ralentizado 30 s entre fsync y sustitución.
    let mut child = std::process::Command::new(std::env::current_exe().expect("current_exe"))
        .args(["helper_slow_save", "--exact", "--nocapture"])
        .env("CANVAS_KILL_TEST_TARGET", &target)
        .env("CANVAS_IO_TEST_SLEEP_BEFORE_REPLACE_MS", "30000")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("lanzar hijo");

    // Espera a que el temporal exista: el hijo está DENTRO del guardado.
    let tmp_exists = |dir: &Path| {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries.filter_map(|e| e.ok()).any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with(".canvas-desktop-")
                })
            })
            .unwrap_or(false)
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !tmp_exists(dir.path()) {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("el hijo nunca llegó a crear el temporal");
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!("el hijo terminó antes de tiempo: {status}");
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    // Kill a mitad de guardado (el temporal está escrito, el original no se
    // ha sustituido aún).
    child.kill().expect("matar al hijo");
    let _ = child.wait();

    // El original sigue intacto byte a byte y decodifica.
    let after = std::fs::read(&target).expect("releer original");
    assert_eq!(after, original_bytes, "el original cambió tras el kill");
    let decoded = image::open(&target).expect("decodificable").to_rgba8();
    assert_eq!(decoded.get_pixel(0, 0).0, [10, 200, 30, 255]);

    // Limpieza del temporal huérfano del guardado abortado y comprobación de
    // que un guardado completo posterior funciona y no deja restos.
    for entry in std::fs::read_dir(dir.path()).expect("leer dir").flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".canvas-desktop-")
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let rgba = vec![128u8; 16 * 16 * 4];
    canvas_io::save_rgba(&target, rgba, 16, 16, 92, None).expect("guardado completo posterior");
    image::open(&target).expect("nuevo contenido decodificable");
    assert!(!tmp_exists(dir.path()), "quedaron temporales huérfanos");
}
