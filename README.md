# medoo_rs

[![Crates.io](https://img.shields.io/crates/v/medoo_rs)](https://crates.io/crates/medoo_rs)
[![Docs](https://img.shields.io/docsrs/medoo_rs)](https://docs.rs/medoo_rs)
[![Downloads](https://img.shields.io/crates/d/medoo_rs)](https://crates.io/crates/medoo_rs)
[![License](https://img.shields.io/crates/l/medoo_rs)](#licencia)
[![MSRV](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-203%20passing-brightgreen)](#tests)

> 🇪🇸 **Solo hablo español** — la documentación, los comentarios del
> código y los mensajes de error están en español. Sorry / lo siento /
> sumimasen. PRs en inglés son bienvenidos igualmente.

Query builder dinámico inspirado en [Medoo](https://medoo.in) (PHP).
API plana, ergonómica, **multi-backend** (Postgres / MySQL / SQLite).
Núcleo sin dependencias: construye `(SQL, Vec<Value>)` parametrizado.
La capa async (pool, transacciones, streaming) se monta encima como
**feature opcional**.

> Cheat sheet rápida → [MANUAL.md](MANUAL.md)

## Tabla de contenidos

- [Filosofía](#filosofía)
- [Instalación](#instalación)
- [Features opcionales](#features-opcionales)
- [Quick start (núcleo)](#quick-start-núcleo)
- [Quick start (pool async)](#quick-start-pool-async)
- [Conceptos](#conceptos)
- [SELECT](#select)
- [INSERT / UPSERT / RETURNING](#insert--upsert--returning)
- [UPDATE / DELETE](#update--delete)
- [Macros](#macros)
- [JSON](#json)
- [DDL](#ddl)
- [Migraciones](#migraciones)
- [Logging](#logging)
- [Pool async — referencia completa](#pool-async--referencia-completa)
- [FromRow (mapeo a struct)](#fromrow-mapeo-a-struct)
- [chrono (fechas tipadas)](#chrono-fechas-tipadas)
- [Seguridad](#seguridad)
- [Tests](#tests)

---

## Filosofía

```rust
let mut q = db.select("users");
if let Some(s) = filter.status   { q = q.where_eq("status", s); }
if let Some(m) = filter.min_age  { q = q.where_op("age", ">", m); }
if filter.recent                 { q = q.order_desc("created_at"); }
if let Some(n) = filter.limit    { q = q.limit(n); }
let (sql, params) = q.to_sql()?;
```

Cada `if` agrega o no agrega. Sin clones, sin builders verbosos.
Como Medoo, pero rápido y seguro.

---

## Instalación

```toml
[dependencies]
medoo_rs = "0.1"
```

Compila con Rust estable. Núcleo **zero deps**.

**Linux / WSL:** `sudo apt install build-essential`.
**Windows:** Visual Studio Build Tools (*Desktop development with C++*).

---

## Features opcionales

Todas son opt-in. Sin activar ninguna, el núcleo permanece zero-deps
y solo emite `(sql, params)` — vos lo ejecutás contra el runtime que
prefieras.

| Feature              | Activa                                                    |
|----------------------|-----------------------------------------------------------|
| `runtime-mysql`      | Pool sqlx MySQL / MariaDB                                 |
| `runtime-postgres`   | Pool sqlx Postgres                                        |
| `runtime-sqlite`     | Pool sqlx SQLite                                          |
| `derive`             | `#[derive(FromRow)]` (proc-macro)                         |
| `chrono`             | `IntoValue` + accesores chrono para fechas tipadas        |

Los `runtime-*` arrastran `sqlx` + `tokio` + `futures`. Necesitás
`tokio` en tu app al activarlos.

```toml
medoo_rs = { version = "0.1", features = ["runtime-postgres", "derive", "chrono"] }
tokio    = { version = "1", features = ["macros", "rt-multi-thread"] }
```

---

## Quick start (núcleo)

```rust
use medoo_rs::{Backend, Db, LogCategory, Logger, record, where_};

let db = Db::new(Backend::Postgres)
    .with_logger(Logger::stdout().filter(LogCategory::WRITE | LogCategory::READ));

let q = db.select("users")
    .where_cond(where_!{ "status" => "active", "age" => [">", 18] })
    .order_desc("created_at")
    .limit(20);
let (sql, params) = db.build(&q)?;

let ins = db.insert("users").set(record!{ "name" => "ana", "age" => 30 });
let (sql, params) = db.build(&ins)?;
```

`db.build(&q)` corre `to_sql()` y emite log. Para SQL puro sin logger
usá `q.to_sql()` directo.

---

## Quick start (pool async)

```toml
[dependencies]
medoo_rs = { version = "0.1", features = ["runtime-postgres"] }
tokio    = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use medoo_rs::runtime::{Pool, RowExt};
use medoo_rs::{record, Logger, LogCategory};

#[tokio::main]
async fn main() -> medoo_rs::Result<()> {
    let pool = Pool::connect_postgres("postgres://user:pass@localhost/mydb").await?
        .with_logger(Logger::file("queries.log")?.filter(LogCategory::WRITE));

    let n = pool.execute(
        &pool.insert("users").set(record!{ "name" => "Ana", "age" => 30 })
    ).await?;

    let rows = pool.fetch_all(
        &pool.select("users").where_op("age", ">=", 18).order_desc("id")
    ).await?;
    for r in &rows {
        println!("{} ({})", r.get_str("name").unwrap_or(""), r.get_i64("age").unwrap_or(0));
    }
    Ok(())
}
```

Pool con opciones, transacciones, savepoints, retry, streaming y
`EXPLAIN`: ver [Pool async — referencia completa](#pool-async--referencia-completa).

---

## Conceptos

### `Db`
Punto de entrada del builder. No mantiene pool — solo emite SQL.
```rust
let db = Db::new(Backend::Postgres); // o Backend::MySql / Backend::Sqlite
```

### `Value`
```rust
pub enum Value { Null, Bool, Int, Float, Text, Bytes, Json }
```
`IntoValue` cubre `bool`, `i8..i64`, `u8..u64`, `f32/f64`, `&str`,
`String`, `Option<T>`. Se infiere automáticamente.

### Backends y placeholders
| Backend  | Placeholder | Quote ident |
|----------|-------------|-------------|
| Postgres | `$1, $2…`   | `"col"`     |
| MySQL    | `?`         | `` `col` `` |
| SQLite   | `?`         | `"col"`     |

### Errores (`QueryError`)
- `InvalidIdentifier` — ident con caracteres fuera de `[A-Za-z0-9_.]`
- `InvalidOperator` — operador desconocido
- `EmptyInList` — `where_in(..., [])`
- `EmptyRecord` — INSERT sin columnas
- `MissingWhere` — UPDATE/DELETE sin WHERE (guard anti-foot-gun)
- `BindMismatch` — `?` y `params` desalineados en `where_raw`
- `Driver(String)` — error del driver async (feature `runtime-*`)

---

## SELECT

```rust
let (sql, params) = db.select("users")
    .columns(vec!["id", "name AS nombre", "COUNT(*) AS n"])
    .distinct()
    .left_join("orders", "users.id = orders.user_id")
    .where_eq("status", "active")
    .where_op("age", ">", 18)        // operador como string
    .where_op("age", "[>=]", 18)     // formato Medoo equivalente
    .where_in("role", vec!["admin", "owner"])
    .where_between("created_at", "2026-01-01", "2026-12-31")
    .where_not_null("email")
    .or_where(vec![
        Cond::eq("flag", true),
        Cond::eq("vip",  true),
    ])
    .group_by("status")
    .having(Cond::op("status", "<>", "draft")?)
    .order_desc("created_at")
    .limit(20)
    .offset(40)
    .to_sql()?;
```

**Operadores**: `=` (`==`) · `<>` (`!`, `!=`) · `>` `<` `>=` `<=` ·
`~`/`LIKE` · `!~`/`NOT LIKE` · `~*`/`ILIKE`. Formatos `>=` y `[>=]`
ambos válidos. Case-insensitive en palabras.

**NULL automático**: `.where_eq("col", None::<&str>)` → `col IS NULL`.
Solo `=` y `<>` aceptan NULL; otros operadores → `InvalidOperator`.

**Atajos LIKE**: `.where_starts_with`, `.where_ends_with`,
`.where_contains`, `.where_ilike` (auto-escapan `%` y `_`).

**BETWEEN**: `.where_between("c", lo, hi)` (valores) ·
`.where_between_cols("c", "lo_col", "hi_col")` (columnas) ·
`.where_value_in_range(v, "lo", "hi")` (valor contra rango).

**Subqueries**: `.where_in_subquery`, `.where_not_in_subquery`,
`.where_exists`, `.where_not_exists`, `.where_scalar`.

**CTE**: `.with("name", sub)` · `.with_recursive_flag()`.

**JOINs**: `inner_join` · `left_join` · `right_join` · `cross_join`.
LATERAL (PG / MySQL 8+): `inner_join_lateral` · `left_join_lateral` ·
`cross_join_lateral`.

**Raw fragments**:
```rust
db.select("logs")
  .where_eq("level", "error")
  .where_raw("created_at BETWEEN ? AND ?", vec![
      Value::Text("2026-01-01".into()),
      Value::Text("2026-12-31".into()),
  ]);
```
Los `?` se traducen al placeholder del backend.

**Non-panic**: `try_where_op` / `try_where_eq` para input dinámico
(retornan `Result` en vez de paniquear con operador inválido).

---

## INSERT / UPSERT / RETURNING

```rust
db.insert("users")
  .set(record!{ "name" => "ana", "age" => 30 })
  .set(record!{ "name" => "luis", "age" => 25 })   // multi-row
  .to_sql()?;

// UPSERT: ON CONFLICT (PG/SQLite) / ON DUPLICATE KEY UPDATE (MySQL)
db.insert("users")
  .set(record!{ "email" => "x@y", "name" => "Ana" })
  .on_conflict(vec!["email"])
  .do_update(vec!["name"]);            // o .do_nothing()

// RETURNING: PG, SQLite 3.35+, MariaDB 10.5+ (no MySQL clásico)
db.insert("users").set(record!{...}).returning(vec!["id", "created_at"]);
db.insert("users").set(record!{...}).returning(vec!["*"]);
```

---

## UPDATE / DELETE

Por defecto sin WHERE → `QueryError::MissingWhere`. Opt-in con
`.allow_full_table()`.

```rust
db.update("users")
  .set("name", "ana")
  .set("age", 31)
  .where_eq("id", 7)
  .returning(vec!["id", "updated_at"])    // PG / SQLite 3.35+ / MariaDB
  .to_sql()?;

db.delete("sessions")
  .where_op("expires_at", "<", "2026-01-01")
  .to_sql()?;
```

---

## Macros

```rust
record!{ "name" => "ana", "age" => 30, "active" => true }

where_!{
    "status"     => "active",      // col = val
    "age"        => [">", 18],     // col <op> val
    "deleted_at" => null,          // IS NULL
    "email"      => not_null,      // IS NOT NULL
}
```

---

## JSON

Path validado: keys `[A-Za-z_][A-Za-z0-9_]*`, índices `[N]`.

```rust
db.select("users")
  .where_json("meta", "$.role",  "=",  "admin")
  .where_json("meta", "$.score", ">=", 10)
  .where_json("meta", "$.deleted_at", "=", None::<&str>);  // IS NULL

let needle = Value::json(r#"{"role":"admin"}"#);
db.select("users").where_json_contains("meta", needle);
```

Render por backend:
- **MySQL**: `JSON_UNQUOTE(JSON_EXTRACT(...))` · `JSON_CONTAINS(...)`
- **Postgres**: `"col" #>> '{path}'` · `"col" @> $n::jsonb`
- **SQLite**: `json_extract(...)` · contains → `InvalidOperator`

---

## DDL

```rust
use medoo_rs::{ColDef, ColType};

db.create_table("users")
  .if_not_exists()
  .col(ColDef::new("id", ColType::BigInt).primary_key().auto_increment())
  .col(ColDef::new("email", ColType::Varchar(255)).not_null().unique())
  .col(ColDef::new("active", ColType::Bool).not_null().default_raw("TRUE"))
  .col(ColDef::new("meta", ColType::Json))
  .col(ColDef::new("created_at", ColType::Timestamp).not_null().default_raw("now()"))
  .to_sql()?;

db.drop_table("users").if_exists().cascade().to_sql()?;

let sqls: Vec<String> = db.alter_table("users")
    .add_column(ColDef::new("nickname", ColType::Text))
    .drop_column("legacy")
    .rename_column("name", "full_name")
    .rename_table("app_users")
    .to_sql()?;
```

`ColType` cubre enteros (`TinyInt`..`BigInt`), `Decimal(p,s)`, `Bool`,
flotantes, textos (`Text` / `TinyText` / `MediumText` / `LongText` /
`Char(n)` / `Varchar(n)`), `Uuid`, binarios (`Bytes` / `Binary(n)` /
`VarBinary(n)` / blobs), `Json`, fechas (`Timestamp` con TZ / `DateTime`
naive / `Date` / `Time` / `TimeTz` / `Year`), `Enum/Set`, `Raw("...")`.
Cada uno se traduce al tipo nativo del backend — tabla completa en
[MANUAL.md](MANUAL.md#coltype--sql-nativo-por-backend).

**Charset / Collation** (por columna o tabla, MySQL principal):
```rust
ColDef::new("nombre", ColType::Varchar(100))
    .charset("utf8mb4")
    .collation("utf8mb4_unicode_ci");

db.create_table("t")
  .col(...)
  .engine("InnoDB")
  .default_charset("utf8mb4")
  .default_collation("utf8mb4_unicode_ci");
```

**CREATE / DROP DATABASE**, **vistas** (incl. MATERIALIZED en PG),
**triggers** (MySQL inline / SQLite con WHEN / PG con FUNCTION) y
**events** (MySQL/MariaDB) — ver [MANUAL.md](MANUAL.md#vistas).

---

## Migraciones

Modelo + planificador. La ejecución la hace tu runtime async o el
pool (`runtime-*`).

```rust
use medoo_rs::{tracking_table_sql, Backend, Migration, Migrator};

let migrator = Migrator::new()
    .add(Migration::new(20260101, "create_users")
        .up("CREATE TABLE users (id BIGSERIAL PRIMARY KEY, email TEXT NOT NULL)")
        .down("DROP TABLE users"))
    .add(Migration::new(20260201, "users_email_idx")
        .up("CREATE UNIQUE INDEX users_email_idx ON users(email)")
        .down("DROP INDEX users_email_idx"));

let create = tracking_table_sql(Backend::Postgres)?;
let pending  = migrator.pending(&applied_versions)?;
let rollback = migrator.rollback_plan(&applied, Some(20260101))?;
```

Detecta duplicados, ordena ascendente, soporta rollback hasta versión
target.

---

## Logging

Sink configurable + filtro por categoría (bitflags).

```rust
use medoo_rs::{Logger, LogCategory};

Logger::stdout()
Logger::stderr()
Logger::file("app.log")?
Logger::buffer()    // -> (Logger, Arc<Mutex<Vec<u8>>>) para tests

LogCategory::READ | LogCategory::WRITE        // INSERT|UPDATE|DELETE + READ
LogCategory::DDL  | LogCategory::RAW
LogCategory::ALL | LogCategory::NONE

let db = Db::new(Backend::Postgres)
    .with_logger(Logger::file("audit.log")?.filter(LogCategory::WRITE));

db.build(&q)?;              // SELECT/INSERT/UPDATE/DELETE: auto
db.log_ddl(&ddl_sql);       // DDL: manual
db.log_raw(sql, &params);   // SQL crudo
```

Formato: `[<unix_secs>s] [<CATEGORY>] <SQL> -- params: <Vec<Value>>`.

`q.to_sql()` es puro (no loguea). `db.build(&q)` loguea como `READ`/`INSERT`/`UPDATE`/`DELETE` automáticamente.

---

## Pool async — referencia completa

Activá una `runtime-*` y obtenés un `Pool` unificado sobre `sqlx`.

### Conexión

```rust
use medoo_rs::runtime::{Pool, PoolOptions, RowExt};

// Defaults razonables
let pool = Pool::connect_mysql("mysql://root@127.0.0.1/db").await?;
let pool = Pool::connect_postgres("postgres://u:p@host/db").await?;
let pool = Pool::connect_sqlite("sqlite::memory:").await?;

// Configurado
use std::time::Duration;
let opts = PoolOptions {
    max_connections: 20,
    min_connections: 2,
    acquire_timeout: Duration::from_secs(30),
    idle_timeout:    Some(Duration::from_secs(600)),
    max_lifetime:    Some(Duration::from_secs(1800)),
};
let pool = Pool::connect_postgres_with(url, opts).await?;

// Con retry inicial (boot resiliente)
let pool = Pool::connect_mysql_retry(url, 5).await?;

// Logger sobre el pool
let pool = pool.with_logger(Logger::file("q.log")?.filter(LogCategory::WRITE));
```

### Ejecución

```rust
// Builders re-expuestos en el Pool
let q = pool.select("users").where_eq("id", 1);

let n        = pool.execute(&q_insert).await?;          // -> u64 (rows affected)
let rows     = pool.fetch_all(&q).await?;               // Vec<HashMap<String, Value>>
let row      = pool.fetch_one(&q).await?;               // o Driver("0 filas")
let opt      = pool.fetch_optional(&q).await?;

// SQL crudo
pool.execute_raw("VACUUM", vec![]).await?;
pool.fetch_all_raw("SELECT * FROM x WHERE y=?", vec![Value::Int(1)]).await?;

// Accesores tipados sobre Row
let nombre: Option<&str> = row.get_str("name");
let edad:   Option<i64>  = row.get_i64("age");
let activo: Option<bool> = row.get_bool("active");
```

### Transacciones

```rust
// Manual
let mut tx = pool.begin().await?;
tx.execute(&pool.insert("u").set(record!{ "n" => "ana" })).await?;
tx.commit().await?;          // o tx.rollback().await?
// drop sin commit -> rollback automático

// Closure (commit si Ok, rollback si Err)
pool.transaction(|tx| Box::pin(async move {
    tx.execute(&q1).await?;
    tx.execute(&q2).await?;
    Ok::<_, QueryError>(())
})).await?;

// Savepoints
tx.savepoint("sp1").await?;
tx.execute(&q).await?;
tx.rollback_to_savepoint("sp1").await?;    // o release_savepoint
```

### Bulk

```rust
pool.execute_many(&queries).await?;     // atómico en transacción
pool.execute_batch(&queries).await?;    // Vec<u64>, sin tx (más rápido)
pool.execute_batch_raw(&[(sql, params)]).await?;
```

### Streaming (datasets grandes)

```rust
pool.for_each_row(&q, |row| {
    println!("{:?}", row.get_str("name"));
    Ok(())
}).await?;

use futures::StreamExt;
let s = pool.fetch_stream(&q)?;
let primeros: Vec<_> = s.take(10).collect().await;

// Tipado con FromRow
let s = pool.fetch_stream_as::<User, _>(&q)?;
```

### Health + EXPLAIN

```rust
pool.ping().await?;                          // SELECT 1
let plan = pool.explain(&q).await?;
let plan = pool.explain_analyze(&q).await?;  // PG: ANALYZE · SQLite: QUERY PLAN
```

### Retry con backoff exponencial

```rust
pool.execute_retry(&q, 3).await?;
pool.fetch_all_retry(&q, 5).await?;
pool.execute_raw_retry(sql, params, 3).await?;
```

Reintenta solo transitorios (conexión caída, deadlock, serialización,
timeout). Backoff: 50ms, 100, 200, 400... cap 5s. Errores de
SQL/constraint salen al toque.

---

## FromRow (mapeo a struct)

**Manual** — sin features extra:

```rust
use medoo_rs::runtime::{FromRow, Row, RowExt};

struct User { id: i64, name: String }
impl FromRow for User {
    fn from_row(r: &Row) -> medoo_rs::Result<Self> {
        Ok(Self {
            id: r.get_i64("id").ok_or_else(|| /* ... */)?,
            name: r.get_str("name").unwrap_or_default().to_string(),
        })
    }
}

let xs:  Vec<User>    = pool.fetch_all_as(&q).await?;
let one: User         = pool.fetch_one_as(&q).await?;
let opt: Option<User> = pool.fetch_optional_as(&q).await?;
```

**Derive** — feature `derive`:

```toml
medoo_rs = { ..., features = ["runtime-postgres", "derive"] }
```

```rust
use medoo_rs::FromRow;

#[derive(FromRow)]
struct User {
    id: i64,
    name: String,
    email: Option<String>,                  // nullable -> Option
    #[medoo(rename = "user_active")]        // alias de columna
    active: bool,
}
```

Tipos soportados: `i8..i64`, `u8..u64`, `f32`, `f64`, `bool`, `String`,
`Vec<u8>`, `Option<T>` de cualquiera.

---

## chrono (fechas tipadas)

Feature `chrono` — agrega `IntoValue` para tipos chrono e accesores
tipados sobre `Row`.

```toml
medoo_rs = { ..., features = ["runtime-postgres", "chrono"] }
```

```rust
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use medoo_rs::runtime::RowExtChrono;

// Insert: cualquier tipo chrono va por IntoValue (ISO 8601)
db.insert("logs").set(record!{ "ts" => Utc::now() });

// Read: accesores tipados
let dt:    Option<DateTime<Utc>> = row.get_datetime_utc("ts");
let nd:    Option<NaiveDateTime> = row.get_naive_datetime("agendado");
let fecha: Option<NaiveDate>     = row.get_date("fecha");
let hora:  Option<NaiveTime>     = row.get_time("hora");
```

Sin la feature, el pool decodifica fechas como `Value::Text` (ISO 8601)
para no atar la lib a un crate de tiempo concreto.

---

## Seguridad

| Punto de entrada     | Defensa                                                |
|----------------------|--------------------------------------------------------|
| Tabla / columna      | Whitelist `[A-Za-z_][A-Za-z0-9_]*`, ≤64 chars          |
| Operador (string)    | Parser cerrado, error explícito                        |
| Valor de usuario     | **Siempre** placeholder, jamás inline                  |
| `JOIN ... ON`        | Solo `ident = ident` (con `tabla.col`)                 |
| Path JSON            | Whitelist + índices numéricos, segmentos vacíos no     |
| `default_raw`        | Rechaza `;` y `--`                                     |
| Columnas con `(...)` | Aceptadas como expr, rechazan `;` y `--`               |
| `where_raw`          | Cuenta `?` vs `params.len()` → `BindMismatch`          |

Tests de inyección por cada punto en [`tests/security.rs`](tests/security.rs).

---

## Tests

```bash
cargo test                                  # núcleo (sin features)
cargo test --features runtime-sqlite        # + pool sqlx sqlite
cargo test --all-features                   # todo
```

**209 tests** verde cubriendo builder, JSON, DDL, vistas, triggers,
events, migraciones, logging, security, pool (sqlite real), derive y
chrono.

---

## Licencia

Licenciado bajo [Apache License, Version 2.0](LICENSE).

Las contribuciones aportadas intencionalmente para inclusión en este proyecto, según se define en la licencia Apache-2.0, serán licenciadas bajo los mismos términos, sin ningún término o condición adicional.
