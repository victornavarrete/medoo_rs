//! medoo_rs — query builder dinámico inspirado en Medoo (PHP).
//!
//! Núcleo sin dependencias: construye SQL parametrizado con placeholders
//! correctos por backend (Postgres `$N`, MySQL/SQLite `?`). La capa async
//! con `sqlx` se monta encima en una segunda iteración.

pub mod backend;
pub mod cond;
pub mod delete;
pub mod error;
pub mod ident;
pub mod insert;
pub mod json;
pub mod log;
pub mod migration;
#[cfg(any(
    feature = "runtime-mysql",
    feature = "runtime-postgres",
    feature = "runtime-sqlite"
))]
pub mod runtime;
pub mod schema;
pub mod select;
pub mod template;
pub mod triggers;
pub mod views;
pub mod update;
pub mod value;

pub use backend::Backend;
pub use cond::{Cond, Operator};
pub use delete::DeleteQuery;
pub use error::{QueryError, Result};
pub use insert::InsertQuery;
pub use json::{JsonPath, JsonSeg};
pub use log::{LogCategory, LogSink, Logger, Query};
#[cfg(feature = "derive")]
pub use medoo_derive::FromRow;
pub use migration::{tracking_table_sql, Migration, Migrator};
pub use schema::{AlterTable, ColDef, ColType, CreateDatabase, CreateTable, DropDatabase, DropTable};
pub use triggers::{CreateEvent, CreateTrigger, DropEvent, DropTrigger, TriggerEvent, TriggerTime};
pub use views::{CreateView, DropView};
pub use select::{OrderDir, SelectQuery};
pub use template::RawTemplate;
pub use update::UpdateQuery;
pub use value::{IntoValue, Value};

/// Punto de entrada estilo Medoo. No mantiene pool todavía: solo
/// construye queries y emite `(sql, params)`.
#[derive(Clone)]
pub struct Db {
    backend: Backend,
    logger: Option<std::sync::Arc<Logger>>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("backend", &self.backend)
            .field("logger", &self.logger.is_some())
            .finish()
    }
}

impl Db {
    pub fn new(backend: Backend) -> Self {
        Self { backend, logger: None }
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// Adjunta un logger. El logger se invoca al llamar `Db::build()`,
    /// `Db::log_ddl()` o `Db::log_raw()`.
    pub fn with_logger(mut self, logger: Logger) -> Self {
        self.logger = Some(std::sync::Arc::new(logger));
        self
    }

    pub fn logger(&self) -> Option<&Logger> {
        self.logger.as_deref()
    }

    /// Construye SQL parametrizado y lo loguea con la categoría
    /// reportada por la query. Equivalente a `q.to_sql()` + log.
    pub fn build<Q: Query>(&self, q: &Q) -> Result<(String, Vec<Value>)> {
        let r = q.build_sql()?;
        if let Some(l) = &self.logger {
            l.log(q.category(), &r.0, &r.1);
        }
        Ok(r)
    }

    pub fn log_ddl(&self, sql: &str) {
        if let Some(l) = &self.logger {
            l.log(LogCategory::DDL, sql, &[]);
        }
    }

    pub fn log_raw(&self, sql: &str, params: &[Value]) {
        if let Some(l) = &self.logger {
            l.log(LogCategory::RAW, sql, params);
        }
    }

    /// Wrappea cualquier query con `EXPLAIN`. Devuelve `(sql, params)`
    /// listo para ejecutar con `pool.fetch_all_raw(...)`.
    pub fn explain<Q: Query>(&self, q: &Q) -> Result<(String, Vec<Value>)> {
        let (sql, params) = q.build_sql()?;
        Ok((format!("EXPLAIN {}", sql), params))
    }

    /// `EXPLAIN ANALYZE` (PG / MySQL 8.0.18+) o `EXPLAIN QUERY PLAN`
    /// (SQLite). Ejecuta la query y reporta su plan + métricas.
    pub fn explain_analyze<Q: Query>(&self, q: &Q) -> Result<(String, Vec<Value>)> {
        let (sql, params) = q.build_sql()?;
        let prefix = match self.backend {
            Backend::Sqlite => "EXPLAIN QUERY PLAN ",
            _ => "EXPLAIN ANALYZE ",
        };
        Ok((format!("{}{}", prefix, sql), params))
    }

    pub fn select(&self, table: &str) -> SelectQuery {
        SelectQuery::new(self.backend, table)
    }

    pub fn insert(&self, table: &str) -> InsertQuery {
        InsertQuery::new(self.backend, table)
    }

    pub fn update(&self, table: &str) -> UpdateQuery {
        UpdateQuery::new(self.backend, table)
    }

    pub fn delete(&self, table: &str) -> DeleteQuery {
        DeleteQuery::new(self.backend, table)
    }

    /// Crea un template SQL con placeholders `{{WHERE}}`, `{{ORDER}}`,
    /// `{{GROUP}}`, `{{HAVING}}`, `{{LIMIT}}`, `{{OFFSET}}`. Ver
    /// [`crate::template`].
    pub fn raw_template<S: Into<String>>(&self, template: S) -> RawTemplate {
        RawTemplate::new(self.backend, template)
    }

    pub fn create_table(&self, table: &str) -> CreateTable {
        CreateTable::new(self.backend, table)
    }

    pub fn drop_table(&self, table: &str) -> DropTable {
        DropTable::new(self.backend, table)
    }

    pub fn alter_table(&self, table: &str) -> AlterTable {
        AlterTable::new(self.backend, table)
    }

    pub fn create_database(&self, name: &str) -> CreateDatabase {
        CreateDatabase::new(self.backend, name)
    }

    pub fn drop_database(&self, name: &str) -> DropDatabase {
        DropDatabase::new(self.backend, name)
    }

    pub fn create_view(&self, name: &str) -> CreateView {
        CreateView::new(self.backend, name)
    }
    pub fn drop_view(&self, name: &str) -> DropView {
        DropView::new(self.backend, name)
    }
    pub fn create_trigger(&self, name: &str) -> CreateTrigger {
        CreateTrigger::new(self.backend, name)
    }
    pub fn drop_trigger(&self, name: &str) -> DropTrigger {
        DropTrigger::new(self.backend, name)
    }
    pub fn create_event(&self, name: &str) -> CreateEvent {
        CreateEvent::new(self.backend, name)
    }
    pub fn drop_event(&self, name: &str) -> DropEvent {
        DropEvent::new(self.backend, name)
    }
}

#[macro_use]
mod macros;
