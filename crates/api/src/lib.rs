//! Wire types shared by the server, CLI, MCP tools and the Leptos UI.
//!
//! Amounts are integer minor units (paise/cents) in Rust and **decimal strings** on the wire (`"3240.50"`),
//! so no client does float maths on money. Inputs also accept JSON numbers.

pub mod loan;
pub mod money;
pub mod models;

pub use models::*;
