//! 2D pseudographic terminal.

// Allow use in embedded systems.
#![no_std]
// Prohibit unsafe code.
#![deny(unsafe_code)]

pub mod backend;
pub mod core;
pub mod render;
