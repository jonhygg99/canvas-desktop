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
    let Ok(name) = SOCKET_NAME.to_ns_name::<GenericNamespaced>() else {
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
}
