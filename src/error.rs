use std::fmt;
use std::io;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Vroom(String),
    Io(io::Error),
    Allocation(String),
    Mmap { error: String, io_error: io::Error },
    Ioctl { error: String, io_error: io::Error },
    Vfio(String),
    Mmio(String),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "IO Error: {error}"),
            Self::Vroom(error) => write!(f, "Custom Error: {error}"),
            Self::Allocation(error) => write!(f, "Allocation Error: {error}"),
            Self::Mmap { error, io_error } => {
                write!(f, "Mmap failed Error: {error} OS error: {io_error}")
            }
            Self::Ioctl { error, io_error } => {
                write!(f, "Ioctl failed Error: {error} OS error: {io_error}")
            }
            Self::Vfio(error) => write!(f, "Vfio Error: {error}"),
            Self::Mmio(error) => write!(f, "Mmio Error: {error}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self::Vroom(value.to_string())
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::Vroom(value)
    }
}
