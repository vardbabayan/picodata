use pgwire::error::{ErrorInfo, PgWireError};
use std::io;
use thiserror::Error;

pub type PgResult<T> = Result<T, PgError>;

/// See https://www.postgresql.org/docs/current/errcodes-appendix.html.
#[derive(Error, Debug)]
pub enum PgError {
    #[error("protocol violation: {0}")]
    ProtocolViolation(String),

    #[error("feature is not supported: {0}")]
    FeatureNotSupported(String),

    #[error("authentication failed for user '{0}'")]
    InvalidPassword(String),

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    #[error("encoding error: {0}")]
    EncodingError(String),

    #[error("pgwire error: {0}")]
    PgWireError(#[from] PgWireError),

    #[error("lua error: {0}")]
    TarantoolError(#[from] tarantool::tlua::LuaError),

    #[error("json error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Build error info from PgError.
impl PgError {
    pub fn info(&self) -> ErrorInfo {
        ErrorInfo::new(
            "ERROR".to_string(),
            self.code().to_string(),
            self.to_string(),
        )
    }
}

impl PgError {
    fn code(&self) -> &str {
        use PgError::*;
        match self {
            ProtocolViolation(_) => "08P01",
            FeatureNotSupported(_) => "0A000",
            InvalidPassword(_) => "28P01",
            IoError(_) => "58030",
            // TODO: make the code depending on the error kind
            _otherwise => "XX000",
        }
    }
}