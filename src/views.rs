//! Vistas: `CREATE VIEW`, `DROP VIEW`, `CREATE MATERIALIZED VIEW` (PG).

use crate::backend::Backend;
use crate::cond::Binder;
use crate::error::{QueryError, Result};
use crate::ident;
use crate::select::SelectQuery;
use crate::value::Value;

#[derive(Debug, Clone)]
pub struct CreateView {
    backend: Backend,
    name: String,
    or_replace: bool,
    if_not_exists: bool,
    materialized: bool,
    columns: Vec<String>,
    select: Option<Box<SelectQuery>>,
}

impl CreateView {
    pub fn new(backend: Backend, name: &str) -> Self {
        Self {
            backend,
            name: name.to_string(),
            or_replace: false,
            if_not_exists: false,
            materialized: false,
            columns: Vec::new(),
            select: None,
        }
    }
    /// `CREATE OR REPLACE VIEW`. Soportado en PG y MySQL. SQLite no.
    pub fn or_replace(mut self) -> Self {
        self.or_replace = true;
        self
    }
    /// `CREATE VIEW IF NOT EXISTS`. Soportado en SQLite y MariaDB. PG y
    /// MySQL clásico no — preferí `or_replace` ahí.
    pub fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }
    /// PG: `CREATE MATERIALIZED VIEW`. En otros backends → error.
    pub fn materialized(mut self) -> Self {
        self.materialized = true;
        self
    }
    /// Renombrar columnas: `CREATE VIEW v (c1, c2, ...) AS SELECT ...`.
    /// Útil cuando el SELECT tiene aliases complejos.
    pub fn columns<I: IntoIterator<Item = S>, S: Into<String>>(mut self, cols: I) -> Self {
        self.columns = cols.into_iter().map(|s| s.into()).collect();
        self
    }
    pub fn as_select(mut self, q: SelectQuery) -> Self {
        self.select = Some(Box::new(q));
        self
    }

    pub fn to_sql(&self) -> Result<(String, Vec<Value>)> {
        ident::validate(&self.name)?;
        let select = self.select.as_ref().ok_or_else(|| {
            QueryError::InvalidIdentifier(".as_select(query) requerido".into())
        })?;

        if self.materialized && self.backend != Backend::Postgres {
            return Err(QueryError::InvalidOperator(
                "MATERIALIZED VIEW solo en Postgres".to_string(),
            ));
        }
        if self.or_replace && self.backend == Backend::Sqlite {
            return Err(QueryError::InvalidOperator(
                "SQLite no soporta CREATE OR REPLACE VIEW".to_string(),
            ));
        }
        if self.if_not_exists && self.backend == Backend::Postgres {
            return Err(QueryError::InvalidOperator(
                "Postgres no soporta CREATE VIEW IF NOT EXISTS — usá or_replace".to_string(),
            ));
        }

        let mut head = String::from("CREATE");
        if self.or_replace {
            head.push_str(" OR REPLACE");
        }
        if self.materialized {
            head.push_str(" MATERIALIZED");
        }
        head.push_str(" VIEW");
        if self.if_not_exists {
            head.push_str(" IF NOT EXISTS");
        }

        let mut b = Binder::new(self.backend);
        let select_sql = select.render_into(&mut b)?;

        let mut sql = format!("{} {}", head, self.backend.quote_ident(&self.name));
        if !self.columns.is_empty() {
            let cols: Result<Vec<String>> = self
                .columns
                .iter()
                .map(|c| {
                    ident::validate(c)?;
                    Ok(self.backend.quote_ident(c))
                })
                .collect();
            sql.push_str(&format!(" ({})", cols?.join(", ")));
        }
        sql.push_str(" AS ");
        sql.push_str(&select_sql);
        Ok((sql, b.into_params()))
    }
}

#[derive(Debug, Clone)]
pub struct DropView {
    backend: Backend,
    name: String,
    if_exists: bool,
    cascade: bool,
    materialized: bool,
}

impl DropView {
    pub fn new(backend: Backend, name: &str) -> Self {
        Self {
            backend,
            name: name.to_string(),
            if_exists: false,
            cascade: false,
            materialized: false,
        }
    }
    pub fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }
    pub fn cascade(mut self) -> Self {
        self.cascade = true;
        self
    }
    pub fn materialized(mut self) -> Self {
        self.materialized = true;
        self
    }
    pub fn to_sql(&self) -> Result<String> {
        ident::validate(&self.name)?;
        if self.materialized && self.backend != Backend::Postgres {
            return Err(QueryError::InvalidOperator(
                "MATERIALIZED VIEW solo en Postgres".to_string(),
            ));
        }
        let mut sql = String::from("DROP");
        if self.materialized {
            sql.push_str(" MATERIALIZED");
        }
        sql.push_str(" VIEW");
        if self.if_exists {
            sql.push_str(" IF EXISTS");
        }
        sql.push(' ');
        sql.push_str(&self.backend.quote_ident(&self.name));
        if self.cascade && self.backend == Backend::Postgres {
            sql.push_str(" CASCADE");
        }
        Ok(sql)
    }
}
