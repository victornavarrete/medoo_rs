use crate::backend::Backend;
use crate::cond::{render_cond, Binder, Cond};
use crate::error::{QueryError, Result};
use crate::ident;
use crate::log::{LogCategory, Query};
use crate::value::{IntoValue, Value};

#[derive(Debug, Clone)]
struct UsingTable {
    table: String,
    on: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateQuery {
    backend: Backend,
    table: String,
    sets: Vec<(String, Value)>,
    using: Vec<UsingTable>,
    wheres: Vec<Cond>,
    allow_full: bool,
    returning: Vec<String>,
}

impl UpdateQuery {
    pub fn new(backend: Backend, table: &str) -> Self {
        Self {
            backend,
            table: table.to_string(),
            sets: Vec::new(),
            using: Vec::new(),
            wheres: Vec::new(),
            allow_full: false,
            returning: Vec::new(),
        }
    }
    /// `RETURNING col1, col2, ...`. PG / SQLite 3.35+ / MariaDB 10.5+.
    pub fn returning<I: IntoIterator<Item = S>, S: Into<String>>(mut self, cols: I) -> Self {
        self.returning = cols.into_iter().map(|s| s.into()).collect();
        self
    }

    pub fn set<C: Into<String>, V: IntoValue>(mut self, col: C, v: V) -> Self {
        self.sets.push((col.into(), v.into_value()));
        self
    }

    pub fn where_eq<C: Into<String>, V: IntoValue>(mut self, col: C, v: V) -> Self {
        self.wheres.push(Cond::eq(col, v));
        self
    }

    pub fn where_op<C: Into<String>, V: IntoValue>(mut self, col: C, op: &str, v: V) -> Self {
        match Cond::op(col, op, v) {
            Ok(c) => self.wheres.push(c),
            Err(e) => panic!("medoo_rs: where_op operador inválido: {}", e),
        }
        self
    }
    pub fn try_where_op<C: Into<String>, V: IntoValue>(
        mut self,
        col: C,
        op: &str,
        v: V,
    ) -> Result<Self> {
        self.wheres.push(Cond::op(col, op, v)?);
        Ok(self)
    }
    pub fn try_where_eq<C: Into<String>, V: IntoValue>(
        mut self,
        col: C,
        v: V,
    ) -> Result<Self> {
        self.wheres.push(Cond::eq(col, v));
        Ok(self)
    }

    pub fn where_cond(mut self, c: Cond) -> Self {
        self.wheres.push(c);
        self
    }

    /// Tabla extra en `UPDATE ... FROM ...` (PG/SQLite) o `UPDATE ... JOIN ...`
    /// (MySQL — requiere `using_join` con ON).
    pub fn using<T: Into<String>>(mut self, table: T) -> Self {
        self.using.push(UsingTable { table: table.into(), on: None });
        self
    }

    /// Tabla extra con condición de join.
    /// - PG/SQLite: agrega tabla a `FROM` y la condición pasa a `WHERE`.
    /// - MySQL: rendea como `INNER JOIN tabla ON ...`.
    pub fn using_join<T: Into<String>, O: Into<String>>(mut self, table: T, on: O) -> Self {
        self.using.push(UsingTable { table: table.into(), on: Some(on.into()) });
        self
    }

    /// Desactiva el guardia que prohíbe UPDATE sin WHERE.
    pub fn allow_full_table(mut self) -> Self {
        self.allow_full = true;
        self
    }

    pub fn to_sql(&self) -> Result<(String, Vec<Value>)> {
        if self.sets.is_empty() {
            return Err(QueryError::EmptyRecord);
        }
        if self.wheres.is_empty() && self.using.is_empty() && !self.allow_full {
            return Err(QueryError::MissingWhere("UPDATE"));
        }
        let mut b = Binder::new(self.backend);
        let qtable = ident::quote(self.backend, &self.table)?;

        let mut set_parts = Vec::with_capacity(self.sets.len());
        for (c, v) in &self.sets {
            let qc = ident::quote(self.backend, c)?;
            let ph = b.push(v.clone());
            set_parts.push(format!("{} = {}", qc, ph));
        }
        let set_sql = set_parts.join(", ");

        let mut sql = match self.backend {
            Backend::MySql if !self.using.is_empty() => {
                let mut s = format!("UPDATE {}", qtable);
                for u in &self.using {
                    let on = u.on.as_deref().ok_or_else(|| QueryError::InvalidOperator(
                        "MySQL: using requiere ON (usá using_join)".to_string(),
                    ))?;
                    s.push_str(" INNER JOIN ");
                    s.push_str(&ident::quote(self.backend, &u.table)?);
                    s.push_str(" ON ");
                    s.push_str(&crate::ident::render_eq_join(self.backend, on)?);
                }
                s.push_str(" SET ");
                s.push_str(&set_sql);
                s
            }
            _ => {
                let mut s = format!("UPDATE {} SET {}", qtable, set_sql);
                if !self.using.is_empty() {
                    s.push_str(" FROM ");
                    let parts: Result<Vec<String>> = self.using.iter()
                        .map(|u| ident::quote(self.backend, &u.table))
                        .collect();
                    s.push_str(&parts?.join(", "));
                }
                s
            }
        };

        // WHEREs: combinar ON de using (en PG/SQLite) + wheres normales.
        let mut where_parts: Vec<String> = Vec::new();
        if self.backend != Backend::MySql {
            for u in &self.using {
                if let Some(on) = &u.on {
                    where_parts.push(crate::ident::render_eq_join(self.backend, on)?);
                }
            }
        }
        for c in &self.wheres {
            where_parts.push(render_cond(c, &mut b)?);
        }
        if !where_parts.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_parts.join(" AND "));
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

impl Query for UpdateQuery {
    fn category(&self) -> LogCategory {
        LogCategory::UPDATE
    }
    fn build_sql(&self) -> Result<(String, Vec<Value>)> {
        self.to_sql()
    }
}
