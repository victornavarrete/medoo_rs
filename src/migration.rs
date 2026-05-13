//! Migraciones: data model + planificador de orden. La ejecución
//! contra el motor real se hace en la capa async del usuario,
//! que llama `migrator.pending(applied).iter()` y ejecuta cada
//! sentencia `up` (o `down` para rollback).
//!
//! Ejemplo de wiring async (sqlx, fuera de este crate):
//!
//! ```ignore
//! let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM _medoo_migrations")
//!     .fetch_all(&pool).await?;
//! for m in migrator.pending(&applied) {
//!     for sql in &m.up { sqlx::query(sql).execute(&pool).await?; }
//!     sqlx::query("INSERT INTO _medoo_migrations(version,name) VALUES (?,?)")
//!         .bind(m.version).bind(&m.name).execute(&pool).await?;
//! }
//! ```

use crate::backend::Backend;
use crate::error::{QueryError, Result};
use crate::ident;

#[derive(Debug, Clone)]
pub struct Migration {
    pub version: i64,
    pub name: String,
    pub up: Vec<String>,
    pub down: Vec<String>,
}

impl Migration {
    pub fn new<N: Into<String>>(version: i64, name: N) -> Self {
        Self { version, name: name.into(), up: Vec::new(), down: Vec::new() }
    }
    pub fn up<S: Into<String>>(mut self, sql: S) -> Self {
        self.up.push(sql.into());
        self
    }
    pub fn down<S: Into<String>>(mut self, sql: S) -> Self {
        self.down.push(sql.into());
        self
    }
}

#[derive(Debug, Default, Clone)]
pub struct Migrator {
    migrations: Vec<Migration>,
}

impl Migrator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(mut self, m: Migration) -> Self {
        self.migrations.push(m);
        self
    }

    /// Devuelve una copia ordenada por `version`. Falla si hay
    /// versiones duplicadas (red flag operacional).
    pub fn ordered(&self) -> Result<Vec<Migration>> {
        let mut v = self.migrations.clone();
        v.sort_by_key(|m| m.version);
        for w in v.windows(2) {
            if w[0].version == w[1].version {
                return Err(QueryError::InvalidIdentifier(format!(
                    "migración duplicada version={}",
                    w[0].version
                )));
            }
        }
        Ok(v)
    }

    /// Migraciones pendientes a aplicar (orden ascendente).
    pub fn pending(&self, applied: &[i64]) -> Result<Vec<Migration>> {
        let ordered = self.ordered()?;
        Ok(ordered
            .into_iter()
            .filter(|m| !applied.contains(&m.version))
            .collect())
    }

    /// Migraciones a deshacer hasta `target_version` (excluida),
    /// en orden descendente. Si `target_version` es `None`, deshace
    /// todas las aplicadas.
    pub fn rollback_plan(&self, applied: &[i64], target_version: Option<i64>) -> Result<Vec<Migration>> {
        let ordered = self.ordered()?;
        let mut planned: Vec<Migration> = ordered
            .into_iter()
            .filter(|m| applied.contains(&m.version))
            .filter(|m| match target_version {
                Some(t) => m.version > t,
                None => true,
            })
            .collect();
        planned.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(planned)
    }

    pub fn all(&self) -> &[Migration] {
        &self.migrations
    }
}

/// SQL para crear la tabla de tracking. Llamalo una vez al arrancar.
pub fn tracking_table_sql(backend: Backend) -> Result<String> {
    let t = ident::quote(backend, "_medoo_migrations")?;
    let v = ident::quote(backend, "version")?;
    let n = ident::quote(backend, "name")?;
    let a = ident::quote(backend, "applied_at")?;
    let ts_default = match backend {
        Backend::Postgres => "DEFAULT now()",
        Backend::MySql => "DEFAULT CURRENT_TIMESTAMP",
        Backend::Sqlite => "DEFAULT CURRENT_TIMESTAMP",
    };
    let ts_type = match backend {
        Backend::Postgres => "TIMESTAMPTZ",
        Backend::MySql => "DATETIME",
        Backend::Sqlite => "TEXT",
    };
    Ok(format!(
        "CREATE TABLE IF NOT EXISTS {t} ({v} BIGINT PRIMARY KEY, {n} TEXT NOT NULL, {a} {ts_type} NOT NULL {ts_default})",
    ))
}
