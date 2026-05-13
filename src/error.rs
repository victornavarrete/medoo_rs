use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum QueryError {
    InvalidIdentifier(String),
    InvalidOperator(String),
    EmptyInList(String),
    EmptyRecord,
    MissingWhere(&'static str),
    UnresolvedTemplate(String),
    BindMismatch { expected: usize, got: usize },
    /// Error del driver async (sqlx u otro), envuelto como string para
    /// no atar la API al tipo concreto.
    Driver(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::InvalidIdentifier(s) => write!(f, "invalid identifier: {:?}", s),
            QueryError::InvalidOperator(s) => write!(f, "invalid operator: {:?}", s),
            QueryError::EmptyInList(c) => write!(f, "IN list for {:?} is empty", c),
            QueryError::EmptyRecord => write!(f, "record has no columns"),
            QueryError::MissingWhere(op) => {
                write!(f, "{} requires WHERE (safety guard); call .allow_full_table()", op)
            }
            QueryError::UnresolvedTemplate(t) => write!(f, "unresolved template token: {}", t),
            QueryError::BindMismatch { expected, got } => {
                write!(f, "bind mismatch: expected {} got {}", expected, got)
            }
            QueryError::Driver(s) => write!(f, "driver error: {}", s),
        }
    }
}

impl std::error::Error for QueryError {}

pub type Result<T> = std::result::Result<T, QueryError>;
