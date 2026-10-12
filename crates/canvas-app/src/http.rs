//! Descarga HTTP acotada compartida por todos los caminos de red del app.
//!
//! Antes esto vivía duplicado en `loader/image_import.rs` (reemplazo por URL)
//! y en `unsplash/api.rs` (miniaturas/imágenes), cada uno con su propio
//! agente y su propio bucle de `take()` para el tope. Aquí hay UNA
//! implementación del agente con timeouts y de la lectura limitada; cada
//! llamador mapea el error a SU tipo (`UnsplashError`, `IoError`…).

use std::io::Read;
use std::time::Duration;

/// Tope de descarga (64 MB) compartido por todos los caminos: las imágenes
/// legítimas se quedan muy por debajo, y sin tope una respuesta maliciosa o
/// desbocada llenaría la memoria del proceso entera en un hilo worker.
pub const MAX_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Error de descarga, independiente del error de cada dominio. Las variantes
/// no llevan rastro del dominio para que cualquier llamador mapee a lo suyo
/// (`UnsplashError::Download`, `IoError::Message`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// Falla de red, HTTP o de lectura de la respuesta.
    Download(String),
    /// La respuesta superó el tope en bytes.
    TooLarge(usize),
    /// La respuesta no trajo ningún byte.
    Empty,
}

/// Agente HTTP con tiempos de espera acotados: una red colgada no puede
/// bloquear un hilo worker para siempre. Compartido para que `search` (que
/// añade query/headers al agente base) y las descargas usen los mismos
/// timeouts.
pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .build()
}

/// Descarga el contenido de `url` con un tope de `max` bytes. Corta con
/// error al superarlo en vez de acumular sin límite:
///
/// 1. Si el servidor declara `Content-Length`, se rechaza de antemano una
///    respuesta declarada más grande que el tope (sin ni siquiera leerla).
/// 2. `take` corta la lectura un byte después del tope: si al terminar hay
///    más de `max` bytes, la respuesta era demasiado grande (defensa para
///    servidores que no declaran `Content-Length` o mienten).
fn get_bytes(url: &str, max: usize) -> Result<Vec<u8>, HttpError> {
    get_bytes_with_headers(url, max, &[])
}

/// Lee como máximo `max + 1` bytes de `reader`. Si al terminar hay más de
/// `max`, la respuesta era demasiado grande (defensa para servidores que no
/// declaran `Content-Length` o mienten).
fn read_capped(reader: impl std::io::Read, max: usize) -> Result<Vec<u8>, HttpError> {
    let mut bytes = Vec::new();
    reader
        .take((max + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| HttpError::Download(e.to_string()))?;
    if bytes.len() > max {
        return Err(HttpError::TooLarge(max));
    }
    if bytes.is_empty() {
        return Err(HttpError::Empty);
    }
    Ok(bytes)
}

/// Rechaza de antemano una respuesta que declara por `Content-Length` más de
/// `max` bytes (sin ni siquiera leerla), y una que declara 0 (la lectura de
/// un body vacío depende de cómo el servidor cierre la conexión: en macOS a
/// veces se reporta como error de socket en vez de 0 bytes, así que se
/// rechaza aquí de forma determinista).
fn reject_oversized_header(resp: &ureq::Response, max: usize) -> Result<(), HttpError> {
    let content_length = resp
        .header("Content-Length")
        .and_then(|value| value.parse::<usize>().ok());
    if content_length.is_some_and(|length| length > max) {
        return Err(HttpError::TooLarge(max));
    }
    if content_length == Some(0) {
        return Err(HttpError::Empty);
    }
    Ok(())
}

/// Como `get_bytes`, pero añadiendo cabeceras a la petición (p. ej.
/// `User-Agent`/`Referer` de navegador para CDNs que bloquean clientes
/// no-navegador, como los de Meta). Comparte topes y lectura limitada.
pub fn get_bytes_with_headers(
    url: &str,
    max: usize,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, HttpError> {
    let mut req = agent().get(url);
    for (name, value) in headers {
        req = req.set(name, value);
    }
    let resp = req.call().map_err(|e| HttpError::Download(e.to_string()))?;
    reject_oversized_header(&resp, max)?;
    read_capped(resp.into_reader(), max)
}

/// Descarga con el tope compartido `MAX_DOWNLOAD_BYTES`.
pub fn get_bytes_bounded(url: &str) -> Result<Vec<u8>, HttpError> {
    get_bytes(url, MAX_DOWNLOAD_BYTES)
}

/// Lee un `Response` ya obtenido (con query/headers/método propios del
/// llamador, p. ej. los filtros de Unsplash o el POST de Serper) con el
/// mismo tope que `get_bytes_...`. Sin tope, una respuesta desbocada o un
/// `Content-Length` que miente llenarían la memoria antes de poder parsear.
pub fn read_response_string_bounded(resp: ureq::Response, max: usize) -> Result<String, HttpError> {
    reject_oversized_header(&resp, max)?;
    let bytes = read_capped(resp.into_reader(), max)?;
    String::from_utf8(bytes).map_err(|e| HttpError::Download(format!("respuesta no UTF-8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::{serve_and_capture, serve_response};

    #[test]
    fn downloads_a_small_body() {
        let address = serve_response("200 OK", b"image bytes");
        let bytes = get_bytes_bounded(&format!("http://{address}/img.png")).unwrap();
        assert_eq!(bytes, b"image bytes");
    }

    #[test]
    fn rejects_empty_body() {
        let address = serve_response("200 OK", b"");
        let error = get_bytes_bounded(&format!("http://{address}/empty.png")).unwrap_err();
        assert_eq!(error, HttpError::Empty);
    }

    #[test]
    fn passes_http_errors_through() {
        let address = serve_response("404 Not Found", b"missing");
        let error = get_bytes_bounded(&format!("http://{address}/missing.png")).unwrap_err();
        assert!(matches!(error, HttpError::Download(_)));
    }

    #[test]
    fn cuts_off_at_the_limit() {
        // Tope pequeño para no materializar 64 MiB en un test.
        let address = serve_response("200 OK", &[0u8; 11]);
        let error = get_bytes(&format!("http://{address}/large.png"), 10).unwrap_err();
        assert!(matches!(error, HttpError::TooLarge(n) if n == 10));
    }

    #[test]
    fn extra_headers_reach_the_server() {
        let (address, rx) = serve_and_capture("200 OK", b"bytes");
        let bytes = get_bytes_with_headers(
            &format!("http://{address}/img.png"),
            MAX_DOWNLOAD_BYTES,
            &[
                ("User-Agent", "TestBrowser/1.0"),
                ("Referer", "https://x.test/"),
            ],
        )
        .unwrap();
        assert_eq!(bytes, b"bytes");
        let req = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(req.contains("User-Agent: TestBrowser/1.0"), "{req}");
        assert!(req.contains("Referer: https://x.test/"), "{req}");
    }

    #[test]
    fn plain_download_sends_no_referer() {
        // Sin cabeceras extra no debe aparecer Referer (comportamiento
        // actual para hosts normales).
        let (address, rx) = serve_and_capture("200 OK", b"bytes");
        let _ = get_bytes_bounded(&format!("http://{address}/img.png")).unwrap();
        let req = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!req.to_lowercase().contains("referer:"), "{req}");
    }

    #[test]
    fn api_response_reading_respects_the_limit() {
        // El camino de las búsquedas (Unsplash/Serper) lee `Response` ajeno:
        // debe cortar igual que `get_bytes` cuando el cuerpo se pasa del tope.
        let address = serve_response("200 OK", b"{\"big\":\"respuesta demasiado larga\"}");
        let resp = agent()
            .get(&format!("http://{address}/search"))
            .call()
            .unwrap();
        let error = read_response_string_bounded(resp, 10).unwrap_err();
        assert!(matches!(error, HttpError::TooLarge(n) if n == 10));
    }

    #[test]
    fn api_response_reading_returns_the_body_when_within_the_limit() {
        let address = serve_response("200 OK", b"{\"ok\":true}");
        let resp = agent()
            .get(&format!("http://{address}/search"))
            .call()
            .unwrap();
        let body = read_response_string_bounded(resp, MAX_DOWNLOAD_BYTES).unwrap();
        assert_eq!(body, "{\"ok\":true}");
    }
}
