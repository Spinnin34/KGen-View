//! Window coordination, split into modules with explicit visibility.
use self::utils::*;
use super::*;
mod commands;
mod dialogs;
mod editing;
mod file_ops;
mod loading;
mod mcp;
mod navigation;
mod render;
mod state;
mod status;
mod utils;
mod view;
mod window;
pub use window::{fatal_error, run};
