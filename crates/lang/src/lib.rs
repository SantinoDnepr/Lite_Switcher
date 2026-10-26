//! Language packs for the automatic switching: manifests, discovery, activation, data formats and
//! the key rendering table. No WinAPI here: the Windows specific parts live in the callers.

pub mod format;
pub mod manifest;
pub mod pack;
pub mod render;
