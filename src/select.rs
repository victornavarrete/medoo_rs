use crate::backend::Backend;
use crate::cond::{render_cond, Binder, Cond};
use crate::error::{QueryError, Result};
use crate::ident;
use crate::log::{LogCategory, Query};
use crate::value::{IntoValue, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

impl JoinKind {
    fn as_sql(&self) -> &'static str {
        match self {
            JoinKind::Inner => "INNER JOIN",
            JoinKind::Left => "LEFT JOIN",
            JoinKind::Right => "RIGHT JOIN",
            JoinKind::Full => "FULL JOIN",
            JoinKind::Cross => "CROSS JOIN",
        }
    }
}

#[derive(Debug, Clone)]
enum JoinTarget {
    Table(String),
    /// LATERAL (subquery) AS alias. Soportado en PG y MySQL 8.0.14+.
    Lateral { sub: Box<SelectQuery>, alias: String },
}

#[derive(Debug, Clone)]
struct Join {
    kind: JoinKind,
    target: JoinTarget,
    on: Option<String>, // None para CROSS JOIN
}

#[derive(Debug, Clone)]
struct Cte {
    name: String,
    query: Box<SelectQuery>,
}

#[derive(Debug, Clone)]
pub struct SelectQuery {
    backend: Backend,
    table: String,
    columns: Vec<String>, // vacío => "*"
    distinct: bool,
    joins: Vec<Join>,
    wheres: Vec<Cond>,
    group_by: Vec<String>,
    having: Vec<Cond>,
    order_by: Vec<(String, OrderDir)>,
    limit: Option<u64>,
    offset: Option<u64>,
    ctes: Vec<Cte>,
    cte_recursive: bool,
}

impl SelectQuery {
    pub fn new(backend: Backend, table: &str) -> Self {
        Self {
            backend,
            table: table.to_string(),
            columns: Vec::new(),
            distinct: false,
            joins: Vec::new(),
            wheres: Vec::new(),
            group_by: Vec::new(),
            having: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
            ctes: Vec::new(),
            cte_recursive: false,
        }
    }

    pub fn columns<I: IntoIterator<Item = S>, S: Into<String>>(mut self, cols: I) -> Self {
        self.columns = cols.into_iter().map(|s| s.into()).collect();
        self
    }

    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    pub fn where_eq<C: Into<String>, V: IntoValue>(mut self, col: C, v: V) -> Self {
        self.wheres.push(Cond::eq(col, v));
        self
    }

    pub fn where_op<C: Into<String>, V: IntoValue>(mut self, col: C, op: &str, v: V) -> Self {
        match Cond::op(col, op, v) {
            Ok(c) => self.wheres.push(c),
            Err(e) => panic!("medoo_rs: where_op operador inválido en literal: {}", e),
        }
        self
    }

    /// Versión non-panic: retorna `Result<Self, QueryError>`. Usá esta
    /// cuando el operador viene de input dinámico (config, query string).
    pub fn try_where_op<C: Into<String>, V: IntoValue>(
        mut self,
        col: C,
        op: &str,
        v: V,
    ) -> Result<Self> {
        self.wheres.push(Cond::op(col, op, v)?);
        Ok(self)
    }

    /// Versión non-panic de `where_eq`. `where_eq` ya no panicea hoy
    /// (siempre construye un `Cond::Cmp`), pero se incluye por simetría
    /// y para retornar `Result` cuando se compone en cadenas fallibles.
    pub fn try_where_eq<C: Into<String>, V: IntoValue>(
        mut self,
        col: C,
        v: V,
    ) -> Result<Self> {
        self.wheres.push(Cond::eq(col, v));
        Ok(self)
    }

    pub fn where_in<C: Into<String>, V: IntoValue, I: IntoIterator<Item = V>>(
        mut self,
        col: C,
        vals: I,
    ) -> Self {
        self.wheres.push(Cond::r#in(col, vals));
        self
    }

    pub fn where_between<C: Into<String>, A: IntoValue, B: IntoValue>(
        mut self,
        col: C,
        lo: A,
        hi: B,
    ) -> Self {
        self.wheres.push(Cond::Between {
            col: col.into(),
            lo: lo.into_value(),
            hi: hi.into_value(),
        });
        self
    }

    /// `WHERE col BETWEEN lo_col AND hi_col` (ambos inclusive).
    /// Útil para "fecha entre fecha_inicio y fecha_fin".
    pub fn where_between_cols<C, L, H>(self, col: C, lo_col: L, hi_col: H) -> Self
    where C: Into<String>, L: Into<String>, H: Into<String> {
        self.where_between_cols_with(col, lo_col, hi_col, true, true)
    }

    /// Versión con flags de inclusividad por límite. `false` deja `<` o `>`.
    pub fn where_between_cols_with<C, L, H>(
        mut self,
        col: C,
        lo_col: L,
        hi_col: H,
        inclusive_lo: bool,
        inclusive_hi: bool,
    ) -> Self
    where C: Into<String>, L: Into<String>, H: Into<String> {
        self.wheres.push(Cond::BetweenCols {
            col: col.into(),
            lo_col: lo_col.into(),
            hi_col: hi_col.into(),
            inclusive_lo,
            inclusive_hi,
        });
        self
    }

    /// `WHERE lo_col <= val AND val <= hi_col` — un valor entre dos
    /// columnas. Útil para "el día de hoy cae entre fecha_desde y
    /// fecha_hasta de la promo".
    pub fn where_value_in_range<V, L, H>(self, val: V, lo_col: L, hi_col: H) -> Self
    where V: IntoValue, L: Into<String>, H: Into<String> {
        self.where_value_in_range_with(val, lo_col, hi_col, true, true)
    }

    pub fn where_value_in_range_with<V, L, H>(
        mut self,
        val: V,
        lo_col: L,
        hi_col: H,
        inclusive_lo: bool,
        inclusive_hi: bool,
    ) -> Self
    where V: IntoValue, L: Into<String>, H: Into<String> {
        self.wheres.push(Cond::ValueInRange {
            val: val.into_value(),
            lo_col: lo_col.into(),
            hi_col: hi_col.into(),
            inclusive_lo,
            inclusive_hi,
        });
        self
    }

    /// `col LIKE 'pattern'`. Pasá `%`/`_` en el pattern como necesites.
    pub fn where_like<C: Into<String>, P: Into<String>>(mut self, col: C, pattern: P) -> Self {
        self.wheres.push(Cond::Cmp {
            col: col.into(),
            op: crate::cond::Operator::Like,
            val: Value::Text(pattern.into()),
        });
        self
    }
    /// `col NOT LIKE 'pattern'`.
    pub fn where_not_like<C: Into<String>, P: Into<String>>(mut self, col: C, pattern: P) -> Self {
        self.wheres.push(Cond::Cmp {
            col: col.into(),
            op: crate::cond::Operator::NotLike,
            val: Value::Text(pattern.into()),
        });
        self
    }
    /// `col LIKE 'prefix%'`. Auto-escapa `%` y `_` del prefix.
    pub fn where_starts_with<C: Into<String>, P: AsRef<str>>(self, col: C, prefix: P) -> Self {
        let p = format!("{}%", escape_like(prefix.as_ref()));
        self.where_like(col, p)
    }
    /// `col LIKE '%suffix'`.
    pub fn where_ends_with<C: Into<String>, P: AsRef<str>>(self, col: C, suffix: P) -> Self {
        let p = format!("%{}", escape_like(suffix.as_ref()));
        self.where_like(col, p)
    }
    /// `col LIKE '%needle%'`.
    pub fn where_contains<C: Into<String>, P: AsRef<str>>(self, col: C, needle: P) -> Self {
        let p = format!("%{}%", escape_like(needle.as_ref()));
        self.where_like(col, p)
    }
    /// PG `ILIKE` (case-insensitive). En MySQL/SQLite cae a
    /// `LOWER(col) LIKE LOWER(?)` para resultar equivalente.
    pub fn where_ilike<C: Into<String>, P: Into<String>>(mut self, col: C, pattern: P) -> Self {
        let pat = pattern.into();
        match self.backend {
            Backend::Postgres => {
                let raw = format!("{} ILIKE ?", crate::ident::quote(self.backend, &col.into()).unwrap_or_default());
                self.wheres.push(Cond::Raw { sql: raw, params: vec![Value::Text(pat)] });
            }
            _ => {
                let qcol = crate::ident::quote(self.backend, &col.into()).unwrap_or_default();
                let raw = format!("LOWER({}) LIKE LOWER(?)", qcol);
                self.wheres.push(Cond::Raw { sql: raw, params: vec![Value::Text(pat)] });
            }
        }
        self
    }

    pub fn where_null<C: Into<String>>(mut self, col: C) -> Self {
        self.wheres.push(Cond::IsNull { col: col.into(), negate: false });
        self
    }

    pub fn where_not_null<C: Into<String>>(mut self, col: C) -> Self {
        self.wheres.push(Cond::IsNull { col: col.into(), negate: true });
        self
    }

    /// `WHERE JSON_UNQUOTE(JSON_EXTRACT(col,'$.path')) <op> ?` (MySQL)
    /// — equivalente Postgres `col #>> '{path}' <op> ?` y SQLite
    /// `json_extract(col,'$.path') <op> ?`.
    pub fn where_json<C: Into<String>, V: IntoValue>(
        mut self,
        col: C,
        path: &str,
        op: &str,
        v: V,
    ) -> Self {
        match crate::cond::Cond::json_get(col, path, op, v) {
            Ok(c) => self.wheres.push(c),
            Err(e) => panic!("medoo_rs: where_json inválido: {}", e),
        }
        self
    }

    /// `JSON_CONTAINS(col, ?)` / `col @> ?::jsonb`. SQLite no soportado.
    pub fn where_json_contains<C: Into<String>>(mut self, col: C, json: Value) -> Self {
        self.wheres.push(crate::cond::Cond::json_contains(col, json));
        self
    }

    /// `WHERE col IN (SELECT ...)`.
    pub fn where_in_subquery<C: Into<String>>(mut self, col: C, sub: SelectQuery) -> Self {
        self.wheres.push(crate::cond::Cond::in_subquery(col, sub));
        self
    }
    /// `WHERE col NOT IN (SELECT ...)`.
    pub fn where_not_in_subquery<C: Into<String>>(mut self, col: C, sub: SelectQuery) -> Self {
        self.wheres.push(crate::cond::Cond::not_in_subquery(col, sub));
        self
    }
    /// `WHERE EXISTS (SELECT ...)`.
    pub fn where_exists(mut self, sub: SelectQuery) -> Self {
        self.wheres.push(crate::cond::Cond::exists(sub));
        self
    }
    /// `WHERE NOT EXISTS (SELECT ...)`.
    pub fn where_not_exists(mut self, sub: SelectQuery) -> Self {
        self.wheres.push(crate::cond::Cond::not_exists(sub));
        self
    }
    /// `WHERE col <op> (SELECT v)` — comparación con subquery escalar.
    /// El operador acepta los mismos formatos que `where_op`.
    pub fn where_scalar<C: Into<String>>(mut self, col: C, op: &str, sub: SelectQuery) -> Self {
        match crate::cond::Cond::scalar_cmp(col, op, sub) {
            Ok(c) => self.wheres.push(c),
            Err(e) => panic!("medoo_rs: where_scalar operador inválido: {}", e),
        }
        self
    }

    /// Agrega un CTE: `WITH <name> AS (<sub>) SELECT ...`. Se pueden
    /// encadenar varios; salen separados por coma.
    pub fn with<N: Into<String>>(mut self, name: N, sub: SelectQuery) -> Self {
        self.ctes.push(Cte { name: name.into(), query: Box::new(sub) });
        self
    }
    /// Marca los CTEs como `WITH RECURSIVE`. Soportado en PG/SQLite/
    /// MySQL 8+/MariaDB 10.2+.
    pub fn with_recursive_flag(mut self) -> Self {
        self.cte_recursive = true;
        self
    }

    pub fn where_raw<S: Into<String>>(mut self, sql: S, params: Vec<Value>) -> Self {
        self.wheres.push(Cond::Raw { sql: sql.into(), params });
        self
    }

    pub fn where_cond(mut self, c: Cond) -> Self {
        self.wheres.push(c);
        self
    }

    pub fn or_where(mut self, conds: Vec<Cond>) -> Self {
        self.wheres.push(Cond::Or(conds));
        self
    }

    pub fn join_on<T: Into<String>, O: Into<String>>(mut self, kind: JoinKind, table: T, on: O) -> Self {
        self.joins.push(Join {
            kind,
            target: JoinTarget::Table(table.into()),
            on: Some(on.into()),
        });
        self
    }
    pub fn inner_join<T: Into<String>, O: Into<String>>(self, t: T, on: O) -> Self {
        self.join_on(JoinKind::Inner, t, on)
    }
    pub fn left_join<T: Into<String>, O: Into<String>>(self, t: T, on: O) -> Self {
        self.join_on(JoinKind::Left, t, on)
    }
    pub fn right_join<T: Into<String>, O: Into<String>>(self, t: T, on: O) -> Self {
        self.join_on(JoinKind::Right, t, on)
    }
    pub fn cross_join<T: Into<String>>(mut self, t: T) -> Self {
        self.joins.push(Join {
            kind: JoinKind::Cross,
            target: JoinTarget::Table(t.into()),
            on: None,
        });
        self
    }

    /// `INNER JOIN LATERAL (subquery) AS <alias> ON <cond>` (PG / MySQL 8.0.14+).
    pub fn inner_join_lateral<A: Into<String>, O: Into<String>>(
        mut self,
        sub: SelectQuery,
        alias: A,
        on: O,
    ) -> Self {
        self.joins.push(Join {
            kind: JoinKind::Inner,
            target: JoinTarget::Lateral { sub: Box::new(sub), alias: alias.into() },
            on: Some(on.into()),
        });
        self
    }
    /// `LEFT JOIN LATERAL (subquery) AS <alias> ON <cond>`.
    pub fn left_join_lateral<A: Into<String>, O: Into<String>>(
        mut self,
        sub: SelectQuery,
        alias: A,
        on: O,
    ) -> Self {
        self.joins.push(Join {
            kind: JoinKind::Left,
            target: JoinTarget::Lateral { sub: Box::new(sub), alias: alias.into() },
            on: Some(on.into()),
        });
        self
    }
    /// `CROSS JOIN LATERAL (subquery) AS <alias>` — sin ON.
    pub fn cross_join_lateral<A: Into<String>>(mut self, sub: SelectQuery, alias: A) -> Self {
        self.joins.push(Join {
            kind: JoinKind::Cross,
            target: JoinTarget::Lateral { sub: Box::new(sub), alias: alias.into() },
            on: None,
        });
        self
    }

    pub fn order_by<C: Into<String>>(mut self, col: C, dir: OrderDir) -> Self {
        self.order_by.push((col.into(), dir));
        self
    }
    pub fn order_asc<C: Into<String>>(self, col: C) -> Self {
        self.order_by(col, OrderDir::Asc)
    }
    pub fn order_desc<C: Into<String>>(self, col: C) -> Self {
        self.order_by(col, OrderDir::Desc)
    }

    pub fn group_by<C: Into<String>>(mut self, col: C) -> Self {
        self.group_by.push(col.into());
        self
    }

    pub fn having(mut self, c: Cond) -> Self {
        self.having.push(c);
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
        let sql = self.render_into(&mut b)?;
        Ok((sql, b.into_params()))
    }

    /// Renderiza la query usando un Binder externo (para subqueries).
    /// Los placeholders se asignan en orden global con los del outer.
    pub(crate) fn render_into(&self, b: &mut Binder) -> Result<String> {
        let mut sql = String::new();
        if !self.ctes.is_empty() {
            sql.push_str(if self.cte_recursive { "WITH RECURSIVE " } else { "WITH " });
            let mut parts = Vec::with_capacity(self.ctes.len());
            for cte in &self.ctes {
                ident::validate(&cte.name)?;
                let inner = cte.query.render_into(b)?;
                parts.push(format!(
                    "{} AS ({})",
                    self.backend.quote_ident(&cte.name),
                    inner
                ));
            }
            sql.push_str(&parts.join(", "));
            sql.push(' ');
        }
        sql.push_str("SELECT ");
        if self.distinct {
            sql.push_str("DISTINCT ");
        }
        if self.columns.is_empty() {
            sql.push('*');
        } else {
            let cols: Result<Vec<String>> = self
                .columns
                .iter()
                .map(|c| render_column(self.backend, c))
                .collect();
            sql.push_str(&cols?.join(", "));
        }
        sql.push_str(" FROM ");
        sql.push_str(&ident::quote(self.backend, &self.table)?);

        for j in &self.joins {
            sql.push(' ');
            sql.push_str(j.kind.as_sql());
            sql.push(' ');
            match &j.target {
                JoinTarget::Table(t) => {
                    sql.push_str(&ident::quote(self.backend, t)?);
                }
                JoinTarget::Lateral { sub, alias } => {
                    ident::validate(alias)?;
                    sql.push_str("LATERAL (");
                    sql.push_str(&sub.render_into(b)?);
                    sql.push_str(") AS ");
                    sql.push_str(&self.backend.quote_ident(alias));
                }
            }
            if let Some(on) = &j.on {
                sql.push_str(" ON ");
                sql.push_str(&render_join_on(self.backend, on)?);
            }
        }

        if !self.wheres.is_empty() {
            sql.push_str(" WHERE ");
            let parts: Result<Vec<String>> = self.wheres.iter().map(|c| render_cond(c, b)).collect();
            sql.push_str(&parts?.join(" AND "));
        }

        if !self.group_by.is_empty() {
            sql.push_str(" GROUP BY ");
            let parts: Result<Vec<String>> = self
                .group_by
                .iter()
                .map(|c| ident::quote(self.backend, c))
                .collect();
            sql.push_str(&parts?.join(", "));
        }

        if !self.having.is_empty() {
            sql.push_str(" HAVING ");
            let parts: Result<Vec<String>> = self.having.iter().map(|c| render_cond(c, b)).collect();
            sql.push_str(&parts?.join(" AND "));
        }

        if !self.order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            let parts: Result<Vec<String>> = self
                .order_by
                .iter()
                .map(|(c, d)| {
                    let qc = ident::quote(self.backend, c)?;
                    Ok(format!("{} {}", qc, if matches!(d, OrderDir::Asc) { "ASC" } else { "DESC" }))
                })
                .collect();
            sql.push_str(&parts?.join(", "));
        }

        if let Some(l) = self.limit {
            sql.push_str(&format!(" LIMIT {}", l));
        }
        if let Some(o) = self.offset {
            sql.push_str(&format!(" OFFSET {}", o));
        }

        Ok(sql)
    }
}

/// Escapa `%`, `_` y `\` en input de usuario para `starts_with` /
/// `ends_with` / `contains`. Usa `\` como escape char (default LIKE).
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn render_column(backend: Backend, c: &str) -> Result<String> {
    if c == "*" {
        return Ok("*".to_string());
    }
    let (expr, alias) = match split_alias(c) {
        Some((e, a)) => (e, Some(a)),
        None => (c, None),
    };
    let expr_sql = if expr.contains('(') {
        // raw fragment (agregado u operador); pasa sin quotar pero rechaza
        // ';' o '--' por defensa básica.
        if expr.contains(';') || expr.contains("--") {
            return Err(QueryError::InvalidIdentifier(expr.to_string()));
        }
        expr.to_string()
    } else {
        ident::quote(backend, expr)?
    };
    if let Some(a) = alias {
        ident::validate(a)?;
        Ok(format!("{} AS {}", expr_sql, backend.quote_ident(a)))
    } else {
        Ok(expr_sql)
    }
}

fn split_alias(c: &str) -> Option<(&str, &str)> {
    // soporta "expr AS alias" (case insensitive) o "expr alias" no — exigimos AS para ser explícitos.
    let lower = c.to_ascii_lowercase();
    let idx = lower.find(" as ")?;
    let expr = c[..idx].trim();
    let alias = c[idx + 4..].trim();
    if expr.is_empty() || alias.is_empty() {
        return None;
    }
    Some((expr, alias))
}

impl Query for SelectQuery {
    fn category(&self) -> LogCategory {
        LogCategory::READ
    }
    fn build_sql(&self) -> Result<(String, Vec<Value>)> {
        self.to_sql()
    }
}

/// Renderiza una cláusula ON segura: acepta `tabla.col = otra.col`,
/// encadenadas con `AND`, o el literal `true`/`1=1` para joins lateral
/// sin condición. No permite literales ni operadores fuera de `=`.
fn render_join_on(backend: Backend, on: &str) -> Result<String> {
    ident::render_eq_join(backend, on)
}
