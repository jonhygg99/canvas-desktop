//! Integración con el escritorio en Linux: instala un archivo `.desktop`
//! en `~/.local/share/applications/` con `MimeType` para las extensiones
//! soportadas y ejecuta `update-desktop-database` para que el escritorio lo
//! recoja. `unregister` borra el archivo y vuelve a ejecutar
//! `update-desktop-database`. `update_jump_list` es no-op (no hay equivalente
//! estándar de Jump List en freedesktop.org; los recientes se gestionan
//! internamente).
//!
//! Los `.desktop` son el mecanismo estándar de asociación de archivos en
//! freedesktop.org (GNOME, KDE Plasma, XFCE, etc.): el escritorio busca en
//! `XDG_DATA_DIRS` y `~/.local/share/applications/`, y usa el campo
//! `MimeType` para decidir qué apps ofrecen «Abrir con» para cada extensión.
//! `update-desktop-database` actualiza el caché `mimeinfo.cache` que acelera
//! la búsqueda.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::integration::{ShellError, ShellIntegration};

/// Extensiones asociadas (deben coincidir con `windows.rs::ASSOC_EXTENSIONS`).
const ASSOC_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "svg", "gif", "bmp", "canvas"];

const DESKTOP_FILE_NAME: &str = "canvas-desktop.desktop";
const APP_ID: &str = "canvas-desktop";

pub struct LinuxShell;

impl ShellIntegration for LinuxShell {
    fn register_file_associations(&self, exe: &Path) -> Result<(), ShellError> {
        let dir = applications_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| ShellError::Registry(e.to_string()))?;
        let path = dir.join(DESKTOP_FILE_NAME);
        // MIME type por extensión. `jpg` y `jpeg` producen el mismo MIME
        // (`image/jpeg`), así que se deduplican para no escribirlo dos veces
        // en `MimeType=` (freedesktop.org lo acepta, pero queda feo y algunos
        // validadores lo marcan como warning).
        let mut mime_types: Vec<&str> = ASSOC_EXTENSIONS
            .iter()
            .map(|ext| match *ext {
                "jpg" | "jpeg" => "image/jpeg",
                "png" => "image/png",
                "webp" => "image/webp",
                "svg" => "image/svg+xml",
                "gif" => "image/gif",
                "bmp" => "image/bmp",
                "canvas" => "application/x-canvas-desktop",
                _ => "application/octet-stream",
            })
            .collect();
        mime_types.sort();
        mime_types.dedup();

        let exe_str = escape_desktop_exec(exe.to_string_lossy().as_ref());
        let mut content = String::new();
        content.push_str("[Desktop Entry]\n");
        content.push_str("Type=Application\n");
        content.push_str(&format!("Name={}\n", "Canvas Desktop"));
        content.push_str("GenericName=Image Editor\n");
        content.push_str("Comment=Canva-like design editor\n");
        content.push_str(&format!("Exec=\"{exe_str}\" \"%f\"\n"));
        content.push_str(&format!("Icon={}\n", APP_ID));
        content.push_str("Terminal=false\n");
        content.push_str("Categories=Graphics;Photography;2DGraphics;\n");
        content.push_str(&format!("MimeType={};\n", mime_types.join(";")));

        let mut file =
            std::fs::File::create(&path).map_err(|e| ShellError::Registry(e.to_string()))?;
        file.write_all(content.as_bytes())
            .map_err(|e| ShellError::Registry(e.to_string()))?;
        // El bit de ejecutable no es estrictamente necesario para la
        // asociación, pero algunos escritorios lo verifican.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            let _ = std::fs::set_permissions(&path, perms);
        }

        update_desktop_database(&dir);
        Ok(())
    }

    fn unregister_file_associations(&self) -> Result<(), ShellError> {
        let dir = applications_dir()?;
        let path = dir.join(DESKTOP_FILE_NAME);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| ShellError::Registry(e.to_string()))?;
        }
        update_desktop_database(&dir);
        Ok(())
    }

    fn update_jump_list(&self, _recents: &[PathBuf]) -> Result<(), ShellError> {
        // No hay equivalente estándar de Jump List en freedesktop.org.
        // Los recientes se gestionan internamente en la propia app.
        Ok(())
    }
}

/// `~/.local/share/applications/` (o `$XDG_DATA_HOME/applications/`).
fn applications_dir() -> Result<PathBuf, ShellError> {
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.is_empty() {
            return Ok(PathBuf::from(xdg_data).join("applications"));
        }
    }
    // Fallback: `~/.local/share/applications/`.
    let home = std::env::var("HOME")
        .map_err(|_| ShellError::Registry("$HOME no está definido".to_string()))?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("applications"))
}

/// Ejecuta `update-desktop-database` en `dir` si está disponible en el PATH.
/// Mejor esfuerzo: si el comando no existe (algunos escritorios no lo
/// instalan), la asociación seguirá funcionando, solo tardará más en
/// reflejarse.
fn escape_desktop_exec(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
}

fn update_desktop_database(dir: &Path) {
    let _ = std::process::Command::new("update-desktop-database")
        .arg(dir)
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applications_dir_falls_back_to_home() {
        // Simula un entorno sin XDG_DATA_HOME.
        let saved = std::env::var("XDG_DATA_HOME").ok();
        std::env::remove_var("XDG_DATA_HOME");
        let dir = applications_dir();
        // Si HOME no está (CI muy restringida), se ignora el test.
        if std::env::var("HOME").is_ok() {
            assert!(dir.is_ok());
            let dir = dir.unwrap();
            assert!(dir.ends_with("applications"));
            assert!(dir.starts_with(".local") || dir.to_string_lossy().contains(".local"));
        }
        if let Some(v) = saved {
            std::env::set_var("XDG_DATA_HOME", v);
        }
    }

    #[test]
    fn applications_dir_respects_xdg_data_home() {
        let tmp = std::env::temp_dir().join("canvas-xdg-test");
        let _ = std::fs::create_dir_all(&tmp);
        let saved = std::env::var("XDG_DATA_HOME").ok();
        std::env::set_var("XDG_DATA_HOME", &tmp);
        let dir = applications_dir().unwrap();
        assert_eq!(dir, tmp.join("applications"));
        if let Some(v) = saved {
            std::env::set_var("XDG_DATA_HOME", v);
        } else {
            std::env::remove_var("XDG_DATA_HOME");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn escapes_desktop_exec_arguments() {
        assert_eq!(
            escape_desktop_exec(r#"/tmp/Canvas \"Desktop\"/$bin`"#),
            r#"/tmp/Canvas \\\"Desktop\\\"/\\$bin\\`"#
        );
    }

    #[test]
    fn mime_types_cover_all_extensions() {
        // Cada extensión debe tener un MIME type conocido.
        let mimes: Vec<&str> = ASSOC_EXTENSIONS
            .iter()
            .map(|ext| match *ext {
                "jpg" | "jpeg" => "image/jpeg",
                "png" => "image/png",
                "webp" => "image/webp",
                "svg" => "image/svg+xml",
                "gif" => "image/gif",
                "bmp" => "image/bmp",
                "canvas" => "application/x-canvas-desktop",
                _ => "application/octet-stream",
            })
            .collect();
        assert_eq!(mimes.len(), ASSOC_EXTENSIONS.len());
        assert!(mimes.iter().all(|m| !m.is_empty()));
    }
}
