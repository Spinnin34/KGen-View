#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod catalog;
mod commands;
mod diagnostics;
mod i18n;
mod imaging;
mod mailbox;
mod mcp;
mod platform;
mod resources;
mod settings;
mod ui;

fn main() {
    if std::env::args().any(|arg| arg == "--capture-stop") {
        platform::screenshot::stop_background();
        return;
    }
    if std::env::args().any(|arg| arg == "--capture-background") {
        if let Err(error) = platform::screenshot::run_background() {
            if !std::env::args().any(|arg| arg == "--quiet") {
                app::fatal_error(&error);
            }
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = diagnostics::benchmark_cli() {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = app::run() {
        app::fatal_error(&error.to_string());
    }
}
