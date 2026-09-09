//! brandi — brand coherence intelligence layer.
//!
//! Keeps a machine-readable brand definition in `.brandi/`, split into a
//! brief (who the product is and who it's for) and guidelines (how it
//! sounds, looks, and what language it forbids), and lints user-facing
//! surfaces of a software project against them.

pub mod adb_social;
pub mod assets;
pub mod automation;
pub mod brief;
pub mod cli;
pub mod commands;
pub mod creative;
pub mod daemon;
pub mod direct;
pub mod error;
pub mod evaluation;
pub mod guidelines;
pub mod output;
pub mod process;
pub mod propose;
pub mod report;
pub mod rules;
pub mod scan;
pub mod social;
pub mod surface;
pub mod tape;
pub mod target_use;
pub mod types;
