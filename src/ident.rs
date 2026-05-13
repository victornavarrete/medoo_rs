use crate::backend::Backend;
use crate::error::{QueryError, Result};

/// Valida un identificador (tabla, columna, alias). Permite además
/// la forma `tabla.columna`. Cero tolerancia con cualquier carácter
/// fuera de `[A-Za-z0-9_]` para bloquear inyección por nombre.
pub fn validate(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(QueryError::InvalidIdentifier(name.to_string()));
    }
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() > 2 {
        return Err(QueryError::InvalidIdentifier(name.to_string()));
    }
    for p in parts {
        if p.is_empty() {
            return Err(QueryError::InvalidIdentifier(name.to_string()));
        }
        let mut chars = p.chars();
        let first = chars.next().unwrap();
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err(QueryError::InvalidIdentifier(name.to_string()));
        }
        for c in chars {
            if !(c.is_ascii_alphanumeric() || c == '_') {
                return Err(QueryError::InvalidIdentifier(name.to_string()));
            }
        }
    }
    Ok(())
}

/// Cita un identificador validado (soporta `tabla.columna`).
pub fn quote(backend: Backend, name: &str) -> Result<String> {
    validate(name)?;
    Ok(name
        .split('.')
        .map(|p| backend.quote_ident(p))
        .collect::<Vec<_>>()
        .join("."))
}

/// Renderiza una cláusula ON segura: `tabla.col = otra.col`,
/// encadenadas con `AND`. Bypass para `true` y `1=1` (LATERAL).
/// Sin literales ni operadores fuera de `=`.
pub fn render_eq_join(backend: Backend, on: &str) -> Result<String> {
    let trimmed = on.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Ok("true".to_string());
    }
    if trimmed == "1=1" {
        return Ok("1 = 1".to_string());
    }
    let mut out = Vec::new();
    for chunk in on.split(" AND ") {
        let parts: Vec<&str> = chunk.split('=').map(|s| s.trim()).collect();
        if parts.len() != 2 {
            return Err(QueryError::InvalidIdentifier(on.to_string()));
        }
        let l = quote(backend, parts[0])?;
        let r = quote(backend, parts[1])?;
        out.push(format!("{} = {}", l, r));
    }
    Ok(out.join(" AND "))
}
