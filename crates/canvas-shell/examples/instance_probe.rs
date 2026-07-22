//! Sonda headless de la instancia única, sin GUI:
//! - sin argumentos: adquiere el lock, escucha 10 s e imprime lo que recibe.
//! - con argumentos: intenta adquirir; si ya hay primaria, le envía las rutas.

use std::sync::mpsc::channel;

fn main() {
    tracing_subscriber::fmt().init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths: Vec<std::path::PathBuf> = args.iter().map(std::path::PathBuf::from).collect();

    match canvas_shell::acquire_instance(&paths) {
        canvas_shell::InstanceRole::Primary(listener) => {
            println!("ROLE=primary");
            let (tx, rx) = channel();
            listener.spawn_accept_loop(move |line| {
                let _ = tx.send(line);
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                if let Ok(line) = rx.recv_timeout(std::time::Duration::from_millis(200)) {
                    println!("RECV={line}");
                }
            }
        }
        canvas_shell::InstanceRole::Secondary => println!("ROLE=secondary"),
        canvas_shell::InstanceRole::Standalone => println!("ROLE=standalone"),
    }
}
