//! Page chrome and helpers shared by every tool in the suite.
mod json;
mod shell;
mod time;

pub use json::script_json;
pub use shell::{Shell, Tool, page};
pub use time::{now_ms, relative_time};
