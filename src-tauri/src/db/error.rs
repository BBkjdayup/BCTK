use std::{error::Error, fmt, io, time::SystemTimeError};

pub type DatabaseResult<T> = Result<T, DatabaseError>;

#[derive(Debug)]
pub enum DatabaseError {
    Io(io::Error),
    Sqlx(sqlx::Error),
    Migration(sqlx::migrate::MigrateError),
    SystemClock(SystemTimeError),
    UnexpectedApplicationId { expected: i64, found: i64 },
    UnrecognizedDatabase,
    IntegrityCheck(String),
    MigrationSwitch(String),
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "database filesystem error: {error}"),
            Self::Sqlx(error) => write!(formatter, "SQLite error: {error}"),
            Self::Migration(error) => write!(formatter, "database migration failed: {error}"),
            Self::SystemClock(error) => write!(formatter, "system clock error: {error}"),
            Self::UnexpectedApplicationId { expected, found } => write!(
                formatter,
                "the selected SQLite file belongs to another application (expected {expected}, found {found})"
            ),
            Self::UnrecognizedDatabase => write!(
                formatter,
                "the selected non-empty SQLite file is not a Zhitiku database"
            ),
            Self::IntegrityCheck(message) => {
                write!(formatter, "database integrity check failed: {message}")
            }
            Self::MigrationSwitch(message) => {
                write!(formatter, "database migration switch failed: {message}")
            }
        }
    }
}

impl Error for DatabaseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlx(error) => Some(error),
            Self::Migration(error) => Some(error),
            Self::SystemClock(error) => Some(error),
            Self::UnexpectedApplicationId { .. }
            | Self::UnrecognizedDatabase
            | Self::IntegrityCheck(_)
            | Self::MigrationSwitch(_) => None,
        }
    }
}

impl From<io::Error> for DatabaseError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<sqlx::Error> for DatabaseError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}

impl From<sqlx::migrate::MigrateError> for DatabaseError {
    fn from(error: sqlx::migrate::MigrateError) -> Self {
        Self::Migration(error)
    }
}

impl From<SystemTimeError> for DatabaseError {
    fn from(error: SystemTimeError) -> Self {
        Self::SystemClock(error)
    }
}
