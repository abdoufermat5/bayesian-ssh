//! TUI (Terminal User Interface) module for interactive connection browsing
//!
//! Provides a full-screen terminal interface for browsing, searching,
//! and connecting to SSH servers.
//!
//! ## Module layout
//!
//! - `models`     — Enums, small types (`Tab`, `AppMode`, `EditState`, …)
//! - `state`      — `App` struct and all state-management methods
//! - `input`      — Keyboard input handlers (dispatched per tab/mode)
//! - `event_loop` — Terminal setup/teardown and the main loop (`run_tui`)
//! - `ui/`        — All rendering code, split into sub-modules

pub mod event_loop;
pub mod input;
pub mod models;
pub mod state;
pub mod ui;
