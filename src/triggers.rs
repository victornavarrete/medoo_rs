//! Triggers y events. El `body` se trata como SQL crudo: la lib
//! valida el `name` y la `table`, pero el cuerpo es responsabilidad del
//! dev (no debería venir nunca de input de usuario).

use crate::backend::Backend;
use crate::error::{QueryError, Result};
use crate::ident;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerTime {
    Before,
    After,
    /// Solo SQLite y PG (PG usa "INSTEAD OF" para vistas).
    InsteadOf,
}

impl TriggerTime {
    fn as_sql(&self) -> &'static str {
        match self {
            TriggerTime::Before => "BEFORE",
            TriggerTime::After => "AFTER",
            TriggerTime::InsteadOf => "INSTEAD OF",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerEvent {
    Insert,
    Update,
    Delete,
}

impl TriggerEvent {
    fn as_sql(&self) -> &'static str {
        match self {
            TriggerEvent::Insert => "INSERT",
            TriggerEvent::Update => "UPDATE",
            TriggerEvent::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateTrigger {
    backend: Backend,
    name: String,
    time: Option<TriggerTime>,
    event: Option<TriggerEvent>,
    table: String,
    when_condition: Option<String>,
    body: Option<String>,        // MySQL/SQLite: cuerpo SQL
    pg_function: Option<String>, // PG: nombre de función a invocar
    if_not_exists: bool,
}

impl CreateTrigger {
    pub fn new(backend: Backend, name: &str) -> Self {
        Self {
            backend,
            name: name.to_string(),
            time: None,
            event: None,
            table: String::new(),
            when_condition: None,
            body: None,
            pg_function: None,
            if_not_exists: false,
        }
    }
    pub fn before(mut self) -> Self {
        self.time = Some(TriggerTime::Before);
        self
    }
    pub fn after(mut self) -> Self {
        self.time = Some(TriggerTime::After);
        self
    }
    pub fn instead_of(mut self) -> Self {
        self.time = Some(TriggerTime::InsteadOf);
        self
    }
    pub fn insert(mut self) -> Self {
        self.event = Some(TriggerEvent::Insert);
        self
    }
    pub fn update(mut self) -> Self {
        self.event = Some(TriggerEvent::Update);
        self
    }
    pub fn delete(mut self) -> Self {
        self.event = Some(TriggerEvent::Delete);
        self
    }
    pub fn on_table<S: Into<String>>(mut self, table: S) -> Self {
        self.table = table.into();
        self
    }
    /// Condición opcional `WHEN (...)`. Soportado en PG y SQLite.
    /// MySQL no la soporta — se ignora.
    pub fn when<S: Into<String>>(mut self, condition: S) -> Self {
        self.when_condition = Some(condition.into());
        self
    }
    /// Cuerpo SQL del trigger (MySQL/SQLite). Ej: `"INSERT INTO log VALUES(NEW.id)"`.
    pub fn body<S: Into<String>>(mut self, body: S) -> Self {
        self.body = Some(body.into());
        self
    }
    /// PG: nombre de la función pre-existente que el trigger ejecuta.
    /// Ej: `.execute_function("audit_users_fn")` → `EXECUTE FUNCTION audit_users_fn()`
    pub fn execute_function<S: Into<String>>(mut self, fn_name: S) -> Self {
        self.pg_function = Some(fn_name.into());
        self
    }
    pub fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }

    pub fn to_sql(&self) -> Result<String> {
        ident::validate(&self.name)?;
        if self.table.is_empty() {
            return Err(QueryError::InvalidIdentifier("on_table required".into()));
        }
        ident::validate(&self.table)?;
        let time = self.time.ok_or_else(|| {
            QueryError::InvalidIdentifier("trigger time required (.before/.after/.instead_of)".into())
        })?;
        let event = self.event.ok_or_else(|| {
            QueryError::InvalidIdentifier("trigger event required (.insert/.update/.delete)".into())
        })?;

        let qname = self.backend.quote_ident(&self.name);
        let qtable = self.backend.quote_ident(&self.table);
        let head = if self.if_not_exists {
            // MySQL no soporta IF NOT EXISTS en CREATE TRIGGER hasta versión muy nueva
            // PG y SQLite sí.
            match self.backend {
                Backend::MySql => "CREATE TRIGGER",
                _ => "CREATE TRIGGER IF NOT EXISTS",
            }
        } else {
            "CREATE TRIGGER"
        };

        match self.backend {
            Backend::Postgres => {
                let fn_name = self.pg_function.as_ref().ok_or_else(|| {
                    QueryError::InvalidIdentifier(
                        "PG: .execute_function() required (PG ejecuta función, no body inline)".into(),
                    )
                })?;
                ident::validate(fn_name)?;
                let mut sql = format!(
                    "{} {} {} {} ON {} FOR EACH ROW",
                    head, qname, time.as_sql(), event.as_sql(), qtable
                );
                if let Some(when) = &self.when_condition {
                    sql.push_str(&format!(" WHEN ({})", when));
                }
                sql.push_str(&format!(" EXECUTE FUNCTION {}()", self.backend.quote_ident(fn_name)));
                Ok(sql)
            }
            Backend::MySql => {
                let body = self.body.as_ref().ok_or_else(|| {
                    QueryError::InvalidIdentifier("MySQL: .body() required".into())
                })?;
                Ok(format!(
                    "{} {} {} {} ON {} FOR EACH ROW BEGIN {}; END",
                    head, qname, time.as_sql(), event.as_sql(), qtable, body.trim_end_matches(';')
                ))
            }
            Backend::Sqlite => {
                let body = self.body.as_ref().ok_or_else(|| {
                    QueryError::InvalidIdentifier("SQLite: .body() required".into())
                })?;
                let mut sql = format!(
                    "{} {} {} {} ON {} FOR EACH ROW",
                    head, qname, time.as_sql(), event.as_sql(), qtable
                );
                if let Some(when) = &self.when_condition {
                    sql.push_str(&format!(" WHEN {}", when));
                }
                sql.push_str(&format!(" BEGIN {}; END", body.trim_end_matches(';')));
                Ok(sql)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct DropTrigger {
    backend: Backend,
    name: String,
    table: Option<String>, // MySQL no requiere ON; PG/SQLite tampoco en sintaxis básica
    if_exists: bool,
}

impl DropTrigger {
    pub fn new(backend: Backend, name: &str) -> Self {
        Self {
            backend,
            name: name.to_string(),
            table: None,
            if_exists: false,
        }
    }
    pub fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }
    /// PG necesita `ON tabla` en DROP TRIGGER. MySQL/SQLite no.
    pub fn on_table<S: Into<String>>(mut self, t: S) -> Self {
        self.table = Some(t.into());
        self
    }
    pub fn to_sql(&self) -> Result<String> {
        ident::validate(&self.name)?;
        let mut sql = String::from("DROP TRIGGER");
        if self.if_exists {
            sql.push_str(" IF EXISTS");
        }
        sql.push(' ');
        sql.push_str(&self.backend.quote_ident(&self.name));
        if self.backend == Backend::Postgres {
            let t = self.table.as_ref().ok_or_else(|| {
                QueryError::InvalidIdentifier("PG: DROP TRIGGER requiere .on_table()".into())
            })?;
            ident::validate(t)?;
            sql.push_str(&format!(" ON {}", self.backend.quote_ident(t)));
        }
        Ok(sql)
    }
}

// ============= Events (MySQL only) =============

#[derive(Debug, Clone)]
pub struct CreateEvent {
    backend: Backend,
    name: String,
    schedule: Option<String>, // ej: "EVERY 1 DAY", "AT '2026-12-31 23:59:59'"
    body: Option<String>,
    if_not_exists: bool,
}

impl CreateEvent {
    pub fn new(backend: Backend, name: &str) -> Self {
        Self {
            backend,
            name: name.to_string(),
            schedule: None,
            body: None,
            if_not_exists: false,
        }
    }
    pub fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }
    /// `EVERY 1 DAY` / `EVERY 1 HOUR STARTS ...` / `AT '<datetime>'`.
    /// El string va literal; el dev es responsable.
    pub fn schedule<S: Into<String>>(mut self, expr: S) -> Self {
        self.schedule = Some(expr.into());
        self
    }
    pub fn body<S: Into<String>>(mut self, body: S) -> Self {
        self.body = Some(body.into());
        self
    }
    pub fn to_sql(&self) -> Result<String> {
        if self.backend != Backend::MySql {
            return Err(QueryError::InvalidOperator(
                "events solo en MySQL/MariaDB".to_string(),
            ));
        }
        ident::validate(&self.name)?;
        let schedule = self.schedule.as_ref().ok_or_else(|| {
            QueryError::InvalidIdentifier(".schedule() required".into())
        })?;
        let body = self.body.as_ref().ok_or_else(|| {
            QueryError::InvalidIdentifier(".body() required".into())
        })?;
        let head = if self.if_not_exists {
            "CREATE EVENT IF NOT EXISTS"
        } else {
            "CREATE EVENT"
        };
        Ok(format!(
            "{} {} ON SCHEDULE {} DO {}",
            head,
            self.backend.quote_ident(&self.name),
            schedule,
            body.trim_end_matches(';')
        ))
    }
}

#[derive(Debug, Clone)]
pub struct DropEvent {
    backend: Backend,
    name: String,
    if_exists: bool,
}

impl DropEvent {
    pub fn new(backend: Backend, name: &str) -> Self {
        Self { backend, name: name.to_string(), if_exists: false }
    }
    pub fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }
    pub fn to_sql(&self) -> Result<String> {
        if self.backend != Backend::MySql {
            return Err(QueryError::InvalidOperator(
                "events solo en MySQL/MariaDB".to_string(),
            ));
        }
        ident::validate(&self.name)?;
        let mut sql = String::from("DROP EVENT");
        if self.if_exists {
            sql.push_str(" IF EXISTS");
        }
        sql.push(' ');
        sql.push_str(&self.backend.quote_ident(&self.name));
        Ok(sql)
    }
}
