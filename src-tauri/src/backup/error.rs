use std::{error::Error, fmt, io};

pub type BackupResult<T> = Result<T, BackupError>;

#[derive(Debug)]
pub enum BackupError {
    Io {
        context: String,
        source: io::Error,
    },
    Zip(zip::result::ZipError),
    Json(serde_json::Error),
    InvalidInput(String),
    Rejected(String),
    LimitExceeded {
        resource: String,
        limit: u64,
        actual: u64,
    },
    UnsupportedVersion {
        found: u32,
        supported: u32,
    },
    Integrity {
        path: String,
        reason: String,
    },
}

impl BackupError {
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub(crate) fn rejected(message: impl Into<String>) -> Self {
        Self::Rejected(message.into())
    }

    pub(crate) fn integrity(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Integrity {
            path: path.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { context, source } => write!(formatter, "{context}: {source}"),
            Self::Zip(error) => write!(formatter, "invalid .tqb ZIP archive: {error}"),
            Self::Json(error) => write!(formatter, "invalid backup manifest JSON: {error}"),
            Self::InvalidInput(message) => write!(formatter, "invalid backup input: {message}"),
            Self::Rejected(message) => write!(formatter, "backup archive rejected: {message}"),
            Self::LimitExceeded {
                resource,
                limit,
                actual,
            } => write!(
                formatter,
                "backup safety limit exceeded for {resource}: limit {limit} bytes, actual {actual} bytes"
            ),
            Self::UnsupportedVersion { found, supported } => write!(
                formatter,
                "unsupported backup manifest version {found}; this application supports version {supported}"
            ),
            Self::Integrity { path, reason } => {
                write!(
                    formatter,
                    "backup integrity check failed for {path}: {reason}"
                )
            }
        }
    }
}

impl Error for BackupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Zip(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::InvalidInput(_)
            | Self::Rejected(_)
            | Self::LimitExceeded { .. }
            | Self::UnsupportedVersion { .. }
            | Self::Integrity { .. } => None,
        }
    }
}

impl From<zip::result::ZipError> for BackupError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Zip(error)
    }
}

impl From<serde_json::Error> for BackupError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
