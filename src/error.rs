//! Error type shared by all brandi modules.

use std::fmt;

/// Convenient result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, BrandiError>;

/// All errors brandi can produce.
#[derive(Debug)]
pub enum BrandiError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    Json(serde_json::Error),
    Image(image::ImageError),
    Invalid(String),
    Daemon(String),
    NotFound(String),
    Network(String),
}

impl fmt::Display for BrandiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrandiError::Io(e) => write!(f, "io error: {e}"),
            BrandiError::Yaml(e) => write!(f, "yaml error: {e}"),
            BrandiError::Json(e) => write!(f, "json error: {e}"),
            BrandiError::Image(e) => write!(f, "image error: {e}"),
            BrandiError::Invalid(msg) => write!(f, "invalid: {msg}"),
            BrandiError::Daemon(msg) => write!(f, "daemon error: {msg}"),
            BrandiError::NotFound(msg) => write!(f, "not found: {msg}"),
            BrandiError::Network(msg) => write!(f, "network error: {msg}"),
        }
    }
}

impl std::error::Error for BrandiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BrandiError::Io(e) => Some(e),
            BrandiError::Yaml(e) => Some(e),
            BrandiError::Json(e) => Some(e),
            BrandiError::Image(e) => Some(e),
            BrandiError::Invalid(_)
            | BrandiError::Daemon(_)
            | BrandiError::NotFound(_)
            | BrandiError::Network(_) => None,
        }
    }
}

impl BrandiError {
    /// Stable support code for machine correlation and documentation lookup.
    pub fn code(&self) -> &'static str {
        match self {
            BrandiError::Io(_) => "BRD-IO-001",
            BrandiError::Yaml(_) => "BRD-YAML-001",
            BrandiError::Json(_) => "BRD-JSON-001",
            BrandiError::Image(_) => "BRD-IMAGE-001",
            BrandiError::Invalid(_) => "BRD-INPUT-001",
            BrandiError::Daemon(_) => "BRD-DAEMON-001",
            BrandiError::NotFound(_) => "BRD-NOTFOUND-001",
            BrandiError::Network(_) => "BRD-NETWORK-001",
        }
    }

    pub fn action(&self) -> &'static str {
        match self {
            BrandiError::Io(_) => "Check the path, permissions, and available storage, then retry.",
            BrandiError::Yaml(_) => "Correct the named YAML file and run its validation command.",
            BrandiError::Json(_) => {
                "Validate the JSON input or baseline against the documented schema."
            }
            BrandiError::Image(_) => {
                "Use a supported, decodable image within the configured safety limits."
            }
            BrandiError::Invalid(_) => {
                "Review the supplied value and the command help, then retry."
            }
            BrandiError::Daemon(_) => {
                "Inspect `brandi daemon status` and the daemon log before retrying."
            }
            BrandiError::NotFound(_) => "Verify the project root and required file or resource.",
            BrandiError::Network(_) => {
                "Check connectivity and configured credentials without printing secrets."
            }
        }
    }
}

impl From<std::io::Error> for BrandiError {
    fn from(e: std::io::Error) -> Self {
        BrandiError::Io(e)
    }
}

impl From<serde_yaml::Error> for BrandiError {
    fn from(e: serde_yaml::Error) -> Self {
        BrandiError::Yaml(e)
    }
}

impl From<serde_json::Error> for BrandiError {
    fn from(e: serde_json::Error) -> Self {
        BrandiError::Json(e)
    }
}

impl From<image::ImageError> for BrandiError {
    fn from(e: image::ImageError) -> Self {
        BrandiError::Image(e)
    }
}
