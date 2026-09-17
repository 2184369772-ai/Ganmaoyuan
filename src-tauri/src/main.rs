// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
fn handle_windows_shell_commands() -> Result<bool, String> {
    use std::{env, path::PathBuf};

    let mut args = env::args_os().skip(1);
    let Some(first) = args.next() else {
        return Ok(false);
    };
    let first = first.to_string_lossy().to_string();
    if first != "--register-sendto" && first != "--unregister-sendto" {
        return Ok(false);
    }
    let current_exe = env::current_exe()
        .map(PathBuf::from)
        .map_err(|err| format!("读取感冒院程序路径失败：{err}"))?;
    if first == "--register-sendto" {
        app_lib::shell_integration::register_sendto_shortcut(&current_exe)?;
    } else {
        app_lib::shell_integration::unregister_sendto_shortcut()?;
    }
    Ok(true)
}

fn main() {
    match app_lib::context_bridge::run_from_process_args() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    #[cfg(target_os = "windows")]
    match handle_windows_shell_commands() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    app_lib::run();
}
