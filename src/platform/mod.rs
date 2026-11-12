//! Small Win32 adapters. Keeping these details out of the application model makes
//! platform behavior independently testable and keeps `app` focused on workflow.

pub mod clipboard;
pub mod conversion_dialog;
pub mod imgbb;
pub mod menu;
pub mod properties_dialog;
pub mod screenshot;
pub mod text;
pub mod theme;
pub mod updater;
