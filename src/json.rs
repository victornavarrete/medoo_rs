//! Soporte para columnas JSON inspirado en la API de MySQL
//! (`JSON_EXTRACT`, `JSON_UNQUOTE`, `JSON_CONTAINS`), con renders
//! equivalentes para Postgres (`#>`, `#>>`, `@>`) y SQLite
//! (`json_extract`).

use crate::backend::Backend;
use crate::error::{QueryError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonSeg {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPath {
    segments: Vec<JsonSeg>,
}

impl JsonPath {
    /// Parsea paths estilo MySQL: `$.a.b[0].c` (el `$` inicial es opcional).
    /// Solo acepta keys `[A-Za-z_][A-Za-z0-9_]*` e índices numéricos —
    /// cualquier otro carácter es rechazado para bloquear inyección.
    pub fn parse(input: &str) -> Result<Self> {
        let s = input.trim();
        if s.is_empty() {
            return Err(QueryError::InvalidIdentifier(input.to_string()));
        }
        let body = s.strip_prefix('$').unwrap_or(s);
        let body = body.strip_prefix('.').unwrap_or(body);

        let mut segments = Vec::new();
        let mut buf = String::new();
        let mut chars = body.chars().peekable();

        let flush_key = |buf: &mut String, segments: &mut Vec<JsonSeg>| -> Result<()> {
            if buf.is_empty() {
                return Ok(());
            }
            validate_key(buf)?;
            segments.push(JsonSeg::Key(std::mem::take(buf)));
            Ok(())
        };

        while let Some(c) = chars.next() {
            match c {
                '.' => {
                    // requiere algo antes del punto (key o cierre `]`).
                    let prev_was_index =
                        buf.is_empty() && matches!(segments.last(), Some(JsonSeg::Index(_)));
                    if buf.is_empty() && !prev_was_index {
                        return Err(QueryError::InvalidIdentifier(input.to_string()));
                    }
                    flush_key(&mut buf, &mut segments)?;
                }
                '[' => {
                    flush_key(&mut buf, &mut segments)?;
                    let mut num = String::new();
                    for nc in chars.by_ref() {
                        if nc == ']' {
                            break;
                        }
                        if !nc.is_ascii_digit() {
                            return Err(QueryError::InvalidIdentifier(input.to_string()));
                        }
                        num.push(nc);
                    }
                    if num.is_empty() {
                        return Err(QueryError::InvalidIdentifier(input.to_string()));
                    }
                    let idx: usize = num
                        .parse()
                        .map_err(|_| QueryError::InvalidIdentifier(input.to_string()))?;
                    segments.push(JsonSeg::Index(idx));
                }
                c if c.is_ascii_alphanumeric() || c == '_' => buf.push(c),
                _ => return Err(QueryError::InvalidIdentifier(input.to_string())),
            }
        }
        flush_key(&mut buf, &mut segments)?;

        if segments.is_empty() {
            return Err(QueryError::InvalidIdentifier(input.to_string()));
        }
        Ok(JsonPath { segments })
    }

    pub fn segments(&self) -> &[JsonSeg] {
        &self.segments
    }

    /// `$.a.b[0]` (MySQL/SQLite).
    pub fn render_mysql(&self) -> String {
        let mut out = String::from("$");
        for s in &self.segments {
            match s {
                JsonSeg::Key(k) => {
                    out.push('.');
                    out.push_str(k);
                }
                JsonSeg::Index(i) => {
                    out.push('[');
                    out.push_str(&i.to_string());
                    out.push(']');
                }
            }
        }
        out
    }

    /// `{a,b,0}` (Postgres `#>` / `#>>`).
    pub fn render_postgres(&self) -> String {
        let parts: Vec<String> = self
            .segments
            .iter()
            .map(|s| match s {
                JsonSeg::Key(k) => k.clone(),
                JsonSeg::Index(i) => i.to_string(),
            })
            .collect();
        format!("{{{}}}", parts.join(","))
    }
}

fn validate_key(k: &str) -> Result<()> {
    if k.is_empty() || k.len() > 64 {
        return Err(QueryError::InvalidIdentifier(k.to_string()));
    }
    let mut chars = k.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(QueryError::InvalidIdentifier(k.to_string()));
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return Err(QueryError::InvalidIdentifier(k.to_string()));
        }
    }
    Ok(())
}

/// Renderiza un acceso JSON como expresión SQL (sin la parte de comparación).
/// Si `as_text` es true, retorna texto desempacado (sin comillas JSON).
pub(crate) fn render_extract(
    backend: Backend,
    quoted_col: &str,
    path: &JsonPath,
    as_text: bool,
) -> String {
    match backend {
        Backend::MySql => {
            let p = path.render_mysql();
            if as_text {
                format!("JSON_UNQUOTE(JSON_EXTRACT({}, '{}'))", quoted_col, p)
            } else {
                format!("JSON_EXTRACT({}, '{}')", quoted_col, p)
            }
        }
        Backend::Sqlite => {
            // SQLite siempre devuelve texto desempacado para escalares;
            // ignoramos `as_text` por irrelevante.
            let _ = as_text;
            let p = path.render_mysql();
            format!("json_extract({}, '{}')", quoted_col, p)
        }
        Backend::Postgres => {
            let p = path.render_postgres();
            let op = if as_text { "#>>" } else { "#>" };
            format!("{} {} '{}'", quoted_col, op, p)
        }
    }
}

/// Renderiza `JSON_CONTAINS(col, ?)` / `col @> ?::jsonb`. SQLite no
/// soporta containment nativo: devuelve error.
pub(crate) fn render_contains(
    backend: Backend,
    quoted_col: &str,
    placeholder: &str,
) -> Result<String> {
    match backend {
        Backend::MySql => Ok(format!("JSON_CONTAINS({}, {})", quoted_col, placeholder)),
        Backend::Postgres => Ok(format!("{} @> {}::jsonb", quoted_col, placeholder)),
        Backend::Sqlite => Err(QueryError::InvalidOperator(
            "json_contains no soportado en SQLite".to_string(),
        )),
    }
}
