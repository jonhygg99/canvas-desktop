//! Instancia única: la primera instancia toma un socket local con nombre
//! (named pipe en Windows) y escucha; las siguientes le envían sus rutas por
//! ese canal y salen con código 0. El proceso vivo las recibe como
//! `OpenPath`.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, Stream, ToNsName};

const SOCKET_NAME: &str = "canvas-desktop-single-instance.sock";

/// Papel de este proceso tras intentar adquirir la instancia única.
pub enum InstanceRole {
    /// Somos la instancia viva: conservar el listener y aceptar rutas.
    Primary(InstanceListener),
    /// Ya había una instancia; le enviamos las rutas. Salir con código 0.
    Secondary,
    /// No se pudo ni escuchar ni conectar (IPC roto): seguir en solitario,
    /// mejor una segunda ventana que ninguna.
    Standalone,
}

pub struct InstanceListener {
    listener: Listener,
}

/// Intenta convertirse en la instancia única. `paths_to_send` son las rutas
/// de argv que, si ya hay una instancia viva, se le reenvían (vacío = solo
/// pedirle que se traiga al frente).
pub fn acquire_instance(paths_to_send: &[PathBuf]) -> InstanceRole {
    acquire_instance_with_name(SOCKET_NAME, paths_to_send)
}

/// Igual que `acquire_instance` pero con un nombre de socket personalizado.
/// Los tests lo usan con un sufijo único para no colisionar con una
/// instancia real de la app corriendo en la misma máquina.
pub(crate) fn acquire_instance_with_name(
    socket_name: &str,
    paths_to_send: &[PathBuf],
) -> InstanceRole {
    let Ok(name) = socket_name.to_ns_name::<GenericNamespaced>() else {
        tracing::warn!("nombre de socket local inválido; sin instancia única");
        return InstanceRole::Standalone;
    };

    match ListenerOptions::new().name(name.clone()).create_sync() {
        Ok(listener) => InstanceRole::Primary(InstanceListener { listener }),
        Err(bind_err) => {
            // El lock ya está tomado: envía las rutas a la instancia viva.
            match Stream::connect(name) {
                Ok(mut conn) => {
                    if paths_to_send.is_empty() {
                        let _ = writeln!(conn);
                    }
                    for path in paths_to_send {
                        let _ = writeln!(conn, "{}", path.display());
                    }
                    let _ = conn.flush();
                    InstanceRole::Secondary
                }
                Err(connect_err) => {
                    tracing::warn!(
                        "instancia única no disponible (bind: {bind_err}; connect: \
                         {connect_err}); arrancando en solitario"
                    );
                    InstanceRole::Standalone
                }
            }
        }
    }
}

/// Topes por conexión del socket de instancia única: el canal lo puede
/// abrir cualquier proceso local, así que una línea es como mucho una ruta
/// (32 KiB cubren de sobra `MAX_PATH` extendido) y una conexión como mucho
/// un millar de rutas. Sin topes, una conexión que nunca termina anclaría
/// el hilo de aceptación (las siguientes aperturas se perderían) o una
/// línea gigante se alojaría entera en RAM (`lines()` la acumula hasta el
/// salto de línea).
pub const MAX_INSTANCE_LINE_BYTES: usize = 32 * 1024;
pub const MAX_INSTANCE_LINES: usize = 1024;
const MAX_INSTANCE_TOTAL_BYTES: u64 = 1024 * 1024;
/// Sin datos durante este plazo la conexión se da por terminada: en Windows
/// la lectura sin bloqueo no distingue «tubería vacía» de «emisor cerró»
/// (ambas llegan como `Ok(0)` porque `ERROR_NO_DATA` se degrada a EOF), así
/// que el vacío prolongado CUENTA como fin. El emisor real escribe sus rutas
/// de una vez al conectar, de modo que una pausa así de larga solo puede ser
/// una conexión muerta; de otro modo anclaría su hilo para siempre
/// (`set_recv_timeout` no existe en los named pipes de Windows, por eso el
/// plazo se aplica aquí con lectura sin bloqueo).
const INSTANCE_EMPTY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// Plazo total por conexión: cierra el goteo que esquivaría el anterior a
/// base de un byte cada pocos segundos. Una entrega real tarda milisegundos.
const INSTANCE_TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Pausa entre sondeos cuando aún no hay datos.
const INSTANCE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(5);

/// Entrega cada línea recibida al callback (una ruta por línea; línea vacía
/// = «tráete la ventana»), acotada por `MAX_INSTANCE_*`: al primer exceso
/// se avisa por `tracing` y se corta la conexión en vez de seguir leyendo.
fn pump_lines(conn: Stream, on_line: impl FnMut(String)) {
    pump_lines_with_timeout(conn, on_line, INSTANCE_EMPTY_TIMEOUT);
}

/// Como `pump_lines` pero con el plazo de vacío parametrizable (los tests
/// usan uno corto para no esperar el plazo de producción).
fn pump_lines_with_timeout(
    conn: Stream,
    mut on_line: impl FnMut(String),
    empty_timeout: std::time::Duration,
) {
    if conn.set_nonblocking(true).is_err() {
        // No debería pasar (el socket acaba de aceptarse); sin lectura sin
        // bloqueo no hay plazo de vacío, así que se suelta la conexión en
        // vez de arriesgar un hilo anclado para siempre.
        tracing::warn!("socket de instancia única: sin lectura sin bloqueo, conexión descartada");
        return;
    }
    let mut conn = conn.take(MAX_INSTANCE_TOTAL_BYTES);
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8 * 1024];
    let mut delivered = 0usize;
    let start = std::time::Instant::now();
    let empty_since = std::cell::Cell::new(start);
    // Sin datos: el vacío prolongado cuenta como fin de conexión (ver el
    // comentario de `INSTANCE_EMPTY_TIMEOUT`).
    let no_data = || {
        if empty_since.get().elapsed() > empty_timeout {
            tracing::warn!(
                "socket de instancia única: sin datos más de {empty_timeout:?} (se corta la conexión)"
            );
            return true;
        }
        if start.elapsed() > INSTANCE_TOTAL_TIMEOUT {
            tracing::warn!(
                "socket de instancia única: conexión abierta más de {INSTANCE_TOTAL_TIMEOUT:?} (se corta la conexión)"
            );
            return true;
        }
        std::thread::sleep(INSTANCE_POLL_INTERVAL);
        false
    };
    'conn: loop {
        match conn.read(&mut chunk) {
            Ok(0) => {
                if no_data() {
                    break;
                }
            }
            Ok(n) => {
                empty_since.set(std::time::Instant::now());
                pending.extend_from_slice(&chunk[..n]);
                while let Some(nl) = pending.iter().position(|&b| b == b'\n') {
                    let mut raw: Vec<u8> = pending.drain(..=nl).collect();
                    raw.pop(); // el '\n'
                    if raw.last() == Some(&b'\r') {
                        raw.pop(); // el '\r' de `\r\n`
                    }
                    let line = match String::from_utf8(raw) {
                        Ok(line) => line,
                        Err(_) => break 'conn, // como `lines()`: se corta al primer error UTF-8.
                    };
                    if line.len() > MAX_INSTANCE_LINE_BYTES {
                        tracing::warn!(
                            "socket de instancia única: línea de {} bytes supera el tope de {MAX_INSTANCE_LINE_BYTES} (se corta la conexión)",
                            line.len()
                        );
                        break 'conn;
                    }
                    if delivered >= MAX_INSTANCE_LINES {
                        tracing::warn!(
                            "socket de instancia única: más de {MAX_INSTANCE_LINES} rutas en una conexión (se corta la conexión)"
                        );
                        break 'conn;
                    }
                    delivered += 1;
                    on_line(line);
                }
                if pending.len() > MAX_INSTANCE_LINE_BYTES {
                    // Línea parcial que ya supera el tope sin ningún `\n`:
                    // cortarla ahora evita alojarla entera en RAM.
                    tracing::warn!(
                        "socket de instancia única: línea parcial supera el tope de {MAX_INSTANCE_LINE_BYTES} bytes (se corta la conexión)"
                    );
                    break 'conn;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if no_data() {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

impl InstanceListener {
    /// Acepta conexiones en un hilo propio y entrega cada línea recibida al
    /// callback (una ruta por línea; línea vacía = «tráete la ventana»).
    /// Cada conexión se atiende en su propio hilo: una que se quede abierta
    /// sin hablar no bloquea las siguientes.
    pub fn spawn_accept_loop(self, on_line: impl Fn(String) + Send + Sync + 'static) {
        let on_line = Arc::new(on_line);
        std::thread::spawn(move || loop {
            match self.listener.accept() {
                Ok(conn) => {
                    let on_line = Arc::clone(&on_line);
                    std::thread::spawn(move || pump_lines(conn, |line| on_line(line)));
                }
                Err(e) => {
                    tracing::warn!("accept del socket de instancia única falló: {e}");
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        });
    }

    /// Acepta UNA conexión síncrona y devuelve las líneas recibidas. Para
    /// tests: no lanza un hilo, no bucle infinito.
    #[cfg(test)]
    pub(crate) fn accept_one_sync(&self) -> Vec<String> {
        match self.listener.accept() {
            Ok(conn) => {
                let mut out = Vec::new();
                pump_lines(conn, |line| out.push(line));
                out
            }
            Err(e) => {
                tracing::warn!("accept síncrono de test falló: {e}");
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Genera un nombre de socket único para este test, usando el PID para
    /// evitar colisiones entre tests paralelos.
    fn test_socket_name(label: &str) -> String {
        format!("canvas-desktop-test-{label}-{}.sock", std::process::id())
    }

    #[test]
    fn first_instance_becomes_primary() {
        let name = test_socket_name("primary");
        let role = acquire_instance_with_name(&name, &[]);
        assert!(matches!(role, InstanceRole::Primary(_)), "debe ser Primary");
        // El listener se limpia al caer fuera de scope (drop).
        drop(role);
    }

    #[test]
    fn second_instance_becomes_secondary_and_sends_paths() {
        let name = test_socket_name("secondary");
        let primary = acquire_instance_with_name(&name, &[]);
        let InstanceRole::Primary(listener) = primary else {
            panic!("el primer adquirir debe ser Primary");
        };

        // Lanza un hilo que acepta UNA conexión y recoge las líneas.
        let handle = std::thread::spawn(move || listener.accept_one_sync());

        // Da un respiro para que el hilo esté en accept().
        std::thread::sleep(std::time::Duration::from_millis(50));

        let paths = vec![
            PathBuf::from("C:/test/a.png"),
            PathBuf::from("C:/test/b.png"),
        ];
        let role = acquire_instance_with_name(&name, &paths);
        assert!(
            matches!(role, InstanceRole::Secondary),
            "debe ser Secondary"
        );

        let received = handle.join().expect("el hilo no falló");
        assert_eq!(received.len(), 2, "debe recibir las 2 rutas enviadas");
        assert_eq!(received[0], "C:/test/a.png");
        assert_eq!(received[1], "C:/test/b.png");
    }

    #[test]
    fn empty_paths_send_a_blank_line() {
        let name = test_socket_name("blank");
        let primary = acquire_instance_with_name(&name, &[]);
        let InstanceRole::Primary(listener) = primary else {
            panic!("debe ser Primary");
        };

        let handle = std::thread::spawn(move || listener.accept_one_sync());
        std::thread::sleep(std::time::Duration::from_millis(50));

        let role = acquire_instance_with_name(&name, &[]);
        assert!(matches!(role, InstanceRole::Secondary));

        let received = handle.join().expect("hilo");
        // Una línea vacía = "tráete la ventana".
        assert_eq!(received.len(), 1);
        assert!(received[0].is_empty());
    }

    #[test]
    fn oversized_line_is_dropped_and_connection_closed() {
        use interprocess::local_socket::traits::Stream as _;
        use std::io::Write as _;
        let name = test_socket_name("caps");
        let primary = acquire_instance_with_name(&name, &[]);
        let InstanceRole::Primary(listener) = primary else {
            panic!("debe ser Primary");
        };
        let handle = std::thread::spawn(move || listener.accept_one_sync());
        std::thread::sleep(std::time::Duration::from_millis(50));
        let ns = name
            .to_ns_name::<GenericNamespaced>()
            .expect("nombre válido");
        let mut conn = Stream::connect(ns).expect("conectar");
        writeln!(conn, "ok.png").expect("línea normal");
        writeln!(conn, "{}", "x".repeat(MAX_INSTANCE_LINE_BYTES + 1)).expect("línea gigante");
        // Tras el corte el servidor ya cerró: este envío puede fallar con
        // `BrokenPipe` y es parte de lo esperado.
        let _ = writeln!(conn, "late.png");
        drop(conn);
        let received = handle.join().expect("hilo");
        // Solo la primera: al primer exceso se corta sin seguir leyendo.
        assert_eq!(received, vec!["ok.png".to_owned()]);
    }

    #[test]
    fn silent_connection_does_not_block_later_ones() {
        let name = test_socket_name("held");
        let primary = acquire_instance_with_name(&name, &[]);
        let InstanceRole::Primary(listener) = primary else {
            panic!("debe ser Primary");
        };
        let (tx, rx) = std::sync::mpsc::channel();
        listener.spawn_accept_loop(move |line| {
            let _ = tx.send(line);
        });
        // Una conexión que se queda abierta sin hablar no debe anclar el
        // aceptador: la siguiente apertura llega igual.
        let ns = name
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .expect("nombre válido");
        let _held = Stream::connect(ns).expect("conectar");
        std::thread::sleep(std::time::Duration::from_millis(50));
        let role = acquire_instance_with_name(&name, &[PathBuf::from("b.png")]);
        assert!(matches!(role, InstanceRole::Secondary));
        let got = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("la segunda conexión llega");
        assert_eq!(got, "b.png");
    }

    #[test]
    fn silent_connection_is_released_by_idle_timeout() {
        use std::time::{Duration, Instant};
        let name = test_socket_name("idle");
        let primary = acquire_instance_with_name(&name, &[]);
        let InstanceRole::Primary(listener) = primary else {
            panic!("debe ser Primary");
        };
        // Bombeo con un plazo corto: la conexión muda debe soltarse sola
        // en vez de anclar el hilo.
        let handle = std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Ok(conn) = listener.listener.accept() {
                super::pump_lines_with_timeout(
                    conn,
                    |line| out.push(line),
                    Duration::from_millis(200),
                );
            }
            out
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let ns = name
            .to_ns_name::<GenericNamespaced>()
            .expect("nombre válido");
        let _held = Stream::connect(ns).expect("conectar");
        let start = Instant::now();
        let received = handle.join().expect("hilo");
        assert!(received.is_empty());
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "la conexión muda debe soltarse por plazo, no quedarse colgada"
        );
    }

    #[test]
    fn after_primary_drops_a_new_instance_becomes_primary() {
        let name = test_socket_name("recycle");
        {
            let role = acquire_instance_with_name(&name, &[]);
            assert!(matches!(role, InstanceRole::Primary(_)));
            // drop aquí: el socket se libera.
        }
        // Pequeña pausa para que el SO libere el socket.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let role = acquire_instance_with_name(&name, &[]);
        assert!(
            matches!(role, InstanceRole::Primary(_)),
            "tras liberar el socket, una nueva instancia debe ser Primary"
        );
    }
}
