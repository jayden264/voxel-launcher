//! launcher-core: everything a Minecraft launcher needs, with no UI dependency.

pub mod account;
pub mod backup;
pub mod config;
pub mod diagnostics;
pub mod doctor;
pub mod download;
pub mod error;
pub mod forge;
pub mod install;
pub mod java;
pub mod launch;
pub mod loader;
pub mod mirror;
pub mod modpack;
pub mod mods;
pub mod netcheck;
pub mod paths;
pub mod precheck;
pub mod recommended;
pub mod screenshots;
pub mod servers;
pub mod share;
pub mod stats;
pub mod updater;
pub mod version;
pub mod worlds;

pub use error::{Error, Result};
