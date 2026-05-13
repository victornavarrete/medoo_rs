#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Postgres,
    MySql,
    Sqlite,
}

impl Backend {
    /// Devuelve el placeholder para la posición `idx` (1-based).
    pub fn placeholder(&self, idx: usize) -> String {
        match self {
            Backend::Postgres => format!("${}", idx),
            Backend::MySql | Backend::Sqlite => "?".to_string(),
        }
    }

    /// Cita un identificador (columna/tabla) ya validado.
    pub fn quote_ident(&self, name: &str) -> String {
        match self {
            Backend::MySql => format!("`{}`", name),
            _ => format!("\"{}\"", name),
        }
    }
}
