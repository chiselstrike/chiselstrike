use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[macro_export]
macro_rules! bad_request {
    ($($token:tt)*) => {
        return Err($crate::error::Error::bad_request(anyhow::anyhow!($($token)*)))
    };
}

#[macro_export]
macro_rules! forbidden {
    ($($token:tt)*) => {
        return Err($crate::error::Error::forbidden(anyhow::anyhow!($($token)*)))
    };
}

#[macro_export]
macro_rules! internal {
    ($($token:tt)*) => {
        return Err($crate::error::Error::internal(anyhow::anyhow!($($token)*)))
    };
}

#[derive(Debug)]
pub struct Error {
    pub inner: anyhow::Error,
    pub err_kind: ErrorKind,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let context = match self.err_kind {
            ErrorKind::Forbbiden => "forbidden",
            ErrorKind::BadRequest => "bad request",
            ErrorKind::Internal => "internal error",
        };

        write!(f, "{context}: {}", self.inner)
    }
}

#[derive(Debug)]
pub enum ErrorKind {
    Forbbiden,
    BadRequest,
    Internal,
}

impl std::error::Error for Error {}

pub trait ResultExt<T> {
    fn err_internal(self) -> Result<T>;
    fn err_forbidden(self) -> Result<T>;
    fn err_bad_request(self) -> Result<T>;
}

impl<T, E> ResultExt<T> for std::result::Result<T, E>
where
    E: Into<anyhow::Error>,
{
    fn err_internal(self) -> Result<T> {
        self.map_err(|e| Error::internal(e.into()))
    }

    fn err_forbidden(self) -> Result<T> {
        self.map_err(|e| Error::forbidden(e.into()))
    }

    fn err_bad_request(self) -> Result<T> {
        self.map_err(|e| Error::bad_request(e.into()))
    }
}

impl Error {
    pub fn internal(inner: anyhow::Error) -> Self {
        Self {
            inner,
            err_kind: ErrorKind::Internal,
        }
    }

    pub fn forbidden(inner: anyhow::Error) -> Self {
        Self {
            inner,
            err_kind: ErrorKind::Forbbiden,
        }
    }

    pub fn bad_request(inner: anyhow::Error) -> Self {
        Self {
            inner,
            err_kind: ErrorKind::BadRequest,
        }
    }
}
