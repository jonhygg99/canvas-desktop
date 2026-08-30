//! Servidor HTTP de prueba compartido por los tests de descargas (`http.rs`
//! y `loader/image_import.rs`). Único lugar donde vive la corrección del
//! «FIN limpio» que estabilizó esos tests flaky; no se compila fuera de los
//! tests (`#[cfg(test)]` en `main.rs`).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

/// Servidor HTTP en loopback que devuelve una respuesta controlada. El hilo
/// sirve una sola petición. Antes de responder drena el request del cliente
/// (las cabeceras hasta `\r\n\r\n`) y cierra SOLO con el `drop` — sin
/// `shutdown(Both)`.
///
/// Por qué: si el servidor cierra sin haber leído lo que el cliente envió,
/// macOS responde al `close` con RST en vez de FIN, y la limpieza del pool
/// de conexiones de ureq (`return_to_pool` → `set_read_timeout(None)`) choca
/// con ese socket a medio cerrar con `EINVAL` y panica («returning stream to
/// pool: Os { code: 22 }»). Eso hacía flaky estos tests bajo carga (≈1 de 3
/// suites completas). Drenando el request el cierre es FIN limpio y el pool
/// de ureq no disputa. Devuelve la dirección (`host:puerto`) a la que
/// apuntar el GET.
pub(crate) fn serve_response(status: &str, body: &[u8]) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let body = body.to_vec();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        // Drena las cabeceras del request para que el cierre sea FIN limpio
        // (sin datos sin leer → nunca RST). El GET de ureq llega completo de
        // golpe, así que este bucle termina rápido.
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
        // Sin `shutdown(Both)`: el drop cierra solo.
    });
    address.to_string()
}
