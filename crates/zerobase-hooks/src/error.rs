//! Error types for the JS hooks subsystem.

use std::fmt;

/// Errors that can occur when loading or executing JS hooks.
#[derive(Debug)]
pub enum JsHookError {
    /// Failed to read a hook file from disk.
    FileRead {
        path: String,
        source: std::io::Error,
    },
    /// JavaScript evaluation/execution error.
    JsEval {
        file: String,
        message: String,
    },
    /// A JS callback returned / threw an error.
    JsCallback {
        hook_name: String,
        message: String,
    },
    /// Invalid hook registration (e.g. missing callback or event name).
    InvalidRegistration {
        message: String,
    },
    /// File watcher error.
    Watcher(String),
}

impl fmt::Display for JsHookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileRead { path, source } => {
                write!(f, "failed to read hook file '{path}': {source}")
            }
            Self::JsEval { file, message } => {
                write!(f, "JS evaluation error in '{file}': {message}")
            }
            Self::JsCallback { hook_name, message } => {
                write!(f, "JS hook '{hook_name}' error: {message}")
            }
            Self::InvalidRegistration { message } => {
                write!(f, "invalid hook registration: {message}")
            }
            Self::Watcher(msg) => write!(f, "file watcher error: {msg}"),
        }
    }
}

impl std::error::Error for JsHookError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FileRead { source, .. } => Some(source),
            _ => None,
        }
    }
}
