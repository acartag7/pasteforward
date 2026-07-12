use std::fmt::{Display, Formatter};
use std::io;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    Usage(String),
    MissingDestination(String),
    DuplicateDestination(String),
    InvalidDestination(String),
    UnsupportedPlatform(String),
    CommandFailed {
        program: String,
        args: Vec<String>,
        code: Option<i32>,
        stderr: String,
    },
    CommandTimedOut {
        program: String,
        seconds: u64,
    },
    StateLockTimedOut {
        milliseconds: u64,
    },
    MalformedDaemonMarker,
    LimitExceeded(String),
    DoctorFailed(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "{err}"),
            Error::Json(err) => write!(f, "{err}"),
            Error::Usage(message) => write!(f, "{message}"),
            Error::MissingDestination(name) => write!(f, "destination not found: {name}"),
            Error::DuplicateDestination(name) => write!(f, "destination already exists: {name}"),
            Error::InvalidDestination(message) => write!(f, "{message}"),
            Error::UnsupportedPlatform(message) => write!(f, "{message}"),
            Error::CommandFailed {
                program,
                args: _,
                code,
                stderr,
            } => {
                let status = code.map_or_else(|| "signal".to_string(), |c| c.to_string());
                let detail = sanitize_command_detail(stderr);
                if detail.is_empty() {
                    write!(f, "command failed with status {status}: {program}")
                } else {
                    write!(
                        f,
                        "command failed with status {status}: {program}: {detail}"
                    )
                }
            }
            Error::CommandTimedOut { program, seconds } => {
                write!(f, "command timed out after {seconds}s: {program}")
            }
            Error::StateLockTimedOut { milliseconds } => {
                write!(f, "daemon state lock timed out after {milliseconds}ms")
            }
            Error::MalformedDaemonMarker => write!(f, "daemon state marker is malformed"),
            Error::LimitExceeded(message) => write!(f, "{message}"),
            Error::DoctorFailed(message) => write!(f, "{message}"),
        }
    }
}

fn sanitize_command_detail(value: &str) -> String {
    let mut detail = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(1024)
        .collect::<String>();
    detail.truncate(detail.trim_end().len());
    detail
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::Io(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::Json(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_details_are_single_line_and_bounded() {
        let detail = sanitize_command_detail(&format!("first\n{}", "x".repeat(2000)));
        assert!(!detail.contains('\n'));
        assert_eq!(detail.len(), 1024);
    }
}
