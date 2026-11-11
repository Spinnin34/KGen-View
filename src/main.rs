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
