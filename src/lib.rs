//! HumanSSH - A fast, cross-platform SSH terminal
//!
//! This crate provides a GPU-accelerated terminal emulator with SSH support.

pub mod actions;
pub mod app;
pub mod config;
pub mod terminal;
pub mod theme;

// Test utilities module - only compiled during tests
#[cfg(test)]
pub mod test_utils;
