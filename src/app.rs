//! Application facade. The implementation lives in focused modules so startup,
//! UI chrome, image workflows and Win32 adapters can evolve independently.

mod controller;

pub use controller::{fatal_error, run};
