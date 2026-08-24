//! Instancia única: la primera instancia toma un socket local con nombre
//! (named pipe en Windows) y escucha; las siguientes le envían sus rutas por
//! ese canal y salen con código 0. El proceso vivo las recibe como
//! `OpenPath`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

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

impl InstanceListener {
    /// Acepta conexiones en un hilo propio y entrega cada línea recibida al
    /// callback (una ruta por línea; línea vacía = «tráete la ventana»).
    pub fn spawn_accept_loop(self, on_line: impl Fn(String) + Send + 'static) {
        std::thread::spawn(move || loop {
            match self.listener.accept() {
                Ok(conn) => {
                    let reader = BufReader::new(conn);
                    for line in reader.lines().map_while(|l| l.ok()) {
                        on_line(line);
                    }
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
                let reader = BufReader::new(conn);
                reader.lines().map_while(|l| l.ok()).collect()
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
