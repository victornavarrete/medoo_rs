use crate::backend::Backend;
use crate::cond::Binder;
use crate::error::{QueryError, Result};
use crate::ident;
use crate::log::{LogCategory, Query};
use crate::value::{IntoValue, Value};

#[derive(Debug, Clone)]
pub enum ConflictAction {
    /// `DO NOTHING` (PG/SQLite) o `INSERT IGNORE` (MySQL).
    Nothing,
    /// `DO UPDATE SET col = EXCLUDED.col` (PG/SQLite) o
    /// `ON DUPLICATE KEY UPDATE col = VALUES(col)` (MySQL). Las columnas
    /// listadas se sobrescriben con el valor que vino en el INSERT.
    Update(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct InsertQuery {
    backend: Backend,
    table: String,
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    returning: Vec<String>,
    conflict_cols: Vec<String>,    // ignorado en MySQL
    conflict_action: Option<ConflictAction>,
}

impl InsertQuery {
    pub fn new(backend: Backend, table: &str) -> Self {
        Self {
            backend,
            table: table.to_string(),
            columns: Vec::new(),
            rows: Vec::new(),
            returning: Vec::new(),
            conflict_cols: Vec::new(),
            conflict_action: None,
        }
    }

    /// Especifica las columnas del `ON CONFLICT (...)` (PG/SQLite).
    /// MySQL las ignora — usa todos los UNIQUE/PK constraints automáticamente.
    pub fn on_conflict<I: IntoIterator<Item = S>, S: Into<String>>(mut self, cols: I) -> Self {
        self.conflict_cols = cols.into_iter().map(|s| s.into()).collect();
        self
    }
    /// `DO NOTHING` / `INSERT IGNORE`. No requiere `on_conflict` en MySQL.
    pub fn do_nothing(mut self) -> Self {
        self.conflict_action = Some(ConflictAction::Nothing);
        self
    }
    /// Sobrescribe estas columnas con los valores del INSERT al chocar
    /// con un constraint único. PG/SQLite necesita `on_conflict` previo.
    pub fn do_update<I: IntoIterator<Item = S>, S: Into<String>>(mut self, cols: I) -> Self {
        self.conflict_action = Some(ConflictAction::Update(
            cols.into_iter().map(|s| s.into()).collect(),
        ));
        self
    }

    /// Inserta una fila a partir de pares (columna, valor). El primer
    /// `set` fija las columnas; los siguientes deben coincidir.
    pub fn set<C: Into<String>, V: IntoValue>(mut self, pairs: Vec<(C, V)>) -> Self {
        let (cols, vals): (Vec<_>, Vec<_>) = pairs
            .into_iter()
            .map(|(c, v)| (c.into(), v.into_value()))
            .unzip();
        if self.columns.is_empty() {
            self.columns = cols;
        }
        self.rows.push(vals);
        self
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// `RETURNING col1, col2, ...`. Soportado en Postgres, SQLite 3.35+
    /// y MariaDB 10.5+. MySQL no lo soporta — el motor rechazará al ejecutar.
    /// Pasar `["*"]` para todas las columnas.
    pub fn returning<I: IntoIterator<Item = S>, S: Into<String>>(mut self, cols: I) -> Self {
        self.returning = cols.into_iter().map(|s| s.into()).collect();
        self
    }

    pub fn to_sql(&self) -> Result<(String, Vec<Value>)> {
        if self.columns.is_empty() || self.rows.is_empty() {
            return Err(QueryError::EmptyRecord);
        }
        let mut b = Binder::new(self.backend);
        let qcols: Result<Vec<String>> = self
            .columns
            .iter()
            .map(|c| ident::quote(self.backend, c))
            .collect();
        let qcols = qcols?;
        let mut row_groups = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            if row.len() != self.columns.len() {
                return Err(QueryError::BindMismatch {
                    expected: self.columns.len(),
                    got: row.len(),
                });
            }
            let phs: Vec<String> = row.iter().map(|v| b.push(v.clone())).collect();
            row_groups.push(format!("({})", phs.join(", ")));
        }
        // Prefijo: INSERT vs INSERT IGNORE (MySQL + DO NOTHING)
        let prefix = if self.backend == Backend::MySql
            && matches!(self.conflict_action, Some(ConflictAction::Nothing))
        {
            "INSERT IGNORE INTO"
        } else {
            "INSERT INTO"
        };
        let mut sql = format!(
            "{} {} ({}) VALUES {}",
            prefix,
            ident::quote(self.backend, &self.table)?,
            qcols.join(", "),
            row_groups.join(", ")
        );

        // Cláusula de conflicto
        if let Some(action) = &self.conflict_action {
            match (self.backend, action) {
                (Backend::MySql, ConflictAction::Nothing) => { /* ya en prefix */ }
                (Backend::MySql, ConflictAction::Update(cols)) => {
                    let mut parts = Vec::with_capacity(cols.len());
                    for c in cols {
                        let qc = ident::quote(self.backend, c)?;
                        parts.push(format!("{} = VALUES({})", qc, qc));
                    }
                    sql.push_str(" ON DUPLICATE KEY UPDATE ");
                    sql.push_str(&parts.join(", "));
                }
                (Backend::Postgres | Backend::Sqlite, ConflictAction::Nothing) => {
                    sql.push_str(" ON CONFLICT");
                    if !self.conflict_cols.is_empty() {
                        let qcols: Result<Vec<String>> = self
                            .conflict_cols
                            .iter()
                            .map(|c| ident::quote(self.backend, c))
                            .collect();
                        sql.push_str(&format!(" ({})", qcols?.join(", ")));
                    }
                    sql.push_str(" DO NOTHING");
                }
                (Backend::Postgres | Backend::Sqlite, ConflictAction::Update(cols)) => {
                    if self.conflict_cols.is_empty() {
                        return Err(QueryError::InvalidIdentifier(
                            "on_conflict requerido en PG/SQLite con do_update".into(),
                        ));
                    }
                    let qconfl: Result<Vec<String>> = self
                        .conflict_cols
                        .iter()
                        .map(|c| ident::quote(self.backend, c))
                        .collect();
                    let mut parts = Vec::with_capacity(cols.len());
                    for c in cols {
                        let qc = ident::quote(self.backend, c)?;
                        parts.push(format!("{} = EXCLUDED.{}", qc, qc));
                    }
                    sql.push_str(&format!(
                        " ON CONFLICT ({}) DO UPDATE SET {}",
                        qconfl?.join(", "),
                        parts.join(", ")
                    ));
                }
            }
        }

        if !self.returning.is_empty() {
            let parts: Vec<String> = self.returning.iter().map(|c| {
                if c == "*" { Ok("*".to_string()) } else { ident::quote(self.backend, c) }
            }).collect::<Result<_>>()?;
            sql.push_str(" RETURNING ");
            sql.push_str(&parts.join(", "));
        }
        Ok((sql, b.into_params()))
    }
}

impl Query for InsertQuery {
    fn category(&self) -> LogCategory {
        LogCategory::INSERT
    }
    fn build_sql(&self) -> Result<(String, Vec<Value>)> {
        self.to_sql()
    }
}
