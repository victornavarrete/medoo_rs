# medoo_rs — Cheat sheet

Referencia rápida. Para texto explicativo ver [README.md](README.md).

## Setup

```rust
use medoo_rs::*;
let db = Db::new(Backend::Postgres);                         // sin logger
let db = Db::new(Backend::Postgres).with_logger(Logger::stdout());
```

Backends: `Postgres` ($N) · `MySql` (?) · `Sqlite` (?)

## SELECT

| Método | SQL |
|--|--|
| `.columns(vec!["id", "name AS n", "COUNT(*)"])` | `SELECT "id", "name" AS "n", COUNT(*)` |
| `.distinct()` | `SELECT DISTINCT` |
| `.where_eq("c", v)` | `c = ?` (o `IS NULL` si v es `None`) |
| `.where_op("c", ">", v)` / `"[>=]"` / `"~"` / `"!~"` / `"<>"` / `"=="` / `"ILIKE"` / `"~*"` | `c > ?` etc. |
| `.try_where_op(c, op, v)` / `.try_where_eq` | versiones non-panic, retornan `Result` |
| `.where_in("c", vec)` | `c IN (?, ?, ?)` |
| `.where_between("c", lo, hi)` | `c BETWEEN ? AND ?` (valores) |
| `.where_between_cols("c", "lo_col", "hi_col")` | `c BETWEEN lo_col AND hi_col` (columnas) |
| `.where_between_cols_with(c, l, h, inc_lo, inc_hi)` | con flags de inclusividad |
| `.where_value_in_range(v, "lo", "hi")` | `lo <= v AND v <= hi` |
| `.where_null("c")` / `.where_not_null("c")` | `c IS NULL` / `IS NOT NULL` |
| `.where_like("c", "%foo%")` / `.where_not_like` | `c LIKE ?` |
| `.where_starts_with("c", "ana")` / `.where_ends_with` / `.where_contains` | `c LIKE 'ana%'` (auto-escape `%`/`_`) |
| `.where_ilike("c", "ana%")` | PG: `ILIKE`. Otros: `LOWER(c) LIKE LOWER(?)` |
| `.where_raw("a > ? AND b < ?", vec![..])` | re-bindea `?` al placeholder del backend |
| `.where_cond(Cond::Or(vec![..]))` | grupo `(.. OR ..)` |
| `.or_where(vec![Cond::eq..., Cond::eq...])` | grupo OR ergonómico |
| `.where_json("col", "$.path", "=", v)` | extracción JSON + comparación |
| `.where_json_contains("col", Value::json("..."))` | JSON_CONTAINS / `@>` |
| `.where_in_subquery("col", sub)` / `.where_not_in_subquery` | `col IN (SELECT ...)` con merge de placeholders |
| `.where_exists(sub)` / `.where_not_exists(sub)` | `EXISTS (SELECT ...)` |
| `.where_scalar("col", ">", sub)` | `col > (SELECT ...)` escalar |
| `.with("name", sub)` / `.with_recursive_flag()` | CTE: `WITH name AS (...) SELECT ...` |
| `.inner_join("t", "a.id = b.a_id")` / `.left_join` / `.right_join` / `.cross_join("t")` | JOIN clásico |
| `.inner_join_lateral(sub, "alias", "on")` / `.left_join_lateral` / `.cross_join_lateral` | LATERAL (PG / MySQL 8+) |
| `.group_by("c")` · `.having(Cond::op(..))` | |
| `.order_asc("c")` / `.order_desc("c")` / `.order_by("c", OrderDir::Asc)` | |
| `.limit(n)` / `.offset(n)` | |
| `.to_sql()` → `Result<(String, Vec<Value>)>` | puro, no loguea |
| `db.build(&q)` → ídem y loguea como `READ` | |

### Operadores `where_op`

`=` (`==`) · `<>` (`!`, `!=`) · `>` `<` `>=` `<=` · `~`/`LIKE` · `!~`/`NOT LIKE` · `~*`/`ILIKE`.
Aceptan formato Medoo `[op]`: `[>=]`, `[~]`, `[<>]`. Case-insensitive
para palabras (`like`, `LIKE`, `Like`).

**NULL guard**: solo `=`/`<>` aceptan `Value::Null` (emiten `IS [NOT] NULL`).
Otros operadores contra NULL → `InvalidOperator` (evita bug silencioso).

**Non-panic**: `where_op` panicea con operador inválido. Para input
dinámico usá `try_where_op(col, op, v) -> Result<Self>`.

## INSERT

```rust
db.insert("users").set(record!{ "name" => "ana", "age" => 30 });
db.insert("users")
  .set(record!{ "name" => "ana", "age" => 30 })
  .set(record!{ "name" => "luis", "age" => 25 });   // multi-row

// UPSERT: ON CONFLICT (PG/SQLite) / ON DUPLICATE KEY UPDATE (MySQL)
db.insert("users")
  .set(record!{ "email" => "x@y", "name" => "Ana" })
  .on_conflict(vec!["email"])         // ignorado en MySQL
  .do_update(vec!["name"]);            // o .do_nothing()

// RETURNING: PG, SQLite 3.35+, MariaDB 10.5+ (no MySQL)
db.insert("users").set(record!{...}).returning(vec!["id", "created_at"]);
db.insert("users").set(record!{...}).returning(vec!["*"]);
```

## UPDATE / DELETE

```rust
db.update("users").set("name", "ana").where_eq("id", 7);
db.delete("sessions").where_op("expires_at", "<", "2026-01-01");

// RETURNING también en UPDATE/DELETE (mismo soporte que INSERT)
db.update("u").set("active", false).where_eq("id", 1).returning(vec!["id", "updated_at"]);
db.delete("logs").where_op("ts", "<", "2026-01-01").returning(vec!["*"]);
```

Sin WHERE → `QueryError::MissingWhere`. Opt-in con `.allow_full_table()`.

## Macros

```rust
record!{ "name" => "ana", "age" => 30, "active" => true }
where_!{
    "status" => "active",      // col = val
    "age"    => [">", 18],     // col <op> val
    "deleted_at" => null,      // IS NULL
    "email"      => not_null,  // IS NOT NULL
}
```

## JSON

```rust
// Path: "$.a.b[0]" o "a.b[0]" (validado: solo [A-Za-z0-9_] + índices)
db.select("u").where_json("meta", "$.role", "=", "admin");
db.select("u").where_json_contains("meta", Value::json(r#"{"x":1}"#));

// Render:
// MySQL:    JSON_UNQUOTE(JSON_EXTRACT(`meta`, '$.role')) = ?
//           JSON_CONTAINS(`meta`, ?)
// Postgres: "meta" #>> '{role}' = $1
//           "meta" @> $1::jsonb
// SQLite:   json_extract("meta", '$.role') = ?   (contains: error)
```

`Value::json("...")` para guardar JSON crudo. La DB valida sintaxis.

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
  .primary_key(vec!["a", "b"])    // alternativo: PK compuesta
  .to_sql()?;

db.drop_table("t").if_exists().cascade().to_sql()?;

db.alter_table("t")
  .add_column(ColDef::new("x", ColType::Text))
  .drop_column("y")
  .rename_column("a", "b")
  .rename_table("t2")
  .to_sql()?;   // Vec<String> (una sentencia por acción)
```

### `ColType` → SQL nativo por backend

| Variante           | Postgres         | MySQL              | SQLite      |
|--------------------|------------------|--------------------|-------------|
| `TinyInt`          | SMALLINT         | TINYINT            | INTEGER     |
| `SmallInt`         | SMALLINT         | SMALLINT           | INTEGER     |
| `Int`              | INTEGER          | INTEGER            | INTEGER     |
| `BigInt`           | BIGINT           | BIGINT             | INTEGER     |
| `Decimal(p, s)`    | DECIMAL(p,s)     | DECIMAL(p,s)       | DECIMAL(p,s)|
| `Bool`             | BOOLEAN          | TINYINT(1)         | INTEGER     |
| `Float`            | REAL             | REAL               | REAL        |
| `Double`           | DOUBLE PRECISION | DOUBLE             | DOUBLE      |
| `Text`             | TEXT             | TEXT               | TEXT        |
| `TinyText`         | TEXT             | TINYTEXT           | TEXT        |
| `MediumText`       | TEXT             | MEDIUMTEXT         | TEXT        |
| `LongText`         | TEXT             | LONGTEXT           | TEXT        |
| `Char(n)`          | CHAR(n)          | CHAR(n)            | CHAR(n)     |
| `Varchar(n)`       | VARCHAR(n)       | VARCHAR(n)         | VARCHAR(n)  |
| `Uuid`             | UUID             | CHAR(36)           | CHAR(36)    |
| `Bytes`            | BYTEA            | BLOB               | BLOB        |
| `Binary(n)`        | BYTEA            | BINARY(n)          | BLOB        |
| `VarBinary(n)`     | BYTEA            | VARBINARY(n)       | BLOB        |
| `TinyBlob`         | BYTEA            | TINYBLOB           | BLOB        |
| `MediumBlob`       | BYTEA            | MEDIUMBLOB         | BLOB        |
| `LongBlob`         | BYTEA            | LONGBLOB           | BLOB        |
| `Json`             | JSONB            | JSON               | TEXT        |
| `Timestamp` *(TZ)* | TIMESTAMPTZ      | TIMESTAMP          | TEXT        |
| `DateTime` *(naive)*| TIMESTAMP       | DATETIME           | TEXT        |
| `Date`             | DATE             | DATE               | DATE        |
| `Time`             | TIME             | TIME               | TEXT        |
| `TimeTz`           | TIMETZ           | TEXT               | TEXT        |
| `Year`             | SMALLINT         | YEAR               | INTEGER     |
| `Enum(vec![...])`  | TEXT             | ENUM('a','b','c')  | TEXT        |
| `Set(vec![...])`   | TEXT             | SET('a','b','c')   | TEXT        |
| `Raw("...")`       | (literal)        | (literal)          | (literal)   |

**Notas finas:**
- **Timezone**:
  - `Timestamp` = "instante con TZ" → `TIMESTAMPTZ` (PG) / `TIMESTAMP`
    (MySQL: guarda en UTC, convierte a session TZ al leer) / `TEXT` (SQLite).
  - `DateTime` = "wall-clock naive" → `TIMESTAMP` (PG sin TZ) / `DATETIME`
    (MySQL crudo) / `TEXT` (SQLite).
  - Si dudás cuál usar: **transacciones globales / logs / instantes
    absolutos → `Timestamp`. Calendario / agenda local que no debe
    rotar al cambiar de zona → `DateTime`.**
  - El pool decodifica fechas/horas siempre como `Value::Text` (ISO 8601
    con offset cuando aplica) — para no atar la lib a `chrono`/`time`.
- `Decimal(p,s)` se decodifica desde el pool como `Value::Text` para
  no perder precisión (un `99.99` en `f64` no es exacto).
- `Uuid` en MySQL/SQLite va como `CHAR(36)` (string `xxxx-xxxx-...`).
  En Postgres usa el tipo nativo.
- `TinyInt`/`Year` en PG caen a `SMALLINT`; los pongo aparte porque
  semánticamente comunican otra cosa al lector del código.
- Tipos no cubiertos hoy (usar `Raw("...")`): `INTERVAL` (PG),
  `BIT(n)`, arrays PG (`text[]`), `INET`/`CIDR`/`MACADDR`, geometría
  (PostGIS), `MONEY`. Si los necesitas seguido, los promovemos a
  variante propia.

`auto_increment + Postgres` → `BIGSERIAL` o `SERIAL` automático.
`auto_increment + SQLite + primary_key` → `INTEGER PRIMARY KEY AUTOINCREMENT`.
`auto_increment + MySQL` → `AUTO_INCREMENT`.

### CREATE / DROP DATABASE

```rust
db.create_database("terapias_x")
  .if_not_exists()
  .default_charset("utf8mb4")
  .default_collation("utf8mb4_unicode_ci")
  .to_sql()?;
// MySQL:    CREATE DATABASE IF NOT EXISTS `terapias_x` DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
// Postgres: CREATE DATABASE "terapias_x" ENCODING 'utf8mb4' LC_COLLATE 'utf8mb4_unicode_ci'
// SQLite:   error (un archivo = una BD)

db.drop_database("midb").if_exists().to_sql()?;
```

### ALTER TABLE: charset / collation / engine (MySQL)

```rust
// Convertir datos existentes a otra charset:
db.alter_table("users")
  .convert_to_charset_collate("utf8mb4", "utf8mb4_unicode_ci")
  .to_sql()?;
// → ALTER TABLE `users` CONVERT TO CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci

// Solo cambiar el default (sin tocar datos):
db.alter_table("users")
  .set_default_charset_collate("utf8mb4", "utf8mb4_unicode_ci")
  .set_engine("InnoDB")
  .to_sql()?;  // → 2 statements
```

En Postgres/SQLite estas acciones se omiten silenciosamente
(no tienen equivalente directo).

### Charset / Collation

```rust
// Por columna
ColDef::new("nombre", ColType::Varchar(100))
    .charset("utf8mb4")              // MySQL: CHARACTER SET utf8mb4
    .collation("utf8mb4_unicode_ci") // MySQL: COLLATE utf8mb4_unicode_ci
                                     // PG:    COLLATE "utf8mb4_unicode_ci"
                                     // SQLite:COLLATE utf8mb4_unicode_ci

// A nivel tabla (solo MySQL — ignorado en PG/SQLite)
db.create_table("t")
  .col(...)
  .engine("InnoDB")
  .default_charset("utf8mb4")
  .default_collation("utf8mb4_unicode_ci");
// → CREATE TABLE `t` (...) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
```

Nombres validados con whitelist `[A-Za-z0-9_]` (≤64): cualquier
inyección por nombre devuelve `InvalidIdentifier`.

## Vistas

```rust
// CREATE VIEW
db.create_view("activos")
  .or_replace()                       // PG/MySQL (no SQLite)
  .as_select(db.select("u").where_eq("active", true))
  .to_sql()?;

// IF NOT EXISTS (SQLite/MariaDB)
db.create_view("v").if_not_exists().as_select(q).to_sql()?;

// Aliases de columnas: CREATE VIEW v (a, b) AS SELECT ...
db.create_view("v").columns(vec!["uid","nombre"]).as_select(q);

// MATERIALIZED VIEW (solo PG)
db.create_view("totales").materialized().as_select(q).to_sql()?;

// DROP
db.drop_view("v").if_exists().cascade().to_sql()?;
db.drop_view("m").materialized().if_exists().to_sql()?; // PG
```

## Triggers

```rust
// MySQL / SQLite: body inline
db.create_trigger("audit_users")
  .after().insert().on_table("users")
  .body("INSERT INTO log (msg) VALUES ('user added')")
  .to_sql()?;

// SQLite con WHEN
db.create_trigger("audit_high")
  .before().update().on_table("orders")
  .when("NEW.amount > 1000")
  .body("INSERT INTO alerts VALUES (NEW.id)")
  .to_sql()?;

// Postgres: ejecuta una FUNCTION pre-existente
db.create_trigger("trg")
  .after().delete().on_table("users")
  .execute_function("audit_users_fn")     // → EXECUTE FUNCTION audit_users_fn()
  .to_sql()?;

// DROP
db.drop_trigger("t").if_exists().to_sql()?;          // MySQL/SQLite
db.drop_trigger("t").if_exists().on_table("u")?;     // PG requiere ON
```

Métodos: `.before()/.after()/.instead_of()` × `.insert()/.update()/.delete()`.

## Events (solo MySQL/MariaDB)

```rust
db.create_event("daily_cleanup")
  .if_not_exists()
  .schedule("EVERY 1 DAY")
  .body("DELETE FROM tmp WHERE created_at < NOW() - INTERVAL 30 DAY")
  .to_sql()?;

db.drop_event("daily_cleanup").if_exists().to_sql()?;
```

PG/SQLite → `InvalidOperator` ("events solo en MySQL/MariaDB"). Para PG
usá `pg_cron` o un scheduler externo.

## Migraciones

```rust
let migrator = Migrator::new()
    .add(Migration::new(20260101, "create_users")
        .up("CREATE TABLE users (...)")
        .down("DROP TABLE users"));

let create_tracking = tracking_table_sql(Backend::Postgres)?;
let pending = migrator.pending(&applied_versions)?;          // asc
let rollback = migrator.rollback_plan(&applied, Some(t))?;   // desc
```

`Migrator::ordered()` falla con `InvalidIdentifier` si hay
versiones duplicadas.

## Logging

```rust
// Sinks:
Logger::stdout()
Logger::stderr()
Logger::file("app.log")?
Logger::buffer()  // -> (Logger, Arc<Mutex<Vec<u8>>>) para tests

// Filtros (combinables con `|`):
LogCategory::READ | LogCategory::WRITE         // INSERT|UPDATE|DELETE+READ
LogCategory::DDL | LogCategory::RAW
LogCategory::ALL  // default
LogCategory::NONE

// Aplicación:
let db = Db::new(Backend::Postgres)
    .with_logger(Logger::file("audit.log")?.filter(LogCategory::WRITE));

db.build(&q)?;            // SELECT/INSERT/UPDATE/DELETE: log automático
db.log_ddl(&ddl_sql);     // DDL: manual (no pasa por trait Query)
db.log_raw(sql, &params); // SQL crudo
```

Formato: `[<unix_secs>s] [<CATEGORY>] <SQL> -- params: <Vec<Value>>`

## Errores

```rust
QueryError::InvalidIdentifier(String)          // ident con caracteres prohibidos
QueryError::InvalidOperator(String)            // operador desconocido
QueryError::EmptyInList(String)                // where_in con vec vacío
QueryError::EmptyRecord                        // INSERT/CREATE TABLE sin cols
QueryError::MissingWhere(&'static str)         // UPDATE/DELETE sin WHERE
QueryError::UnresolvedTemplate(String)         // (reservado para Fase 6 futuro)
QueryError::BindMismatch { expected, got }     // ? y params desalineados
```

## Pool async (feature `runtime-*`)

Features opcionales — sin activar nada el núcleo es zero-deps:

| Feature              | Activa                                                |
|----------------------|-------------------------------------------------------|
| `runtime-mysql`      | Pool sqlx MySQL/MariaDB                               |
| `runtime-postgres`   | Pool sqlx Postgres                                    |
| `runtime-sqlite`     | Pool sqlx SQLite                                      |
| `derive`             | `#[derive(FromRow)]` (proc-macro)                     |
| `chrono`             | `IntoValue` y `RowExtChrono` para tipos chrono        |

```toml
medoo_rs = { path = "...", features = ["runtime-mysql", "derive", "chrono"] }
```

Necesita `tokio` en tu app cuando activás cualquier `runtime-*`.

```rust
use medoo_rs::runtime::{Pool, RowExt};

let pool = Pool::connect_mysql("mysql://root@127.0.0.1/terapias").await?;
// (también: connect_sqlite("sqlite::memory:") / connect_postgres(url))

// builders re-expuestos en el Pool:
let q = pool.select("users").where_eq("id", 1);

// ejecución:
let n = pool.execute(&pool.insert("u").set(record!{ "name" => "Ana" })).await?;
let rows = pool.fetch_all(&q).await?;     // Vec<HashMap<String, Value>>
let row  = pool.fetch_one(&q).await?;     // o `Driver("0 filas")`
let opt  = pool.fetch_optional(&q).await?;

// SQL crudo:
pool.execute_raw("VACUUM", vec![]).await?;
pool.fetch_all_raw("SELECT * FROM x WHERE y=?", vec![Value::Int(1)]).await?;

// accesores tipados sobre la fila:
let nombre: Option<&str> = row.get_str("name");
let edad:   Option<i64>  = row.get_i64("age");
let activo: Option<bool> = row.get_bool("active");

// logger se adjunta al pool:
let pool = Pool::connect_sqlite("sqlite::memory:").await?
    .with_logger(Logger::file("queries.log")?.filter(LogCategory::WRITE));
```

Errores del driver llegan envueltos: `QueryError::Driver(String)`.

### Transacciones

```rust
// Manual
let mut tx = pool.begin().await?;
tx.execute(&pool.insert("u").set(record!{ "n" => "ana" })).await?;
tx.commit().await?;     // o tx.rollback().await?;
// drop sin commit -> rollback automático

// Closure: commit si Ok, rollback si Err
pool.transaction(|tx| Box::pin(async move {
    tx.execute(...).await?;
    tx.execute(...).await?;
    Ok::<_, QueryError>(())
})).await?;

// Savepoints
tx.savepoint("sp1").await?;
tx.execute(...).await?;
tx.rollback_to_savepoint("sp1").await?;     // o release_savepoint
```

### PoolOptions (max_connections, timeouts)

```rust
let opts = PoolOptions {
    max_connections: 20,
    min_connections: 2,
    acquire_timeout: Duration::from_secs(30),
    idle_timeout: Some(Duration::from_secs(600)),
    max_lifetime: Some(Duration::from_secs(1800)),
};
let pool = Pool::connect_mysql_with(url, opts).await?;

// Con retry de conexión inicial:
let pool = Pool::connect_mysql_retry(url, 5).await?;
```

### Bulk

```rust
// En transacción (atómico): rollback si una falla
pool.execute_many(&queries).await?;

// Sin transacción (una conexión, sin atomicidad): más rápido
pool.execute_batch(&queries).await?;       // Vec<u64>
pool.execute_batch_raw(&[(sql, params)]).await?;
```

### Streaming (datasets grandes)

```rust
// Callback (no carga todo a memoria)
pool.for_each_row(&q, |row| {
    println!("{:?}", row.get_str("name"));
    Ok(())
}).await?;

// Stream real (combinable con .take/.filter/.map)
use futures::StreamExt;
let s = pool.fetch_stream(&q)?;
let primeros: Vec<_> = s.take(10).collect().await;

// Tipado con FromRow
let s = pool.fetch_stream_as::<User, _>(&q)?;
```

### Health check + EXPLAIN

```rust
pool.ping().await?;                                    // SELECT 1
let plan = pool.explain(&q).await?;                    // EXPLAIN
let plan = pool.explain_analyze(&q).await?;            // EXPLAIN ANALYZE / QUERY PLAN
```

### FromRow: mapear filas a struct

**Manual** (sin features extra):
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
let xs: Vec<User> = pool.fetch_all_as(&q).await?;
let one: User    = pool.fetch_one_as(&q).await?;
let opt: Option<User> = pool.fetch_optional_as(&q).await?;
```

**Derive** (feature `derive`):
```toml
medoo_rs = { ..., features = ["runtime-mysql", "derive"] }
```
```rust
use medoo_rs::FromRow;

#[derive(FromRow)]
struct User {
    id: i64,
    name: String,
    email: Option<String>,           // nullable -> Option
    #[medoo(rename = "user_active")]  // alias de columna
    active: bool,
}
```
Tipos soportados: `i8..i64`, `u8..u64`, `f32`, `f64`, `bool`, `String`,
`Vec<u8>`, `Option<T>` para cualquiera.

### Feature `chrono`: fechas tipadas

```toml
medoo_rs = { ..., features = ["runtime-mysql", "chrono"] }
```
```rust
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use medoo_rs::runtime::RowExtChrono;

// Insert: cualquier tipo chrono va por IntoValue (formato ISO 8601)
db.insert("logs").set(record!{ "ts" => Utc::now() });

// Read: accesores chrono sobre Row
let dt:    Option<DateTime<Utc>> = row.get_datetime_utc("ts");
let nd:    Option<NaiveDateTime> = row.get_naive_datetime("agendado");
let fecha: Option<NaiveDate>     = row.get_date("fecha");
let hora:  Option<NaiveTime>     = row.get_time("hora");
```

### Retry con backoff exponencial

```rust
pool.execute_retry(&q, 3).await?;        // hasta 3 intentos
pool.fetch_all_retry(&q, 5).await?;
pool.execute_raw_retry(sql, params, 3).await?;
```

Reintenta solo errores transitorios (conexión caída, deadlock,
serialización, timeout). Backoff: 50ms, 100, 200, 400... cap 5s.
Errores de SQL/constraint salen al toque sin esperar.

## Seguridad — defensas activas

- Identifiers: whitelist `[A-Za-z_][A-Za-z0-9_]*`, ≤64 chars, soporta `tabla.col`.
- Operadores: parser cerrado, error explícito.
- Valores de usuario: **siempre** placeholder.
- `JOIN ... ON`: solo `ident = ident`.
- Path JSON: whitelist + índices numéricos, segmentos vacíos rechazados.
- `default_raw`: rechaza `;` y `--`.
- Columnas con `(`: aceptadas como expresión, pero rechazan `;` y `--`.
- `where_raw`: cuenta `?` vs `params.len()` → `BindMismatch`.

## Convenciones

- Métodos chainables toman `self` por valor → `let mut q = ...; q = q...`.
- `.to_sql()` es **puro** (no loguea, no muta, no toca DB).
- `db.build(&q)` es la versión que loguea.
- Nada se ejecuta hasta que tu runtime async lo hace.
