//! Language packs for the automatic switching: manifests, discovery, activation, data formats and
//! the key rendering table. No WinAPI here: the Windows specific parts live in the callers.

pub mod build;
pub mod format;
pub mod literary;
pub mod manifest;
pub mod pack;
pub mod programming;
pub mod render;
