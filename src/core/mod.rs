//! Core file, path, and validation logic. This module must not depend on egui.

// Temporary until the UI wires every core API in (Task 9 onward).
#![allow(dead_code)]

pub mod atomic;
pub mod config;
pub mod env;
pub mod launch;
pub mod layer;
pub mod model;
pub mod scale;
pub mod settings;
