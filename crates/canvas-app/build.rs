//! Incrusta `assets/windows/icon.ico` como recurso del `.exe` (icono de la
//! barra de tareas, del Explorador y del propio `DefaultIcon` del registro).
//! No-op fuera de Windows: `winresource` es un build-dependency normal en
//! todas las plataformas, pero solo invoca `rc.exe`/`windres` cuando el
//! target es Windows (ver docs de `winresource::WindowsResource::compile`).

use std::path::Path;

fn main() -> std::io::Result<()> {
    let icon = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/windows/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Escape para máquinas sin Windows SDK (la herramienta `rc` no
        // existe): `CANVAS_SKIP_ICON=1` compila sin embeber el icono en vez
        // de morir. El CI real siempre lo tiene, así que el .exe de release
        // lleva icono siempre; esto solo sirve para clippy local.
        if std::env::var_os("CANVAS_SKIP_ICON").is_some_and(|v| v == "1") {
            println!("cargo:warning=CANVAS_SKIP_ICON=1: sin rc, compilo sin icono");
            return Ok(());
        }
        let mut res = winresource::WindowsResource::new();
        res.set_icon(
            icon.to_str()
                .expect("la ruta del icono debe ser UTF-8 válido"),
        );
        res.compile()?;
    }

    Ok(())
}
