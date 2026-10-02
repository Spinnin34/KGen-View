//! AppState implementation split by responsibility.
//!
//! The fragments are included into one module so private methods keep the
//! same visibility and behavior while the implementation stays easy to find.

use super::*;

include!("state.rs");
include!("render.rs");
include!("status.rs");
include!("loading.rs");
include!("navigation.rs");
include!("dialogs.rs");
include!("editing.rs");
include!("file_ops.rs");
include!("commands.rs");
include!("window.rs");
include!("utils.rs");
