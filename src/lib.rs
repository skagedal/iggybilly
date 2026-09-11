//! Library facade so integration tests (and the binary) share a single
//! compiled tree. Nothing in here is meant to be a public Rust API; the
//! module structure is just exposed so `tests/integration.rs` can call
//! `iggybilly::web::build_app`, `iggybilly::db::connect`, etc.

pub mod assets;
pub mod audio;
pub mod auth;
pub mod cli;
pub mod config;
pub mod datefmt;
pub mod db;
pub mod discord;
pub mod error;
pub mod handlers;
pub mod markdown;
pub mod models;
pub mod web;
