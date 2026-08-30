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
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| HttpError::Download(e.to_string()))?;

    let content_length = resp
        .header("Content-Length")
        .and_then(|value| value.parse::<usize>().ok());
    if content_length.is_some_and(|length| length > max) {
        return Err(HttpError::TooLarge(max));
    }
    // `Content-Length: 0` declarado: la lectura de un body vacío depende de
    // cómo el servidor cierre la conexión (en macOS a veces se reporta como
    // error de socket en vez de 0 bytes leídos), así que se rechaza aquí de
    // forma determinista.
    if content_length == Some(0) {
        return Err(HttpError::Empty);
    }

    let mut bytes = Vec::new();
    resp.into_reader()
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

/// Descarga con el tope compartido `MAX_DOWNLOAD_BYTES`.
pub fn get_bytes_bounded(url: &str) -> Result<Vec<u8>, HttpError> {
    get_bytes(url, MAX_DOWNLOAD_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Servidor HTTP en loopback que devuelve una respuesta controlada. El
    /// hilo se queda vivo sirviendo una sola petición (el cliente cierra la
    /// conexión al terminar).
    fn serve_response(status: &str, body: &[u8]) -> String {
        use std::io::Write;
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
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
            stream.shutdown(std::net::Shutdown::Both).unwrap();
        });
        address.to_string()
    }

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
}
