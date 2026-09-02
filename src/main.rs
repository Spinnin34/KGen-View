#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod catalog;
mod commands;
mod i18n;
mod imaging;
mod platform;
mod settings;
mod ui;

fn main() {
    if let Err(error) = app::run() {
        app::fatal_error(&error.to_string());
    }
}
