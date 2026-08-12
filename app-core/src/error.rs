use std::fmt;

#[derive(Debug)]
pub enum UtaStudioError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Other(String),
}

impl fmt::Display for UtaStudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "{e}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for UtaStudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Other(_) => None,
        }
    }
}

impl From<std::io::Error> for UtaStudioError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for UtaStudioError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl From<String> for UtaStudioError {
    fn from(s: String) -> Self {
        Self::Other(s)
    }
}

impl From<&str> for UtaStudioError {
    fn from(s: &str) -> Self {
        Self::Other(s.to_string())
    }
}
