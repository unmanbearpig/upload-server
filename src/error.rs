use std::fmt;
use std::io;

#[derive(Copy, Clone, Debug)]
pub enum ErrorKind {
    Success,
    ServerError,
    UserError,
    NotFound,
    Unknown,
}

impl ErrorKind {
    fn as_http_code(self) -> u16 {
        match self {
            ErrorKind::Success => 200,
            ErrorKind::ServerError => 500,
            ErrorKind::UserError => 400,
            ErrorKind::NotFound => 404,
            ErrorKind::Unknown => 500,
        }
    }

    fn description(self) -> &'static str {
        match self {
            ErrorKind::Success => "Success",
            ErrorKind::ServerError => "Server error",
            ErrorKind::UserError => "Client error",
            ErrorKind::NotFound => "Not found",
            ErrorKind::Unknown => "Unknown",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub msg: String,
}

impl Error {
    pub fn new<T: fmt::Display>(kind: ErrorKind, msg: T) -> Self {
        Error {
            kind,
            msg: msg.to_string(),
        }
    }

    pub fn from_io_error<T: AsRef<str>>(err: io::Error, description: T) -> Self {
        Error {
            kind: ErrorKind::ServerError,
            msg: format!("{}: {}", description.as_ref(), err),
        }
    }

    pub fn as_http_code(&self) -> u16 {
        self.kind.as_http_code()
    }

    pub fn as_html(&self) -> String {
        format!("{} ({}): {}",
                self.kind.as_http_code(), self.kind.description(), self.msg)

    }
}
