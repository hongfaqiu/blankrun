//! NightBlank 核心逻辑：与平台无关，可在任何主机上 `cargo test`。
pub mod config;
pub mod schedule;
pub mod state;
pub mod wake;

pub use config::Config;
pub use state::{Event, State, StateMachine};
