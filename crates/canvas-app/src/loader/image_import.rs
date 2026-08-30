//! Añadir una imagen como capa nueva, o reemplazar la de una capa existente
//! (desde disco o descargada de una URL).

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use canvas_core::LayerId;
use eframe::egui;

use super::AppMsg;

/// Como `spawn_load_image`, pero el resultado se añade como capa nueva al
/// documento abierto en vez de sustituirlo.
pub fn spawn_load_image_as_layer(path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = canvas_io::load_image(&path);
        let _ = tx.send(AppMsg::ImageLoadedForLayer { path, result });
        ctx.request_repaint();
    });
}

pub fn spawn_pick_replacement_image(
    layer: LayerId,
    start_dir: Option<PathBuf>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Replace image")
            .add_filter("Images", canvas_io::IMAGE_EXTENSIONS);
        if let Some(dir) = start_dir.filter(|dir| dir.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            let label = image_label_from_path(&path);
            let result = canvas_io::load_image(&path);
            let _ = tx.send(AppMsg::ImageLoadedForReplace {
                layer,
                label,
                source_path: Some(path),
                result,
            });
        }
        ctx.request_repaint();
    });
}

pub fn spawn_load_replacement_image_from_url(
    layer: LayerId,
    url: String,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let label = image_label_from_url(&url);
        let result = download_url_to_temp(&url).and_then(|path| {
            let loaded = canvas_io::load_image(&path);
            let _ = std::fs::remove_file(&path);
            loaded
        });
        let _ = tx.send(AppMsg::ImageLoadedForReplace {
            layer,
            label,
            source_path: None,
            result,
        });
        ctx.request_repaint();
    });
}

fn image_label_from_path(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Image".to_owned())
}

fn image_label_from_url(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .and_then(|clean| clean.rsplit('/').next())
        .map(Path::new)
        .map(image_label_from_path)
        .filter(|name| name != "Image")
        .unwrap_or_else(|| "Internet image".to_owned())
}

fn extension_from_url(url: &str) -> &str {
    let clean = url.split(['?', '#']).next().unwrap_or(url);
    let ext = clean
        .rsplit('/')
        .next()
        .and_then(|name| Path::new(name).extension());
    ext.and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| canvas_io::IMAGE_EXTENSIONS.contains(&value.as_str()))
        .map(|value| match value.as_str() {
            "jpg" => "jpg",
            "jpeg" => "jpeg",
            "webp" => "webp",
            "gif" => "gif",
            "bmp" => "bmp",
            "svg" => "svg",
            _ => "png",
        })
        .unwrap_or("png")
}

fn download_url_to_temp(url: &str) -> Result<PathBuf, canvas_io::IoError> {
    let trimmed = url.trim();
    validate_http_url(trimmed)?;
    let bytes =
        crate::http::get_bytes_bounded(trimmed).map_err(|e| canvas_io::IoError::Message {
            message: http_error_message(e),
        })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| canvas_io::IoError::Message {
            message: format!("system clock error: {e}"),
        })?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "canvas-desktop-replace-{stamp}.{}",
        extension_from_url(trimmed)
    ));
    std::fs::write(&path, bytes).map_err(|source| canvas_io::IoError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Mapea un `HttpError` compartido al mensaje de usuario que ya presentaba
/// este camino (reemplazo por URL): conserva los mensajes que los tests y la
/// UI esperan (`image download failed`, `download limit`, `no data`).
fn http_error_message(error: crate::http::HttpError) -> String {
    match error {
        crate::http::HttpError::Download(e) => format!("image download failed: {e}"),
        crate::http::HttpError::TooLarge(n) => format!("image exceeds the {n}-byte download limit"),
        crate::http::HttpError::Empty => "image download returned no data".to_owned(),
    }
}

fn validate_http_url(url: &str) -> Result<(), canvas_io::IoError> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err(canvas_io::IoError::Message {
            message: "Use an http:// or https:// image URL".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{download_url_to_temp, validate_http_url};

    #[test]
    fn rejects_non_http_urls() {
        let error = validate_http_url("file:///tmp/image.png").unwrap_err();
        assert!(error.to_string().contains("http://"));
    }

    #[test]
    fn accepts_http_urls() {
        assert!(validate_http_url("https://example.com/image.png").is_ok());
    }

    #[test]
    fn rejects_empty_http_response() {
        let address = serve_response("200 OK", b"");
        let error = download_url_to_temp(&format!("http://{address}/empty.png")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("no data") || message.contains("download failed"));
    }

    #[test]
    fn rejects_http_errors() {
        let address = serve_response("404 Not Found", b"missing");
        let error = download_url_to_temp(&format!("http://{address}/missing.png")).unwrap_err();
        assert!(error.to_string().contains("download failed"));
    }
    #[test]
    fn rejects_responses_over_the_download_limit() {
        // El mapeo de `TooLarge` al mensaje de usuario es la parte propia de
        // este camino; el corte real por bytes vive en `http::tests` con un
        // tope pequeño (evita materializar 64 MiB aquí).
        let limit = crate::http::MAX_DOWNLOAD_BYTES;
        let message = super::http_error_message(crate::http::HttpError::TooLarge(limit));
        assert!(message.contains("download limit"));
    }

    #[test]
    fn writes_a_small_valid_response_to_a_unique_temp_file() {
        let address = serve_response("200 OK", b"image bytes");
        let path = download_url_to_temp(&format!("http://{address}/image.png")).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"image bytes");
        assert!(path.starts_with(std::env::temp_dir()));
        std::fs::remove_file(path).unwrap();
    }

    /// Servidor de prueba idéntico a `http::tests::serve_response`: drena el
    /// request antes de responder y cierra solo con `drop` (sin `shutdown`),
    /// para que el cierre sea FIN limpio y no RST — RST choca con la
    /// limpieza del pool de ureq (`EINVAL`) y hacía flaky estos tests.
    fn serve_response(status: &str, body: &[u8]) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_owned();
        let body = body.to_vec();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            // Drena las cabeceras del request para que el cierre sea FIN
            // limpio (sin datos sin leer → nunca RST); el GET llega completo.
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        req.extend_from_slice(&buf[..n]);
                        if req.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&body);
        });
        address.to_string()
    }
}
