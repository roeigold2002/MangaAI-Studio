use serde::Serialize;

/// Error type returned to the frontend. `code` is stable and machine-readable;
/// `message` is shown to the user and must never contain secrets.
#[derive(Debug, thiserror::Error, Serialize)]
#[error("{code}: {message}")]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into() }
    }
    pub fn not_found(what: &str) -> Self {
        Self::new("not_found", format!("{what} not found"))
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid", message)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::not_found("record"),
            other => AppError::new("db", other.to_string()),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        let code = match e.raw_os_error() {
            Some(28) => "disk_full", // ENOSPC
            _ => "io",
        };
        AppError::new(code, e.to_string())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::new("internal", format!("{e:#}"))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::new("json", e.to_string())
    }
}

impl From<zip::result::ZipError> for AppError {
    fn from(e: zip::result::ZipError) -> Self {
        AppError::new("archive", format!("archive could not be read: {e}"))
    }
}

pub type AppResult<T> = Result<T, AppError>;
