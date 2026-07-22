//! Sonda del registro de asociaciones (solo Windows): registra, deja que el
//! llamador compruebe las claves, y con `--cleanup` desregistra.

fn main() {
    #[cfg(windows)]
    {
        use canvas_shell::ShellIntegration as _;
        let cleanup = std::env::args().any(|a| a == "--cleanup");
        let jumplist = std::env::args().any(|a| a == "--jumplist");
        let shell = canvas_shell::platform();
        if jumplist {
            canvas_shell::set_app_identity();
            let recents = vec![
                std::path::PathBuf::from("C:/Windows/Web/Wallpaper"),
                std::path::PathBuf::from("C:/foto-de-prueba.png"),
            ];
            match shell.update_jump_list(&recents) {
                Ok(()) => println!("JUMPLIST=ok"),
                Err(e) => println!("JUMPLIST=err {e}"),
            }
            return;
        }
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
