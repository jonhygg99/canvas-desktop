//! Sonda del registro de asociaciones (solo Windows): registra, deja que el
//! llamador compruebe las claves, y con `--cleanup` desregistra.

fn main() {
    #[cfg(windows)]
    {
        use canvas_shell::ShellIntegration as _;
        let cleanup = std::env::args().any(|a| a == "--cleanup");
        let shell = canvas_shell::platform();
        if cleanup {
            match shell.unregister_file_associations() {
                Ok(()) => println!("UNREGISTER=ok"),
                Err(e) => println!("UNREGISTER=err {e}"),
            }
        } else {
            let exe = std::env::current_exe().expect("current_exe");
            match shell.register_file_associations(&exe) {
                Ok(()) => println!("REGISTER=ok"),
                Err(e) => println!("REGISTER=err {e}"),
            }
        }
    }
    #[cfg(not(windows))]
    println!("solo Windows");
}
