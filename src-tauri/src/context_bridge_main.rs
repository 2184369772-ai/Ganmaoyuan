//! Console-only entrypoint for the local Ganmaoyuan Context Bridge.
//!
//! The desktop application deliberately uses the Windows GUI subsystem, which
//! is unsuitable for a stdio MCP server. Keeping this executable separate
//! gives MCP clients a reliable stdin/stdout channel without changing the GUI.

fn main() {
    match app_lib::context_bridge::run_from_process_args() {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("Ganmaoyuan Context Bridge requires --context-bridge --project-root <path>.");
            std::process::exit(2);
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
