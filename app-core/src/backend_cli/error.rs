use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendCliError {
    ExecutableMissing(PathBuf),
    SpawnFailed(String),
    UnexpectedExit(String),
    ProtocolMismatch(String),
    ContractMismatch(String),
    StdoutPollution(String),
    MalformedFrame(String),
    FrameTooLarge {
        limit: usize,
    },
    RequestIdMismatch {
        expected: String,
        actual: Option<String>,
    },
    Domain {
        code: String,
        message: String,
        retryable: bool,
        request_id: Option<String>,
        capability: Option<String>,
        resource: Option<String>,
    },
    Io(String),
}

impl fmt::Display for BackendCliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExecutableMissing(path) => write!(
                formatter,
                "backend executable is missing: {}",
                path.display()
            ),
            Self::SpawnFailed(message) => {
                write!(formatter, "could not start backend process: {message}")
            }
            Self::UnexpectedExit(message) => {
                write!(formatter, "backend process exited unexpectedly: {message}")
            }
            Self::ProtocolMismatch(message) => {
                write!(formatter, "backend protocol mismatch: {message}")
            }
            Self::ContractMismatch(message) => {
                write!(formatter, "backend contract mismatch: {message}")
            }
            Self::StdoutPollution(message) => write!(
                formatter,
                "backend stdout was not a machine frame: {message}"
            ),
            Self::MalformedFrame(message) => {
                write!(formatter, "backend emitted a malformed frame: {message}")
            }
            Self::FrameTooLarge { limit } => {
                write!(formatter, "backend frame exceeds the {limit} byte limit")
            }
            Self::RequestIdMismatch { expected, actual } => write!(
                formatter,
                "backend request_id mismatch: expected {expected}, got {}",
                actual.as_deref().unwrap_or("<missing>")
            ),
            Self::Domain { code, message, .. } => write!(formatter, "{code}: {message}"),
            Self::Io(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for BackendCliError {}

impl From<std::io::Error> for BackendCliError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}
