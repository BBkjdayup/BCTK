use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

/// A stable, user-presentable package or template finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
    pub part_name: Option<String>,
    pub message: String,
    pub suggested_action: Option<String>,
}

impl Diagnostic {
    pub fn new(
        severity: DiagnosticSeverity,
        code: &'static str,
        part_name: Option<impl Into<String>>,
        message: impl Into<String>,
        suggested_action: Option<impl Into<String>>,
    ) -> Self {
        Self {
            severity,
            code,
            part_name: part_name.map(Into::into),
            message: message.into(),
            suggested_action: suggested_action.map(Into::into),
        }
    }

    pub fn error(
        code: &'static str,
        part_name: Option<impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            DiagnosticSeverity::Error,
            code,
            part_name,
            message,
            None::<String>,
        )
    }

    pub fn warning(
        code: &'static str,
        part_name: Option<impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            DiagnosticSeverity::Warning,
            code,
            part_name,
            message,
            None::<String>,
        )
    }
}
