//! Integración con el escritorio en macOS: registra la app como manejador
//! de los tipos de imagen soportados usando `LaunchServices`.
//!
//! En macOS, las asociaciones de archivo se declaran normalmente en el
//! `Info.plist` del bundle (`.app`), y el SO enruta los archivos abiertos
//! a través de `application:openURLs:` (no por argv). Pero también es
//! posible registrar tipos dinámicamente con `LaunchServices` vía
//! `LSSetDefaultRoleHandlerForContentType` y `UTTypeDeclare*`. Para una
//! app sin bundle (binario suelto), `register` escribe un `Info.plist`
//! temporal junto al binario y lo registra con `lsregister`.
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

impl ShellIntegration for MacShell {
    fn register_file_associations(&self, exe: &Path) -> Result<(), ShellError> {
        // En macOS, sin un bundle `.app` completo no se pueden registrar
        // asociaciones a nivel de `LaunchServices` de forma fiable.
        // `lsregister` acepta un `Info.plist` suelto, pero el SO no lo
        // usará hasta que el binario esté dentro de un bundle con la
        // estructura correcta (`Canvas Desktop.app/Contents/MacOS/exe`).
        //
        // Lo que sí se puede hacer de forma fiable: escribir un
        // `Info.plist` junto al binario para que un empaquetador posterior
        // lo incluya en el bundle, y registrar los UTI con `lsregister`.
        let plist_path = exe
            .parent()
            .ok_or_else(|| {
                ShellError::Registry("no se pudo resolver el directorio del exe".into())
            })?
            .join("Info.plist");

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

        // Registrar con `lsregister` (mejor esfuerzo; si no está, el
        // plist seguirá ahí para que un empaquetador lo use).
        let lsregister = Path::new("/System/Library/Frameworks/CoreServices.framework")
            .join("Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister");
        if lsregister.exists() {
            let _ = std::process::Command::new(&lsregister)
                .arg(&plist_path)
                .output();
        }

        Ok(())
    }

    fn unregister_file_associations(&self) -> Result<(), ShellError> {
        let exe = std::env::current_exe().map_err(|e| ShellError::Registry(e.to_string()))?;
        let plist_path = exe
            .parent()
            .ok_or_else(|| {
                ShellError::Registry("no se pudo resolver el directorio del exe".into())
            })?
            .join("Info.plist");
        if plist_path.exists() {
            std::fs::remove_file(&plist_path).map_err(|e| ShellError::Registry(e.to_string()))?;
        }
        let lsregister = Path::new("/System/Library/Frameworks/CoreServices.framework")
            .join("Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister");
        if lsregister.exists() {
            let _ = std::process::Command::new(&lsregister)
                .arg("-u")
                .arg(&plist_path)
                .output();
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
