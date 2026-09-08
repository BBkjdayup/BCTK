use std::{error::Error, fmt, io};

use super::Diagnostic;

pub type DocxResult<T> = Result<T, DocxError>;

#[derive(Debug)]
pub enum DocxError {
    Io(io::Error),
    Zip(zip::result::ZipError),
    Xml {
        part_name: String,
        position: u64,
        message: String,
    },
    LimitExceeded {
        resource: String,
        limit: u64,
        actual: u64,
    },
    Rejected {
        diagnostics: Vec<Diagnostic>,
    },
}

impl DocxError {
    pub(crate) fn xml(
        part_name: impl Into<String>,
        position: u64,
        error: impl fmt::Display,
    ) -> Self {
        Self::Xml {
            part_name: part_name.into(),
            position,
            message: error.to_string(),
        }
    }

    pub(crate) fn rejected(diagnostics: Vec<Diagnostic>) -> Self {
        Self::Rejected { diagnostics }
    }
}

impl fmt::Display for DocxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "DOCX filesystem error: {error}"),
            Self::Zip(error) => write!(formatter, "invalid DOCX ZIP package: {error}"),
            Self::Xml {
                part_name,
                position,
                message,
            } => write!(
                formatter,
                "invalid XML in {part_name} at byte {position}: {message}"
            ),
            Self::LimitExceeded {
                resource,
                limit,
                actual,
            } => write!(
                formatter,
                "DOCX resource limit exceeded for {resource}: limit {limit}, actual {actual}"
            ),
            Self::Rejected { diagnostics } => write!(
                formatter,
                "DOCX package rejected with {} diagnostic error(s)",
                diagnostics
                    .iter()
                    .filter(|item| item.severity == super::DiagnosticSeverity::Error)
                    .count()
            ),
        }
    }
}

impl Error for DocxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Zip(error) => Some(error),
            Self::Xml { .. } | Self::LimitExceeded { .. } | Self::Rejected { .. } => None,
        }
    }
}

impl From<io::Error> for DocxError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<zip::result::ZipError> for DocxError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Zip(error)
    }
}
