//! Templates SQL crudos con placeholders `{{WHERE}}` / `{{ORDER}}` etc.
//!
//! Permite escribir SQL a mano y delegar al builder los fragmentos
//! dinámicos. Útil cuando el SELECT no se acomoda al builder
//! (CTEs complejos, funciones específicas del backend, window functions).
//!
//! ```ignore
//! let q = db.raw_template(
//!     "SELECT u.id, u.name, c.name AS country
//!      FROM users u JOIN countries c ON u.country_id = c.id
//!      {{WHERE}} {{ORDER}} {{LIMIT}}"
//! )
//! .where_eq("u.status", "active")
//! .order_desc("u.created_at")
//! .limit(20);
//! let (sql, params) = q.to_sql()?;
//! ```
//!
//! Tokens reconocidos: `{{WHERE}}`, `{{GROUP}}`, `{{HAVING}}`,
//! `{{ORDER}}`, `{{LIMIT}}`, `{{OFFSET}}`. Si la cláusula no se
//! configuró, el token se expande a cadena vacía. Los `?` literales
//! en el template re-bindean al placeholder del backend en orden.

use crate::backend::Backend;
use crate::cond::{render_cond, Binder, Cond};
use crate::error::{QueryError, Result};
use crate::ident;
use crate::log::{LogCategory, Query};
use crate::select::OrderDir;
use crate::value::{IntoValue, Value};

#[derive(Debug, Clone)]
pub struct RawTemplate {
    backend: Backend,
    template: String,
    init_params: Vec<Value>,
    wheres: Vec<Cond>,
    group_by: Vec<String>,
    having: Vec<Cond>,
    order_by: Vec<(String, OrderDir)>,
    limit: Option<u64>,
    offset: Option<u64>,
}

impl RawTemplate {
    pub fn new<S: Into<String>>(backend: Backend, template: S) -> Self {
        Self {
            backend,
            template: template.into(),
            init_params: Vec::new(),
            wheres: Vec::new(),
            group_by: Vec::new(),
            having: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
        }
    }

    /// Params para los `?` literales en el template (en orden de aparición).
    pub fn bind(mut self, params: Vec<Value>) -> Self {
        self.init_params = params;
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
    pub fn where_cond(mut self, c: Cond) -> Self {
        self.wheres.push(c);
        self
    }
    pub fn where_raw<S: Into<String>>(mut self, sql: S, params: Vec<Value>) -> Self {
        self.wheres.push(Cond::Raw { sql: sql.into(), params });
        self
    }

    pub fn group_by<C: Into<String>>(mut self, col: C) -> Self {
        self.group_by.push(col.into());
        self
    }
    pub fn having(mut self, c: Cond) -> Self {
        self.having.push(c);
        self
    }

    pub fn order_asc<C: Into<String>>(mut self, col: C) -> Self {
        self.order_by.push((col.into(), OrderDir::Asc));
        self
    }
    pub fn order_desc<C: Into<String>>(mut self, col: C) -> Self {
        self.order_by.push((col.into(), OrderDir::Desc));
        self
    }
    pub fn order_by<C: Into<String>>(mut self, col: C, dir: OrderDir) -> Self {
        self.order_by.push((col.into(), dir));
        self
    }

    pub fn limit(mut self, n: u64) -> Self {
        self.limit = Some(n);
        self
    }
    pub fn offset(mut self, n: u64) -> Self {
        self.offset = Some(n);
        self
    }

    pub fn to_sql(&self) -> Result<(String, Vec<Value>)> {
        let mut b = Binder::new(self.backend);
        let mut out = String::with_capacity(self.template.len() + 64);
        let bytes = self.template.as_bytes();
        let mut i = 0;
        let mut init_iter = self.init_params.iter().cloned();

        while i < bytes.len() {
            let c = bytes[i];
            if c == b'?' {
                let v = init_iter.next().ok_or(QueryError::BindMismatch {
                    expected: self.init_params.len(),
                    got: self.init_params.len() + 1,
                })?;
                out.push_str(&b.push(v));
                i += 1;
                continue;
            }
            if c == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                if let Some(end) = find_close(bytes, i + 2) {
                    let token = std::str::from_utf8(&bytes[i + 2..end]).map_err(|_| {
                        QueryError::InvalidIdentifier(self.template.clone())
                    })?;
                    let expanded = self.expand(token.trim(), &mut b)?;
                    out.push_str(&expanded);
                    i = end + 2;
                    continue;
                }
            }
            out.push(c as char);
            i += 1;
        }

        if init_iter.next().is_some() {
            return Err(QueryError::BindMismatch {
                expected: self.template.matches('?').count(),
                got: self.init_params.len(),
            });
        }

        Ok((out.trim_end().to_string(), b.into_params()))
    }

    fn expand(&self, token: &str, b: &mut Binder) -> Result<String> {
        match token.to_ascii_uppercase().as_str() {
            "WHERE" => {
                if self.wheres.is_empty() {
                    return Ok(String::new());
                }
                let parts: Result<Vec<String>> =
                    self.wheres.iter().map(|c| render_cond(c, b)).collect();
                Ok(format!("WHERE {}", parts?.join(" AND ")))
            }
            "GROUP" | "GROUPBY" | "GROUP_BY" => {
                if self.group_by.is_empty() {
                    return Ok(String::new());
                }
                let parts: Result<Vec<String>> = self
                    .group_by
                    .iter()
                    .map(|c| ident::quote(self.backend, c))
                    .collect();
                Ok(format!("GROUP BY {}", parts?.join(", ")))
            }
            "HAVING" => {
                if self.having.is_empty() {
                    return Ok(String::new());
                }
                let parts: Result<Vec<String>> =
                    self.having.iter().map(|c| render_cond(c, b)).collect();
                Ok(format!("HAVING {}", parts?.join(" AND ")))
            }
            "ORDER" | "ORDERBY" | "ORDER_BY" => {
                if self.order_by.is_empty() {
                    return Ok(String::new());
                }
                let parts: Result<Vec<String>> = self
                    .order_by
                    .iter()
                    .map(|(c, d)| {
                        let qc = ident::quote(self.backend, c)?;
                        Ok(format!(
                            "{} {}",
                            qc,
                            if matches!(d, OrderDir::Asc) { "ASC" } else { "DESC" }
                        ))
                    })
                    .collect();
                Ok(format!("ORDER BY {}", parts?.join(", ")))
            }
            "LIMIT" => Ok(self.limit.map(|n| format!("LIMIT {}", n)).unwrap_or_default()),
            "OFFSET" => Ok(self.offset.map(|n| format!("OFFSET {}", n)).unwrap_or_default()),
            _ => Err(QueryError::InvalidIdentifier(format!("{{{{{}}}}}", token))),
        }
    }
}

fn find_close(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i);
        }
        i += 1;
    }
    None
}

impl Query for RawTemplate {
    fn category(&self) -> LogCategory {
        LogCategory::RAW
    }
    fn build_sql(&self) -> Result<(String, Vec<Value>)> {
        self.to_sql()
    }
}
