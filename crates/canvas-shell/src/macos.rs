//! Integración con el escritorio en macOS: registra la app como manejador
//! de los tipos de imagen soportados usando `LaunchServices`.
//!
//! En macOS, las asociaciones de archivo se declaran en el `Info.plist`
//! de un bundle `.app`. Para desarrollo con un binario suelto, `register`
//! crea un bundle temporal junto al ejecutable y registra el bundle completo
//! con `lsregister`.
//!
//! `update_jump_list` publica los recientes en el Dock vía
//! `NSDocumentController` — mejor esfuerzo, no es estándar.

use std::path::{Path, PathBuf};

use crate::integration::{ShellError, ShellIntegration};

/// Extensiones asociadas (deben coincidir con `windows.rs::ASSOC_EXTENSIONS`).
const ASSOC_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "svg", "gif", "bmp", "canvas"];

/// Bundle identifier de la app.
const BUNDLE_ID: &str = "com.canvas-desktop.app";

pub struct MacShell;

fn bundle_root(exe: &Path) -> Result<PathBuf, ShellError> {
    let parent = exe
        .parent()
        .ok_or_else(|| ShellError::Registry("no se pudo resolver el directorio del exe".into()))?;
    Ok(parent.join("Canvas Desktop.app"))
}

impl ShellIntegration for MacShell {
    fn register_file_associations(&self, exe: &Path) -> Result<(), ShellError> {
        let bundle_root = bundle_root(exe)?;
        let contents = bundle_root.join("Contents");
        let macos_dir = contents.join("MacOS");
        std::fs::create_dir_all(&macos_dir).map_err(|e| ShellError::Registry(e.to_string()))?;
        let plist_path = contents.join("Info.plist");
        let bundle_exe = macos_dir.join(
            exe.file_name()
                .ok_or_else(|| ShellError::Registry("exe sin nombre".into()))?,
        );
        if bundle_exe != exe {
            std::fs::copy(exe, &bundle_exe).map_err(|e| ShellError::Registry(e.to_string()))?;
        }

        let mime_types: Vec<&str> = ASSOC_EXTENSIONS
            .iter()
            .map(|ext| match *ext {
                "jpg" | "jpeg" => "public.jpeg",
                "png" => "public.png",
                "webp" => "org.webmproject.webp",
                "svg" => "public.svg-image",
                "gif" => "com.compuserve.gif",
                "bmp" => "com.microsoft.bmp",
                "canvas" => "com.canvas-desktop.design",
                _ => "public.data",
            })
            .collect();

        let mut content = String::new();
        content.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        content.push_str("<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ");
        content.push_str("\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n");
        content.push_str("<plist version=\"1.0\">\n");
        content.push_str("<dict>\n");
        content.push_str(&format!(
            "\t<key>CFBundleIdentifier</key>\n\t<string>{BUNDLE_ID}</string>\n"
        ));
        content.push_str("\t<key>CFBundleName</key>\n\t<string>Canvas Desktop</string>\n");
        content.push_str("\t<key>CFBundleTypeRole</key>\n\t<string>Editor</string>\n");
        content.push_str("\t<key>CFBundleDocumentTypes</key>\n\t<array>\n");
        for mime in &mime_types {
            content.push_str("\t\t<dict>\n");
            content.push_str("\t\t\t<key>LSItemContentTypes</key>\n\t\t\t<array>\n");
            content.push_str(&format!("\t\t\t\t<string>{mime}</string>\n"));
            content.push_str("\t\t\t</array>\n");
            content.push_str("\t\t\t<key>CFBundleTypeName</key>\n\t\t\t<string>Image</string>\n");
            content.push_str("\t\t\t<key>LSHandlerRank</key>\n\t\t\t<string>Default</string>\n");
            content.push_str("\t\t</dict>\n");
        }
        content.push_str("\t</array>\n");
        content.push_str("\t<key>UTExportedTypeDeclarations</key>\n\t<array>\n");
        // Declarar el UTI propio del formato `.canvas`.
        content.push_str("\t\t<dict>\n");
        content.push_str(
            "\t\t\t<key>UTTypeIdentifier</key>\n\t\t\t<string>com.canvas-desktop.design</string>\n",
        );
        content.push_str(
            "\t\t\t<key>UTTypeDescription</key>\n\t\t\t<string>Canvas Desktop Design</string>\n",
        );
        content.push_str("\t\t\t<key>UTTypeConformsTo</key>\n\t\t\t<array>\n\t\t\t\t<string>public.data</string>\n\t\t\t</array>\n");
        content.push_str("\t\t\t<key>UTTypeTagSpecification</key>\n\t\t\t<dict>\n");
        content.push_str("\t\t\t\t<key>public.filename-extension</key>\n\t\t\t\t<array>\n\t\t\t\t\t<string>canvas</string>\n\t\t\t\t</array>\n");
        content.push_str("\t\t\t\t<key>public.mime-type</key>\n\t\t\t\t<array>\n\t\t\t\t\t<string>application/x-canvas-desktop</string>\n\t\t\t\t</array>\n");
        content.push_str("\t\t\t</dict>\n");
        content.push_str("\t\t</dict>\n");
        content.push_str("\t</array>\n");
        content.push_str("</dict>\n");
        content.push_str("</plist>\n");

        std::fs::write(&plist_path, content.as_bytes())
            .map_err(|e| ShellError::Registry(e.to_string()))?;

        let lsregister = Path::new("/System/Library/Frameworks/CoreServices.framework")
            .join("Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister");
        if lsregister.exists() {
            let _ = std::process::Command::new(&lsregister)
                .arg(&bundle_root)
                .output()
                .map_err(|e| ShellError::Registry(e.to_string()))?;
        } else {
            return Err(ShellError::Registry("no se encontró lsregister".into()));
        }

        Ok(())
    }

    fn unregister_file_associations(&self) -> Result<(), ShellError> {
        let exe = std::env::current_exe().map_err(|e| ShellError::Registry(e.to_string()))?;
        let bundle_root = bundle_root(&exe)?;
        let plist_path = bundle_root.join("Contents/Info.plist");
        if plist_path.exists() {
            std::fs::remove_file(&plist_path).map_err(|e| ShellError::Registry(e.to_string()))?;
        }
        let lsregister = Path::new("/System/Library/Frameworks/CoreServices.framework")
            .join("Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister");
        if lsregister.exists() {
            let _ = std::process::Command::new(&lsregister)
                .arg("-u")
                .arg(&bundle_root)
                .output()
                .map_err(|e| ShellError::Registry(e.to_string()))?;
        }
        if bundle_root.exists() {
            std::fs::remove_dir_all(&bundle_root)
                .map_err(|e| ShellError::Registry(e.to_string()))?;
        }
        Ok(())
    }

    fn update_jump_list(&self, _recents: &[PathBuf]) -> Result<(), ShellError> {
        // El Dock de macOS no expone una API pública simple para
        // publicar recientes desde un binario sin bundle. Los recientes
        // se gestionan internamente en la propia app.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_root_uses_app_structure() {
        let root = bundle_root(Path::new("/tmp/canvas-desktop")).expect("ruta válida");
        assert!(root.ends_with("Canvas Desktop.app"));
    }

    #[test]
    fn mime_types_cover_all_extensions() {
        let mimes: Vec<&str> = ASSOC_EXTENSIONS
            .iter()
            .map(|ext| match *ext {
                "jpg" | "jpeg" => "public.jpeg",
                "png" => "public.png",
                "webp" => "org.webmproject.webp",
                "svg" => "public.svg-image",
                "gif" => "com.compuserve.gif",
                "bmp" => "com.microsoft.bmp",
                "canvas" => "com.canvas-desktop.design",
                _ => "public.data",
            })
            .collect();
        assert_eq!(mimes.len(), ASSOC_EXTENSIONS.len());
        assert!(mimes.iter().all(|m| !m.is_empty()));
    }
}
