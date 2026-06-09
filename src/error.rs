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
                args,
                code,
                stderr,
            } => {
                let rendered = std::iter::once(program.as_str())
                    .chain(args.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                let status = code.map_or_else(|| "signal".to_string(), |c| c.to_string());
                let detail = stderr.trim();
                if detail.is_empty() {
                    write!(f, "command failed with status {status}: {rendered}")
                } else {
                    write!(
                        f,
                        "command failed with status {status}: {rendered}: {detail}"
                    )
                }
            }
            Error::DoctorFailed(message) => write!(f, "{message}"),
        }
    }
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
