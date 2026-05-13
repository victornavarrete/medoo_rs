//! DDL: `CREATE TABLE`, `DROP TABLE`, `ALTER TABLE`. Genera SQL
//! parametrizado por backend con identifiers validados. No incluye
//! abstracción de tipos exhaustiva — los tipos comunes están
//! cubiertos; lo raro va vía `ColType::Raw("...")`.

use crate::backend::Backend;
use crate::error::{QueryError, Result};
use crate::ident;

/// Valida nombres de charset / collation. Whitelist `[A-Za-z0-9_.-]`,
/// máximo 64 chars. Acepta `.` y `-` para soportar locales POSIX de
/// Postgres (`en_US.UTF-8`, `C.UTF-8`). Sigue siendo seguro: no entran
/// quotes, espacios, ni metacaracteres SQL.
fn validate_collation_name(s: &str) -> Result<()> {
    if s.is_empty() || s.len() > 64 {
        return Err(QueryError::InvalidIdentifier(s.to_string()));
    }
    for c in s.chars() {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-') {
            return Err(QueryError::InvalidIdentifier(s.to_string()));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub enum ColType {
    /// MySQL TINYINT (1 byte). En PG/SQLite cae a SMALLINT/INTEGER.
    TinyInt,
    /// SMALLINT (2 bytes).
    SmallInt,
    Int,
    BigInt,
    /// `DECIMAL(p,s)` / `NUMERIC(p,s)`. Útil para plata. Al leer desde
    /// el pool retorna `Value::Text` para no perder precisión vs f64.
    Decimal(u32, u32),
    Bool,
    Float,
    Double,
    Text,
    /// MySQL `TINYTEXT` (255). PG/SQLite caen a TEXT.
    TinyText,
    /// MySQL `MEDIUMTEXT` (16 MB). PG/SQLite caen a TEXT.
    MediumText,
    /// MySQL `LONGTEXT` (4 GB). PG/SQLite caen a TEXT.
    LongText,
    Char(u32),
    Varchar(u32),
    /// `UUID` en PG; `CHAR(36)` en MySQL/SQLite (forma string).
    Uuid,
    Bytes,
    /// MySQL `BINARY(n)`. PG/SQLite caen a BYTEA/BLOB.
    Binary(u32),
    /// MySQL `VARBINARY(n)`. PG/SQLite caen a BYTEA/BLOB.
    VarBinary(u32),
    /// MySQL `TINYBLOB` / `MEDIUMBLOB` / `LONGBLOB`. Otros: BYTEA/BLOB.
    TinyBlob,
    MediumBlob,
    LongBlob,
    Json,
    /// **TZ-aware**: `TIMESTAMPTZ` (PG) / `TIMESTAMP` (MySQL: guarda
    /// en UTC y convierte a session TZ al leer) / `TEXT` (SQLite, vos
    /// guardás ISO 8601 con offset). Usá esta para instantes globales.
    Timestamp,
    /// **TZ-naive**: `TIMESTAMP` (PG, sin TZ) / `DATETIME` (MySQL,
    /// wall-clock crudo) / `TEXT` (SQLite). Usá esta cuando la fecha/
    /// hora representa un momento local que no debe ser convertido.
    DateTime,
    /// `DATE` en todos los backends (SQLite igual lo guarda como TEXT).
    Date,
    /// `TIME` (sin TZ) en MySQL/PG; `TEXT` en SQLite.
    Time,
    /// `TIME WITH TIME ZONE` (PG). MySQL/SQLite caen a TEXT — vos
    /// guardás el offset en el string.
    TimeTz,
    /// MySQL `YEAR`. PG/SQLite: SMALLINT.
    Year,
    /// MySQL `ENUM('a','b','c')`. En Postgres se cae a `TEXT` (PG tiene
    /// CREATE TYPE separado, no inline). En SQLite también `TEXT`.
    Enum(Vec<String>),
    /// MySQL `SET('a','b','c')`. Fuera de MySQL se cae a `TEXT`.
    Set(Vec<String>),
    Raw(String),
}

impl ColType {
    pub fn render(&self, backend: Backend) -> String {
        match (self, backend) {
            (ColType::TinyInt, Backend::MySql) => "TINYINT".into(),
            (ColType::TinyInt, _) => "SMALLINT".into(),
            (ColType::SmallInt, Backend::Sqlite) => "INTEGER".into(),
            (ColType::SmallInt, _) => "SMALLINT".into(),
            (ColType::Int, _) => "INTEGER".into(),
            (ColType::BigInt, Backend::Sqlite) => "INTEGER".into(),
            (ColType::BigInt, _) => "BIGINT".into(),
            (ColType::Decimal(p, s), _) => format!("DECIMAL({},{})", p, s),
            (ColType::Bool, Backend::Postgres) => "BOOLEAN".into(),
            (ColType::Bool, Backend::MySql) => "TINYINT(1)".into(),
            (ColType::Bool, Backend::Sqlite) => "INTEGER".into(),
            (ColType::Float, _) => "REAL".into(),
            (ColType::Double, Backend::Postgres) => "DOUBLE PRECISION".into(),
            (ColType::Double, _) => "DOUBLE".into(),
            (ColType::Text, _) => "TEXT".into(),
            (ColType::TinyText, Backend::MySql) => "TINYTEXT".into(),
            (ColType::TinyText, _) => "TEXT".into(),
            (ColType::MediumText, Backend::MySql) => "MEDIUMTEXT".into(),
            (ColType::MediumText, _) => "TEXT".into(),
            (ColType::LongText, Backend::MySql) => "LONGTEXT".into(),
            (ColType::LongText, _) => "TEXT".into(),
            (ColType::Char(n), _) => format!("CHAR({})", n),
            (ColType::Varchar(n), _) => format!("VARCHAR({})", n),
            (ColType::Uuid, Backend::Postgres) => "UUID".into(),
            (ColType::Uuid, _) => "CHAR(36)".into(),
            (ColType::Bytes, Backend::Postgres) => "BYTEA".into(),
            (ColType::Bytes, _) => "BLOB".into(),
            (ColType::Binary(n), Backend::MySql) => format!("BINARY({})", n),
            (ColType::Binary(_), Backend::Postgres) => "BYTEA".into(),
            (ColType::Binary(_), Backend::Sqlite) => "BLOB".into(),
            (ColType::VarBinary(n), Backend::MySql) => format!("VARBINARY({})", n),
            (ColType::VarBinary(_), Backend::Postgres) => "BYTEA".into(),
            (ColType::VarBinary(_), Backend::Sqlite) => "BLOB".into(),
            (ColType::TinyBlob, Backend::MySql) => "TINYBLOB".into(),
            (ColType::TinyBlob, Backend::Postgres) => "BYTEA".into(),
            (ColType::TinyBlob, Backend::Sqlite) => "BLOB".into(),
            (ColType::MediumBlob, Backend::MySql) => "MEDIUMBLOB".into(),
            (ColType::MediumBlob, Backend::Postgres) => "BYTEA".into(),
            (ColType::MediumBlob, Backend::Sqlite) => "BLOB".into(),
            (ColType::LongBlob, Backend::MySql) => "LONGBLOB".into(),
            (ColType::LongBlob, Backend::Postgres) => "BYTEA".into(),
            (ColType::LongBlob, Backend::Sqlite) => "BLOB".into(),
            (ColType::Json, Backend::Postgres) => "JSONB".into(),
            (ColType::Json, Backend::MySql) => "JSON".into(),
            (ColType::Json, Backend::Sqlite) => "TEXT".into(),
            (ColType::Timestamp, Backend::Postgres) => "TIMESTAMPTZ".into(),
            (ColType::Timestamp, Backend::MySql) => "TIMESTAMP".into(),
            (ColType::Timestamp, Backend::Sqlite) => "TEXT".into(),
            (ColType::DateTime, Backend::Postgres) => "TIMESTAMP".into(),
            (ColType::DateTime, Backend::MySql) => "DATETIME".into(),
            (ColType::DateTime, Backend::Sqlite) => "TEXT".into(),
            (ColType::Date, _) => "DATE".into(),
            (ColType::Time, Backend::Sqlite) => "TEXT".into(),
            (ColType::Time, _) => "TIME".into(),
            (ColType::TimeTz, Backend::Postgres) => "TIMETZ".into(),
            (ColType::TimeTz, _) => "TEXT".into(),
            (ColType::Year, Backend::MySql) => "YEAR".into(),
            (ColType::Year, Backend::Sqlite) => "INTEGER".into(),
            (ColType::Year, _) => "SMALLINT".into(),
            (ColType::Enum(vals), Backend::MySql) => format!("ENUM({})", quote_str_list(vals)),
            (ColType::Enum(_), _) => "TEXT".into(),
            (ColType::Set(vals), Backend::MySql) => format!("SET({})", quote_str_list(vals)),
            (ColType::Set(_), _) => "TEXT".into(),
            (ColType::Raw(s), _) => s.clone(),
        }
    }
}

/// Cita una lista de strings para ENUM/SET. Asume valores ya validados
/// (sin `'` ni `\`).
fn quote_str_list(vals: &[String]) -> String {
    vals.iter()
        .map(|v| format!("'{}'", v))
        .collect::<Vec<_>>()
        .join(",")
}

/// Verifica que los valores de un ENUM/SET no contengan caracteres que
/// puedan romper el quoting (`'` o `\`). Llamar antes de renderizar.
fn validate_enum_values(vals: &[String]) -> Result<()> {
    if vals.is_empty() {
        return Err(QueryError::EmptyRecord);
    }
    for v in vals {
        if v.contains('\'') || v.contains('\\') {
            return Err(QueryError::InvalidIdentifier(v.clone()));
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ColDef {
    pub name: String,
    pub ty: ColType,
    pub nullable: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub unique: bool,
    pub default_raw: Option<String>, // sin parámetros: literales SQL como "now()" o "0"
    pub charset: Option<String>,     // MySQL: CHARACTER SET <name>
    pub collation: Option<String>,   // MySQL: COLLATE <name> · PG/SQLite: COLLATE "<name>"
}

impl ColDef {
    pub fn new<S: Into<String>>(name: S, ty: ColType) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable: true,
            primary_key: false,
            auto_increment: false,
            unique: false,
            default_raw: None,
            charset: None,
            collation: None,
        }
    }
    pub fn charset<S: Into<String>>(mut self, s: S) -> Self {
        self.charset = Some(s.into());
        self
    }
    pub fn collation<S: Into<String>>(mut self, s: S) -> Self {
        self.collation = Some(s.into());
        self
    }
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }
    pub fn auto_increment(mut self) -> Self {
        self.auto_increment = true;
        self
    }
    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }
    pub fn default_raw<S: Into<String>>(mut self, expr: S) -> Self {
        let expr = expr.into();
        // bloqueo básico anti-injection: prohibir `;` y `--`.
        let safe = !expr.contains(';') && !expr.contains("--");
        if !safe {
            // se pospone al render: marcamos con el propio string para
            // que falle ahí. Más explícito:
        }
        self.default_raw = Some(expr);
        self
    }

    fn render(&self, backend: Backend) -> Result<String> {
        ident::validate(&self.name)?;
        // ENUM/SET: validar valores antes de renderizar
        match &self.ty {
            ColType::Enum(v) | ColType::Set(v) => validate_enum_values(v)?,
            _ => {}
        }
        let mut parts: Vec<String> = Vec::new();
        parts.push(backend.quote_ident(&self.name));

        // tipo + auto_increment ajustado por backend
        let type_sql = match (self.auto_increment, backend) {
            (true, Backend::Postgres) => match self.ty {
                ColType::BigInt => "BIGSERIAL".into(),
                _ => "SERIAL".into(),
            },
            (true, Backend::Sqlite) => "INTEGER".into(),
            _ => self.ty.render(backend),
        };
        parts.push(type_sql);

        // CHARACTER SET / COLLATE — antes de PK/NOT NULL/DEFAULT por
        // convención MySQL/Postgres.
        if let Some(cs) = &self.charset {
            validate_collation_name(cs)?;
            if backend == Backend::MySql {
                parts.push(format!("CHARACTER SET {}", cs));
            }
            // Postgres y SQLite no tienen charset por columna: ignoramos.
        }
        if let Some(co) = &self.collation {
            validate_collation_name(co)?;
            match backend {
                Backend::MySql => parts.push(format!("COLLATE {}", co)),
                Backend::Postgres => parts.push(format!("COLLATE \"{}\"", co)),
                Backend::Sqlite => parts.push(format!("COLLATE {}", co)),
            }
        }

        if self.primary_key {
            parts.push("PRIMARY KEY".into());
            if self.auto_increment && backend == Backend::Sqlite {
                parts.push("AUTOINCREMENT".into());
            }
        }
        if self.auto_increment && backend == Backend::MySql {
            parts.push("AUTO_INCREMENT".into());
        }
        if !self.nullable && !self.primary_key {
            parts.push("NOT NULL".into());
        }
        if self.unique && !self.primary_key {
            parts.push("UNIQUE".into());
        }
        if let Some(d) = &self.default_raw {
            if d.contains(';') || d.contains("--") {
                return Err(QueryError::InvalidIdentifier(d.clone()));
            }
            parts.push(format!("DEFAULT {}", d));
        }
        Ok(parts.join(" "))
    }
}

#[derive(Debug, Clone)]
pub struct CreateTable {
    backend: Backend,
    table: String,
    if_not_exists: bool,
    columns: Vec<ColDef>,
    primary_key_compound: Vec<String>,
    default_charset: Option<String>,
    default_collation: Option<String>,
    engine: Option<String>,
}

impl CreateTable {
    pub fn new(backend: Backend, table: &str) -> Self {
        Self {
            backend,
            table: table.to_string(),
            if_not_exists: false,
            columns: Vec::new(),
            primary_key_compound: Vec::new(),
            default_charset: None,
            default_collation: None,
            engine: None,
        }
    }
    /// `DEFAULT CHARSET=<name>` (MySQL). En Postgres/SQLite se ignora.
    pub fn default_charset<S: Into<String>>(mut self, s: S) -> Self {
        self.default_charset = Some(s.into());
        self
    }
    /// `COLLATE=<name>` a nivel tabla (MySQL). En Postgres/SQLite se ignora.
    pub fn default_collation<S: Into<String>>(mut self, s: S) -> Self {
        self.default_collation = Some(s.into());
        self
    }
    /// `ENGINE=<name>` (MySQL). En otros backends se ignora.
    pub fn engine<S: Into<String>>(mut self, s: S) -> Self {
        self.engine = Some(s.into());
        self
    }
    pub fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }
    pub fn col(mut self, c: ColDef) -> Self {
        self.columns.push(c);
        self
    }
    pub fn primary_key<I: IntoIterator<Item = S>, S: Into<String>>(mut self, cols: I) -> Self {
        self.primary_key_compound = cols.into_iter().map(|s| s.into()).collect();
        self
    }
    pub fn to_sql(&self) -> Result<String> {
        if self.columns.is_empty() {
            return Err(QueryError::EmptyRecord);
        }
        let mut col_sql: Vec<String> = Vec::new();
        for c in &self.columns {
            col_sql.push(c.render(self.backend)?);
        }
        if !self.primary_key_compound.is_empty() {
            let mut qcols: Vec<String> = Vec::new();
            for c in &self.primary_key_compound {
                qcols.push(ident::quote(self.backend, c)?);
            }
            col_sql.push(format!("PRIMARY KEY ({})", qcols.join(", ")));
        }
        let head = if self.if_not_exists { "CREATE TABLE IF NOT EXISTS" } else { "CREATE TABLE" };
        let mut sql = format!(
            "{} {} ({})",
            head,
            ident::quote(self.backend, &self.table)?,
            col_sql.join(", ")
        );
        // Suffix por backend: ENGINE / CHARSET / COLLATE solo aplican en MySQL.
        if self.backend == Backend::MySql {
            if let Some(eng) = &self.engine {
                validate_collation_name(eng)?;
                sql.push_str(&format!(" ENGINE={}", eng));
            }
            if let Some(cs) = &self.default_charset {
                validate_collation_name(cs)?;
                sql.push_str(&format!(" DEFAULT CHARSET={}", cs));
            }
            if let Some(co) = &self.default_collation {
                validate_collation_name(co)?;
                sql.push_str(&format!(" COLLATE={}", co));
            }
        } else {
            // Validamos igual aunque no se emitan, por consistencia.
            if let Some(s) = &self.default_charset {
                validate_collation_name(s)?;
            }
            if let Some(s) = &self.default_collation {
                validate_collation_name(s)?;
            }
            if let Some(s) = &self.engine {
                validate_collation_name(s)?;
            }
        }
        Ok(sql)
    }
}

#[derive(Debug, Clone)]
pub struct DropTable {
    backend: Backend,
    table: String,
    if_exists: bool,
    cascade: bool,
}

impl DropTable {
    pub fn new(backend: Backend, table: &str) -> Self {
        Self { backend, table: table.to_string(), if_exists: false, cascade: false }
    }
    pub fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }
    pub fn cascade(mut self) -> Self {
        self.cascade = true;
        self
    }
    pub fn to_sql(&self) -> Result<String> {
        let mut s = String::from("DROP TABLE");
        if self.if_exists {
            s.push_str(" IF EXISTS");
        }
        s.push(' ');
        s.push_str(&ident::quote(self.backend, &self.table)?);
        if self.cascade && self.backend == Backend::Postgres {
            s.push_str(" CASCADE");
        }
        Ok(s)
    }
}

#[derive(Debug, Clone)]
pub enum AlterAction {
    AddColumn(ColDef),
    DropColumn(String),
    RenameColumn { from: String, to: String },
    RenameTable { to: String },
    /// MySQL: `CONVERT TO CHARACTER SET ... COLLATE ...` (convierte datos).
    /// PG/SQLite: skip silencioso (no tienen equivalente).
    ConvertToCharset { charset: String, collation: Option<String> },
    /// MySQL: `DEFAULT CHARACTER SET ... COLLATE ...` (solo default,
    /// no convierte datos existentes).
    SetDefaultCharset { charset: String, collation: Option<String> },
    /// MySQL: `ENGINE=...`.
    SetEngine(String),
}

#[derive(Debug, Clone)]
pub struct AlterTable {
    backend: Backend,
    table: String,
    actions: Vec<AlterAction>,
}

impl AlterTable {
    pub fn new(backend: Backend, table: &str) -> Self {
        Self { backend, table: table.to_string(), actions: Vec::new() }
    }
    pub fn add_column(mut self, c: ColDef) -> Self {
        self.actions.push(AlterAction::AddColumn(c));
        self
    }
    pub fn drop_column<S: Into<String>>(mut self, name: S) -> Self {
        self.actions.push(AlterAction::DropColumn(name.into()));
        self
    }
    pub fn rename_column<A: Into<String>, B: Into<String>>(mut self, from: A, to: B) -> Self {
        self.actions.push(AlterAction::RenameColumn { from: from.into(), to: to.into() });
        self
    }
    pub fn rename_table<S: Into<String>>(mut self, to: S) -> Self {
        self.actions.push(AlterAction::RenameTable { to: to.into() });
        self
    }
    /// `CONVERT TO CHARACTER SET <cs>` (MySQL). Convierte datos existentes.
    pub fn convert_to_charset<S: Into<String>>(mut self, charset: S) -> Self {
        self.actions.push(AlterAction::ConvertToCharset {
            charset: charset.into(),
            collation: None,
        });
        self
    }
    /// `CONVERT TO CHARACTER SET <cs> COLLATE <co>` (MySQL).
    pub fn convert_to_charset_collate<A: Into<String>, B: Into<String>>(
        mut self,
        charset: A,
        collation: B,
    ) -> Self {
        self.actions.push(AlterAction::ConvertToCharset {
            charset: charset.into(),
            collation: Some(collation.into()),
        });
        self
    }
    /// `DEFAULT CHARACTER SET <cs>` (MySQL). No toca datos existentes.
    pub fn set_default_charset<S: Into<String>>(mut self, charset: S) -> Self {
        self.actions.push(AlterAction::SetDefaultCharset {
            charset: charset.into(),
            collation: None,
        });
        self
    }
    /// `DEFAULT CHARACTER SET <cs> COLLATE <co>` (MySQL).
    pub fn set_default_charset_collate<A: Into<String>, B: Into<String>>(
        mut self,
        charset: A,
        collation: B,
    ) -> Self {
        self.actions.push(AlterAction::SetDefaultCharset {
            charset: charset.into(),
            collation: Some(collation.into()),
        });
        self
    }
    /// `ENGINE=<name>` (MySQL).
    pub fn set_engine<S: Into<String>>(mut self, engine: S) -> Self {
        self.actions.push(AlterAction::SetEngine(engine.into()));
        self
    }

    /// Retorna una o más sentencias (algunos backends no soportan
    /// múltiples acciones en un solo ALTER).
    pub fn to_sql(&self) -> Result<Vec<String>> {
        let qtable = ident::quote(self.backend, &self.table)?;
        let mut out = Vec::new();
        for a in &self.actions {
            match a {
                AlterAction::AddColumn(c) => {
                    out.push(format!("ALTER TABLE {} ADD COLUMN {}", qtable, c.render(self.backend)?));
                }
                AlterAction::DropColumn(n) => {
                    out.push(format!(
                        "ALTER TABLE {} DROP COLUMN {}",
                        qtable,
                        ident::quote(self.backend, n)?
                    ));
                }
                AlterAction::RenameColumn { from, to } => {
                    let qf = ident::quote(self.backend, from)?;
                    let qt = ident::quote(self.backend, to)?;
                    out.push(match self.backend {
                        Backend::MySql => format!("ALTER TABLE {} CHANGE {} {} /* MySQL: añade tipo si lo necesitás vía Raw */", qtable, qf, qt),
                        _ => format!("ALTER TABLE {} RENAME COLUMN {} TO {}", qtable, qf, qt),
                    });
                }
                AlterAction::RenameTable { to } => {
                    let qt = ident::quote(self.backend, to)?;
                    out.push(match self.backend {
                        Backend::MySql => format!("RENAME TABLE {} TO {}", qtable, qt),
                        _ => format!("ALTER TABLE {} RENAME TO {}", qtable, qt),
                    });
                }
                AlterAction::ConvertToCharset { charset, collation } => {
                    validate_collation_name(charset)?;
                    if let Some(co) = collation {
                        validate_collation_name(co)?;
                    }
                    if self.backend == Backend::MySql {
                        let mut s = format!("ALTER TABLE {} CONVERT TO CHARACTER SET {}", qtable, charset);
                        if let Some(co) = collation {
                            s.push_str(&format!(" COLLATE {}", co));
                        }
                        out.push(s);
                    }
                    // PG/SQLite: skip silencioso (sin equivalente directo).
                }
                AlterAction::SetDefaultCharset { charset, collation } => {
                    validate_collation_name(charset)?;
                    if let Some(co) = collation {
                        validate_collation_name(co)?;
                    }
                    if self.backend == Backend::MySql {
                        let mut s = format!("ALTER TABLE {} DEFAULT CHARACTER SET {}", qtable, charset);
                        if let Some(co) = collation {
                            s.push_str(&format!(" COLLATE {}", co));
                        }
                        out.push(s);
                    }
                }
                AlterAction::SetEngine(eng) => {
                    validate_collation_name(eng)?;
                    if self.backend == Backend::MySql {
                        out.push(format!("ALTER TABLE {} ENGINE={}", qtable, eng));
                    }
                }
            }
        }
        Ok(out)
    }
}

// CREATE DATABASE / DROP DATABASE

#[derive(Debug, Clone)]
pub struct CreateDatabase {
    backend: Backend,
    name: String,
    if_not_exists: bool,
    charset: Option<String>,
    collation: Option<String>,
}

impl CreateDatabase {
    pub fn new(backend: Backend, name: &str) -> Self {
        Self {
            backend,
            name: name.to_string(),
            if_not_exists: false,
            charset: None,
            collation: None,
        }
    }
    pub fn if_not_exists(mut self) -> Self {
        self.if_not_exists = true;
        self
    }
    /// MySQL: `DEFAULT CHARSET=<name>` · Postgres: `ENCODING '<name>'`.
    pub fn default_charset<S: Into<String>>(mut self, s: S) -> Self {
        self.charset = Some(s.into());
        self
    }
    /// MySQL: `COLLATE=<name>` · Postgres: `LC_COLLATE '<name>'`.
    pub fn default_collation<S: Into<String>>(mut self, s: S) -> Self {
        self.collation = Some(s.into());
        self
    }
    pub fn to_sql(&self) -> Result<String> {
        ident::validate(&self.name)?;
        if self.backend == Backend::Sqlite {
            return Err(QueryError::InvalidOperator(
                "CREATE DATABASE no aplica en SQLite (un archivo = una BD)".to_string(),
            ));
        }
        let mut sql = String::from("CREATE DATABASE");
        if self.if_not_exists {
            sql.push_str(" IF NOT EXISTS");
        }
        sql.push(' ');
        sql.push_str(&self.backend.quote_ident(&self.name));
        match self.backend {
            Backend::MySql => {
                if let Some(cs) = &self.charset {
                    validate_collation_name(cs)?;
                    sql.push_str(&format!(" DEFAULT CHARSET={}", cs));
                }
                if let Some(co) = &self.collation {
                    validate_collation_name(co)?;
                    sql.push_str(&format!(" COLLATE={}", co));
                }
            }
            Backend::Postgres => {
                if let Some(cs) = &self.charset {
                    validate_collation_name(cs)?;
                    sql.push_str(&format!(" ENCODING '{}'", cs));
                }
                if let Some(co) = &self.collation {
                    validate_collation_name(co)?;
                    sql.push_str(&format!(" LC_COLLATE '{}'", co));
                }
            }
            Backend::Sqlite => unreachable!(),
        }
        Ok(sql)
    }
}

#[derive(Debug, Clone)]
pub struct DropDatabase {
    backend: Backend,
    name: String,
    if_exists: bool,
}

impl DropDatabase {
    pub fn new(backend: Backend, name: &str) -> Self {
        Self { backend, name: name.to_string(), if_exists: false }
    }
    pub fn if_exists(mut self) -> Self {
        self.if_exists = true;
        self
    }
    pub fn to_sql(&self) -> Result<String> {
        ident::validate(&self.name)?;
        if self.backend == Backend::Sqlite {
            return Err(QueryError::InvalidOperator(
                "DROP DATABASE no aplica en SQLite".to_string(),
            ));
        }
        let mut sql = String::from("DROP DATABASE");
        if self.if_exists {
            sql.push_str(" IF EXISTS");
        }
        sql.push(' ');
        sql.push_str(&self.backend.quote_ident(&self.name));
        Ok(sql)
    }
}
