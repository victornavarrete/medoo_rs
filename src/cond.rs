use crate::backend::Backend;
use crate::error::{QueryError, Result};
use crate::ident;
use crate::json::{render_contains, render_extract, JsonPath};
use crate::value::Value;

/// Operadores binarios reconocidos por el parser estilo Medoo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Eq,
    NotEq,
    Gt,
    Lt,
    Gte,
    Lte,
    Like,
    NotLike,
    /// PG nativo `ILIKE` (case-insensitive). En MySQL/SQLite se renderea
    /// como `LOWER(col) LIKE LOWER(?)`.
    ILike,
}

impl Operator {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Operator::Eq => "=",
            Operator::NotEq => "<>",
            Operator::Gt => ">",
            Operator::Lt => "<",
            Operator::Gte => ">=",
            Operator::Lte => "<=",
            Operator::Like => "LIKE",
            Operator::NotLike => "NOT LIKE",
            Operator::ILike => "ILIKE",
        }
    }

    /// Acepta tanto el formato corto (`>`, `<>`, `~`) como el formato
    /// envuelto en corchetes estilo Medoo (`[>]`, `[<>]`, `[~]`).
    /// Case-insensitive para nombres de palabras (`like`, `LIKE`, `Like`).
    pub fn parse(raw: &str) -> Result<Self> {
        let s = raw.trim();
        let inner = if s.starts_with('[') && s.ends_with(']') && s.len() >= 2 {
            &s[1..s.len() - 1]
        } else {
            s
        };
        // operadores simbólicos: comparación literal (case no aplica)
        match inner {
            "=" | "==" => return Ok(Operator::Eq),
            "!" | "<>" | "!=" => return Ok(Operator::NotEq),
            ">" => return Ok(Operator::Gt),
            "<" => return Ok(Operator::Lt),
            ">=" => return Ok(Operator::Gte),
            "<=" => return Ok(Operator::Lte),
            "~" => return Ok(Operator::Like),
            "!~" => return Ok(Operator::NotLike),
            "~*" => return Ok(Operator::ILike),
            _ => {}
        }
        // nombres en palabras: case-insensitive
        if inner.eq_ignore_ascii_case("LIKE") {
            return Ok(Operator::Like);
        }
        if inner.eq_ignore_ascii_case("NOT LIKE") {
            return Ok(Operator::NotLike);
        }
        if inner.eq_ignore_ascii_case("ILIKE") {
            return Ok(Operator::ILike);
        }
        Err(QueryError::InvalidOperator(raw.to_string()))
    }
}

#[derive(Debug, Clone)]
pub enum Cond {
    Cmp { col: String, op: Operator, val: Value },
    In { col: String, vals: Vec<Value> },
    NotIn { col: String, vals: Vec<Value> },
    Between { col: String, lo: Value, hi: Value },
    /// `col BETWEEN lo_col AND hi_col` con flags de inclusividad.
    /// Si ambos son inclusive, usa `BETWEEN`. Si no, usa `>=`/`>` y `<=`/`<`.
    BetweenCols {
        col: String,
        lo_col: String,
        hi_col: String,
        inclusive_lo: bool,
        inclusive_hi: bool,
    },
    /// Un valor entre dos columnas: `lo_col <= val AND val <= hi_col`.
    ValueInRange {
        val: Value,
        lo_col: String,
        hi_col: String,
        inclusive_lo: bool,
        inclusive_hi: bool,
    },
    IsNull { col: String, negate: bool },
    Raw { sql: String, params: Vec<Value> },
    /// Compara el valor (texto desempacado) extraído de un path JSON.
    JsonGet { col: String, path: JsonPath, op: Operator, val: Value },
    /// `JSON_CONTAINS(col, ?)` / `col @> ?::jsonb`.
    JsonContains { col: String, json: Value },
    /// `<col> IN (SELECT ...)`.
    InSubQuery { col: String, sub: Box<crate::select::SelectQuery> },
    /// `<col> NOT IN (SELECT ...)`.
    NotInSubQuery { col: String, sub: Box<crate::select::SelectQuery> },
    /// `EXISTS (SELECT ...)`.
    Exists(Box<crate::select::SelectQuery>),
    /// `NOT EXISTS (SELECT ...)`.
    NotExists(Box<crate::select::SelectQuery>),
    /// `<col> <op> (SELECT v)` — comparación con subquery escalar.
    /// Si el subquery retorna múltiples filas, la DB lanza error.
    ScalarCmp { col: String, op: Operator, sub: Box<crate::select::SelectQuery> },
    And(Vec<Cond>),
    Or(Vec<Cond>),
}

impl Cond {
    pub fn eq<C: Into<String>, V: crate::value::IntoValue>(col: C, v: V) -> Self {
        Cond::Cmp { col: col.into(), op: Operator::Eq, val: v.into_value() }
    }

    pub fn op<C: Into<String>, V: crate::value::IntoValue>(col: C, op: &str, v: V) -> Result<Self> {
        Ok(Cond::Cmp {
            col: col.into(),
            op: Operator::parse(op)?,
            val: v.into_value(),
        })
    }

    pub fn r#in<C: Into<String>, V: crate::value::IntoValue, I: IntoIterator<Item = V>>(
        col: C,
        vals: I,
    ) -> Self {
        Cond::In {
            col: col.into(),
            vals: vals.into_iter().map(|v| v.into_value()).collect(),
        }
    }

    pub fn raw<S: Into<String>>(sql: S, params: Vec<Value>) -> Self {
        Cond::Raw { sql: sql.into(), params }
    }

    pub fn json_get<C: Into<String>, V: crate::value::IntoValue>(
        col: C,
        path: &str,
        op: &str,
        val: V,
    ) -> Result<Self> {
        Ok(Cond::JsonGet {
            col: col.into(),
            path: JsonPath::parse(path)?,
            op: Operator::parse(op)?,
            val: val.into_value(),
        })
    }

    pub fn json_contains<C: Into<String>>(col: C, json: Value) -> Self {
        Cond::JsonContains { col: col.into(), json }
    }

    pub fn in_subquery<C: Into<String>>(col: C, sub: crate::select::SelectQuery) -> Self {
        Cond::InSubQuery { col: col.into(), sub: Box::new(sub) }
    }
    pub fn not_in_subquery<C: Into<String>>(col: C, sub: crate::select::SelectQuery) -> Self {
        Cond::NotInSubQuery { col: col.into(), sub: Box::new(sub) }
    }
    pub fn exists(sub: crate::select::SelectQuery) -> Self {
        Cond::Exists(Box::new(sub))
    }
    pub fn not_exists(sub: crate::select::SelectQuery) -> Self {
        Cond::NotExists(Box::new(sub))
    }
    pub fn scalar_cmp<C: Into<String>>(col: C, op: &str, sub: crate::select::SelectQuery) -> Result<Self> {
        Ok(Cond::ScalarCmp {
            col: col.into(),
            op: Operator::parse(op)?,
            sub: Box::new(sub),
        })
    }
}

/// Acumulador de placeholders / params durante la serialización.
pub(crate) struct Binder {
    backend: Backend,
    params: Vec<Value>,
}

impl Binder {
    pub fn new(backend: Backend) -> Self {
        Self { backend, params: Vec::new() }
    }

    pub fn push(&mut self, v: Value) -> String {
        self.params.push(v);
        self.backend.placeholder(self.params.len())
    }

    pub fn into_params(self) -> Vec<Value> {
        self.params
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }
}

pub(crate) fn render_cond(c: &Cond, b: &mut Binder) -> Result<String> {
    match c {
        Cond::Cmp { col, op, val } => {
            let qcol = ident::quote(b.backend(), col)?;
            // NULL: solo Eq/NotEq tienen semántica con IS [NOT] NULL.
            // Otros operadores contra NULL siempre dan UNKNOWN/false en SQL,
            // así que rechazamos explícito para evitar bug silencioso.
            if matches!(val, Value::Null) {
                return match op {
                    Operator::Eq => Ok(format!("{} IS NULL", qcol)),
                    Operator::NotEq => Ok(format!("{} IS NOT NULL", qcol)),
                    _ => Err(QueryError::InvalidOperator(format!(
                        "{} contra NULL siempre es UNKNOWN; usá where_null/where_not_null o Eq/NotEq",
                        op.as_sql()
                    ))),
                };
            }
            // ILIKE: PG nativo; otros backends → LOWER(col) LIKE LOWER(?).
            if matches!(op, Operator::ILike) {
                let ph = b.push(val.clone());
                return match b.backend() {
                    crate::backend::Backend::Postgres => Ok(format!("{} ILIKE {}", qcol, ph)),
                    _ => Ok(format!("LOWER({}) LIKE LOWER({})", qcol, ph)),
                };
            }
            let ph = b.push(val.clone());
            Ok(format!("{} {} {}", qcol, op.as_sql(), ph))
        }
        Cond::In { col, vals } => {
            if vals.is_empty() {
                return Err(QueryError::EmptyInList(col.clone()));
            }
            let qcol = ident::quote(b.backend(), col)?;
            let phs: Vec<String> = vals.iter().map(|v| b.push(v.clone())).collect();
            Ok(format!("{} IN ({})", qcol, phs.join(", ")))
        }
        Cond::NotIn { col, vals } => {
            if vals.is_empty() {
                return Err(QueryError::EmptyInList(col.clone()));
            }
            let qcol = ident::quote(b.backend(), col)?;
            let phs: Vec<String> = vals.iter().map(|v| b.push(v.clone())).collect();
            Ok(format!("{} NOT IN ({})", qcol, phs.join(", ")))
        }
        Cond::Between { col, lo, hi } => {
            let qcol = ident::quote(b.backend(), col)?;
            let plo = b.push(lo.clone());
            let phi = b.push(hi.clone());
            Ok(format!("{} BETWEEN {} AND {}", qcol, plo, phi))
        }
        Cond::BetweenCols { col, lo_col, hi_col, inclusive_lo, inclusive_hi } => {
            let qcol = ident::quote(b.backend(), col)?;
            let qlo = ident::quote(b.backend(), lo_col)?;
            let qhi = ident::quote(b.backend(), hi_col)?;
            if *inclusive_lo && *inclusive_hi {
                Ok(format!("{} BETWEEN {} AND {}", qcol, qlo, qhi))
            } else {
                let lo_op = if *inclusive_lo { ">=" } else { ">" };
                let hi_op = if *inclusive_hi { "<=" } else { "<" };
                Ok(format!("({} {} {} AND {} {} {})", qcol, lo_op, qlo, qcol, hi_op, qhi))
            }
        }
        Cond::ValueInRange { val, lo_col, hi_col, inclusive_lo, inclusive_hi } => {
            let qlo = ident::quote(b.backend(), lo_col)?;
            let qhi = ident::quote(b.backend(), hi_col)?;
            // Pushear el valor dos veces — MySQL/SQLite usan placeholders
            // posicionales `?`, así que cada uso necesita su propio bind.
            let p1 = b.push(val.clone());
            let p2 = b.push(val.clone());
            let lo_op = if *inclusive_lo { "<=" } else { "<" };
            let hi_op = if *inclusive_hi { "<=" } else { "<" };
            Ok(format!("({} {} {} AND {} {} {})", qlo, lo_op, p1, p2, hi_op, qhi))
        }
        Cond::IsNull { col, negate } => {
            let qcol = ident::quote(b.backend(), col)?;
            Ok(format!("{} {}", qcol, if *negate { "IS NOT NULL" } else { "IS NULL" }))
        }
        Cond::Raw { sql, params } => {
            // Re-bindea: sustituye '?' por placeholders del backend, en orden.
            let mut out = String::with_capacity(sql.len());
            let mut iter = params.iter().cloned();
            for ch in sql.chars() {
                if ch == '?' {
                    let v = iter.next().ok_or(QueryError::BindMismatch {
                        expected: params.len(),
                        got: params.len() + 1,
                    })?;
                    out.push_str(&b.push(v));
                } else {
                    out.push(ch);
                }
            }
            if iter.next().is_some() {
                return Err(QueryError::BindMismatch {
                    expected: sql.matches('?').count(),
                    got: params.len(),
                });
            }
            Ok(out)
        }
        Cond::JsonGet { col, path, op, val } => {
            let qcol = ident::quote(b.backend(), col)?;
            let lhs = render_extract(b.backend(), &qcol, path, true);
            if matches!(val, Value::Null) {
                return match op {
                    Operator::Eq => Ok(format!("{} IS NULL", lhs)),
                    Operator::NotEq => Ok(format!("{} IS NOT NULL", lhs)),
                    _ => Err(QueryError::InvalidOperator(format!(
                        "{} contra NULL en JSON path no tiene sentido; usá Eq/NotEq",
                        op.as_sql()
                    ))),
                };
            }
            let ph = b.push(val.clone());
            Ok(format!("{} {} {}", lhs, op.as_sql(), ph))
        }
        Cond::JsonContains { col, json } => {
            let qcol = ident::quote(b.backend(), col)?;
            let ph = b.push(json.clone());
            render_contains(b.backend(), &qcol, &ph)
        }
        Cond::InSubQuery { col, sub } => {
            let qcol = ident::quote(b.backend(), col)?;
            let inner = sub.render_into(b)?;
            Ok(format!("{} IN ({})", qcol, inner))
        }
        Cond::NotInSubQuery { col, sub } => {
            let qcol = ident::quote(b.backend(), col)?;
            let inner = sub.render_into(b)?;
            Ok(format!("{} NOT IN ({})", qcol, inner))
        }
        Cond::Exists(sub) => {
            let inner = sub.render_into(b)?;
            Ok(format!("EXISTS ({})", inner))
        }
        Cond::NotExists(sub) => {
            let inner = sub.render_into(b)?;
            Ok(format!("NOT EXISTS ({})", inner))
        }
        Cond::ScalarCmp { col, op, sub } => {
            let qcol = ident::quote(b.backend(), col)?;
            let inner = sub.render_into(b)?;
            Ok(format!("{} {} ({})", qcol, op.as_sql(), inner))
        }
        Cond::And(list) => render_group(list, " AND ", b),
        Cond::Or(list) => render_group(list, " OR ", b),
    }
}

fn render_group(list: &[Cond], sep: &str, b: &mut Binder) -> Result<String> {
    if list.is_empty() {
        // un grupo vacío equivale a TRUE en AND y FALSE en OR; devolvemos
        // explícitamente literal seguro.
        return Ok(if sep.contains("AND") { "1=1".into() } else { "1=0".into() });
    }
    let parts: Result<Vec<String>> = list.iter().map(|c| render_cond(c, b)).collect();
    let parts = parts?;
    if parts.len() == 1 {
        Ok(parts.into_iter().next().unwrap())
    } else {
        Ok(format!("({})", parts.join(sep)))
    }
}
