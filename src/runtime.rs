//! Adapter async sobre sqlx. Solo se compila si activai una feature
//! `runtime-*`. El núcleo (sin feature) sigue zero-deps.
//!
//! ```ignore
//! [features]
//! medoo_rs = { path = "...", features = ["runtime-mysql"] }
//! ```
//!
//! Uso:
//! ```ignore
//! let pool = Pool::connect_mysql("mysql://root@127.0.0.1/terapias").await?;
//! let q = pool.select("users").where_eq("id", 1);
//! let rows = pool.fetch_all(&q).await?;
//! ```

#![cfg(any(
    feature = "runtime-mysql",
    feature = "runtime-postgres",
    feature = "runtime-sqlite"
))]

use crate::backend::Backend;
use crate::delete::DeleteQuery;
use crate::error::{QueryError, Result};
use crate::insert::InsertQuery;
use crate::log::{Logger, Query};
use crate::select::SelectQuery;
use crate::update::UpdateQuery;
use crate::value::Value;
use crate::Db;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// Una fila como `HashMap<columna, Value>`. No fuerza tipado: el
/// usuario hace `row.get_str("nombre")?.unwrap_or_default()` etc.
pub type Row = HashMap<String, Value>;

/// Trait para mapear `Row` a un struct propio. Implementación manual
/// (sin proc-macro). Si querés mapeo automático, lo agregaremos
/// después como feature derive.
///
/// ```ignore
/// struct User { id: i64, name: String }
/// impl FromRow for User {
///     fn from_row(r: &Row) -> Result<Self> {
///         Ok(Self {
///             id: r.get_i64("id").ok_or_else(|| QueryError::Driver("col id".into()))?,
///             name: r.get_str("name").unwrap_or_default().to_string(),
///         })
///     }
/// }
/// ```
pub trait FromRow: Sized {
    fn from_row(row: &Row) -> Result<Self>;
}

/// Opciones del pool. Defaults sanos para la mayoría de apps.
#[derive(Debug, Clone)]
pub struct PoolOptions {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Option<Duration>,
    pub max_lifetime: Option<Duration>,
}

impl Default for PoolOptions {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 0,
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Some(Duration::from_secs(600)),
            max_lifetime: Some(Duration::from_secs(1800)),
        }
    }
}

pub trait RowExt {
    fn get_i64(&self, col: &str) -> Option<i64>;
    fn get_str(&self, col: &str) -> Option<&str>;
    fn get_bool(&self, col: &str) -> Option<bool>;
    fn get_f64(&self, col: &str) -> Option<f64>;
}

impl RowExt for Row {
    fn get_i64(&self, col: &str) -> Option<i64> {
        match self.get(col)? {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }
    fn get_str(&self, col: &str) -> Option<&str> {
        match self.get(col)? {
            Value::Text(s) | Value::Json(s) => Some(s.as_str()),
            _ => None,
        }
    }
    fn get_bool(&self, col: &str) -> Option<bool> {
        match self.get(col)? {
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            _ => None,
        }
    }
    fn get_f64(&self, col: &str) -> Option<f64> {
        match self.get(col)? {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
}

/// Accesores chrono para `Row`. Disponibles solo con feature `chrono`.
#[cfg(feature = "chrono")]
pub trait RowExtChrono {
    fn get_datetime_utc(&self, col: &str) -> Option<chrono::DateTime<chrono::Utc>>;
    fn get_naive_datetime(&self, col: &str) -> Option<chrono::NaiveDateTime>;
    fn get_date(&self, col: &str) -> Option<chrono::NaiveDate>;
    fn get_time(&self, col: &str) -> Option<chrono::NaiveTime>;
}

#[cfg(feature = "chrono")]
impl RowExtChrono for Row {
    fn get_datetime_utc(&self, col: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        let s = self.get_str(col)?;
        // RFC3339 (con TZ) → directo
        if let Ok(d) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(d.with_timezone(&chrono::Utc));
        }
        // 'YYYY-MM-DD HH:MM:SS[.ffffff]' → asume UTC
        for fmt in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S"] {
            if let Ok(n) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
                return Some(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(n, chrono::Utc));
            }
        }
        None
    }
    fn get_naive_datetime(&self, col: &str) -> Option<chrono::NaiveDateTime> {
        let s = self.get_str(col)?;
        for fmt in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
            if let Ok(n) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
                return Some(n);
            }
        }
        None
    }
    fn get_date(&self, col: &str) -> Option<chrono::NaiveDate> {
        let s = self.get_str(col)?;
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
    }
    fn get_time(&self, col: &str) -> Option<chrono::NaiveTime> {
        let s = self.get_str(col)?;
        for fmt in ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"] {
            if let Ok(t) = chrono::NaiveTime::parse_from_str(s, fmt) {
                return Some(t);
            }
        }
        None
    }
}

/// Pool unificado. Detrás guarda el pool concreto del backend activo.
pub struct Pool {
    db: Db,
    inner: PoolInner,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
enum PoolInner {
    #[cfg(feature = "runtime-mysql")]
    MySql(sqlx::MySqlPool),
    #[cfg(feature = "runtime-postgres")]
    Postgres(sqlx::PgPool),
    #[cfg(feature = "runtime-sqlite")]
    Sqlite(sqlx::SqlitePool),
}

impl Pool {
    /// Conecta a MySQL. URL típica: `mysql://user:pass@host:3306/db`.
    #[cfg(feature = "runtime-mysql")]
    pub async fn connect_mysql(url: &str) -> Result<Self> {
        let pool = sqlx::MySqlPool::connect(url)
            .await
            .map_err(|e| QueryError::Driver(e.to_string()))?;
        Ok(Self {
            db: Db::new(Backend::MySql),
            inner: PoolInner::MySql(pool),
        })
    }

    /// Conecta a Postgres.
    #[cfg(feature = "runtime-postgres")]
    pub async fn connect_postgres(url: &str) -> Result<Self> {
        let pool = sqlx::PgPool::connect(url)
            .await
            .map_err(|e| QueryError::Driver(e.to_string()))?;
        Ok(Self {
            db: Db::new(Backend::Postgres),
            inner: PoolInner::Postgres(pool),
        })
    }

    /// Conecta a SQLite. URL típica: `sqlite::memory:` o `sqlite:./app.db`.
    #[cfg(feature = "runtime-sqlite")]
    pub async fn connect_sqlite(url: &str) -> Result<Self> {
        let pool = sqlx::SqlitePool::connect(url)
            .await
            .map_err(|e| QueryError::Driver(e.to_string()))?;
        Ok(Self {
            db: Db::new(Backend::Sqlite),
            inner: PoolInner::Sqlite(pool),
        })
    }

    // versiones con opciones de pool
    #[cfg(feature = "runtime-mysql")]
    pub async fn connect_mysql_with(url: &str, opts: PoolOptions) -> Result<Self> {
        let mut po = sqlx::mysql::MySqlPoolOptions::new()
            .max_connections(opts.max_connections)
            .min_connections(opts.min_connections)
            .acquire_timeout(opts.acquire_timeout);
        po = po.idle_timeout(opts.idle_timeout);
        po = po.max_lifetime(opts.max_lifetime);
        let pool = po.connect(url).await.map_err(driver)?;
        Ok(Self { db: Db::new(Backend::MySql), inner: PoolInner::MySql(pool) })
    }

    #[cfg(feature = "runtime-postgres")]
    pub async fn connect_postgres_with(url: &str, opts: PoolOptions) -> Result<Self> {
        let mut po = sqlx::postgres::PgPoolOptions::new()
            .max_connections(opts.max_connections)
            .min_connections(opts.min_connections)
            .acquire_timeout(opts.acquire_timeout);
        po = po.idle_timeout(opts.idle_timeout);
        po = po.max_lifetime(opts.max_lifetime);
        let pool = po.connect(url).await.map_err(driver)?;
        Ok(Self { db: Db::new(Backend::Postgres), inner: PoolInner::Postgres(pool) })
    }

    #[cfg(feature = "runtime-sqlite")]
    pub async fn connect_sqlite_with(url: &str, opts: PoolOptions) -> Result<Self> {
        let mut po = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(opts.max_connections)
            .min_connections(opts.min_connections)
            .acquire_timeout(opts.acquire_timeout);
        po = po.idle_timeout(opts.idle_timeout);
        po = po.max_lifetime(opts.max_lifetime);
        let pool = po.connect(url).await.map_err(driver)?;
        Ok(Self { db: Db::new(Backend::Sqlite), inner: PoolInner::Sqlite(pool) })
    }

    // retry de conexión
    #[cfg(feature = "runtime-mysql")]
    pub async fn connect_mysql_retry(url: &str, max_attempts: usize) -> Result<Self> {
        connect_with_retry(max_attempts, || Self::connect_mysql(url)).await
    }
    #[cfg(feature = "runtime-postgres")]
    pub async fn connect_postgres_retry(url: &str, max_attempts: usize) -> Result<Self> {
        connect_with_retry(max_attempts, || Self::connect_postgres(url)).await
    }
    #[cfg(feature = "runtime-sqlite")]
    pub async fn connect_sqlite_retry(url: &str, max_attempts: usize) -> Result<Self> {
        connect_with_retry(max_attempts, || Self::connect_sqlite(url)).await
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn with_logger(mut self, logger: Logger) -> Self {
        self.db = self.db.clone().with_logger(logger);
        self
    }

    // ---------- builders re-exportados desde la Db interna ----------
    pub fn select(&self, table: &str) -> SelectQuery {
        self.db.select(table)
    }
    pub fn insert(&self, table: &str) -> InsertQuery {
        self.db.insert(table)
    }
    pub fn update(&self, table: &str) -> UpdateQuery {
        self.db.update(table)
    }
    pub fn delete(&self, table: &str) -> DeleteQuery {
        self.db.delete(table)
    }

    // ---------- ejecución ----------

    /// Ejecuta una query (INSERT/UPDATE/DELETE/DDL). Retorna filas afectadas.
    pub async fn execute<Q: Query>(&self, q: &Q) -> Result<u64> {
        let (sql, params) = self.db.build(q)?;
        self.execute_raw(&sql, params).await
    }

    /// Ejecuta SQL crudo con parámetros.
    pub async fn execute_raw(&self, sql: &str, params: Vec<Value>) -> Result<u64> {
        match &self.inner {
            #[cfg(feature = "runtime-mysql")]
            PoolInner::MySql(p) => {
                let q = bind_mysql(sqlx::query(sql), &params);
                let r = q.execute(p).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(r.rows_affected())
            }
            #[cfg(feature = "runtime-postgres")]
            PoolInner::Postgres(p) => {
                let q = bind_pg(sqlx::query(sql), &params);
                let r = q.execute(p).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(r.rows_affected())
            }
            #[cfg(feature = "runtime-sqlite")]
            PoolInner::Sqlite(p) => {
                let q = bind_sqlite(sqlx::query(sql), &params);
                let r = q.execute(p).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(r.rows_affected())
            }
        }
    }

    /// SELECT que retorna todas las filas como `Vec<Row>`.
    pub async fn fetch_all<Q: Query>(&self, q: &Q) -> Result<Vec<Row>> {
        let (sql, params) = self.db.build(q)?;
        self.fetch_all_raw(&sql, params).await
    }

    pub async fn fetch_all_raw(&self, sql: &str, params: Vec<Value>) -> Result<Vec<Row>> {
        match &self.inner {
            #[cfg(feature = "runtime-mysql")]
            PoolInner::MySql(p) => {
                let q = bind_mysql(sqlx::query(sql), &params);
                let rows = q.fetch_all(p).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(rows.into_iter().map(|r| mysql_row_to_map(&r)).collect())
            }
            #[cfg(feature = "runtime-postgres")]
            PoolInner::Postgres(p) => {
                let q = bind_pg(sqlx::query(sql), &params);
                let rows = q.fetch_all(p).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(rows.into_iter().map(|r| pg_row_to_map(&r)).collect())
            }
            #[cfg(feature = "runtime-sqlite")]
            PoolInner::Sqlite(p) => {
                let q = bind_sqlite(sqlx::query(sql), &params);
                let rows = q.fetch_all(p).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(rows.into_iter().map(|r| sqlite_row_to_map(&r)).collect())
            }
        }
    }

    pub async fn fetch_one<Q: Query>(&self, q: &Q) -> Result<Row> {
        let mut rows = self.fetch_all(q).await?;
        if rows.is_empty() {
            Err(QueryError::Driver("fetch_one: 0 filas".into()))
        } else {
            Ok(rows.swap_remove(0))
        }
    }

    pub async fn fetch_optional<Q: Query>(&self, q: &Q) -> Result<Option<Row>> {
        let mut rows = self.fetch_all(q).await?;
        Ok(if rows.is_empty() { None } else { Some(rows.swap_remove(0)) })
    }

    /// Health check: corre `SELECT 1` contra el pool. Si retorna Ok,
    /// hay al menos una conexión viva.
    pub async fn ping(&self) -> Result<()> {
        self.execute_raw("SELECT 1", vec![]).await.map(|_| ())
    }

    /// Streaming: itera filas una a una sin cargar todo a memoria.
    /// El callback recibe cada `Row`. Si retorna `Err`, se aborta el
    /// stream. Útil para datasets grandes (ETL, exports).
    pub async fn for_each_row<Q: Query, F>(&self, q: &Q, mut f: F) -> Result<()>
    where
        F: FnMut(Row) -> Result<()>,
    {
        use futures::StreamExt;
        let (sql, params) = self.db.build(q)?;
        match &self.inner {
            #[cfg(feature = "runtime-mysql")]
            PoolInner::MySql(p) => {
                let qx = bind_mysql(sqlx::query(&sql), &params);
                let mut s = qx.fetch(p);
                while let Some(row) = s.next().await {
                    let row = row.map_err(|e| driver_ctx(e, &sql, &params))?;
                    f(mysql_row_to_map(&row))?;
                }
            }
            #[cfg(feature = "runtime-postgres")]
            PoolInner::Postgres(p) => {
                let qx = bind_pg(sqlx::query(&sql), &params);
                let mut s = qx.fetch(p);
                while let Some(row) = s.next().await {
                    let row = row.map_err(|e| driver_ctx(e, &sql, &params))?;
                    f(pg_row_to_map(&row))?;
                }
            }
            #[cfg(feature = "runtime-sqlite")]
            PoolInner::Sqlite(p) => {
                let qx = bind_sqlite(sqlx::query(&sql), &params);
                let mut s = qx.fetch(p);
                while let Some(row) = s.next().await {
                    let row = row.map_err(|e| driver_ctx(e, &sql, &params))?;
                    f(sqlite_row_to_map(&row))?;
                }
            }
        }
        Ok(())
    }

    /// Streaming + mapeo a struct con `FromRow`.
    pub async fn for_each_as<Q: Query, T: FromRow, F>(&self, q: &Q, mut f: F) -> Result<()>
    where
        F: FnMut(T) -> Result<()>,
    {
        self.for_each_row(q, |row| {
            let t = T::from_row(&row)?;
            f(t)
        })
        .await
    }

    /// Stream real `Pin<Box<dyn Stream<Item = Result<Row>>>>`. Útil
    /// cuando querés combinar con combinators (`.take()`, `.filter()`,
    /// `.map()`) o consumir desde otro componente. Bajo el capó usa
    /// un canal mpsc(64) y una task spawneada que pumpea las filas.
    /// Requiere runtime tokio.
    pub fn fetch_stream<Q: Query>(
        &self,
        q: &Q,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<Row>> + Send>>>
    {
        use futures::stream::poll_fn;
        let (sql, params) = self.db.build(q)?;
        let inner = self.inner.clone();
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Row>>(64);
        tokio::spawn(stream_pump(inner, sql, params, tx));
        let mut rx = rx;
        Ok(Box::pin(poll_fn(move |cx| rx.poll_recv(cx))))
    }

    /// Igual que `fetch_stream` pero mapea cada fila a `T: FromRow`.
    pub fn fetch_stream_as<T, Q>(
        &self,
        q: &Q,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<T>> + Send>>>
    where
        T: FromRow + Send + 'static,
        Q: Query,
    {
        use futures::StreamExt;
        let s = self.fetch_stream(q)?;
        Ok(Box::pin(s.map(|r| r.and_then(|row| T::from_row(&row)))))
    }

    /// `EXPLAIN <query>`. Retorna las filas del plan.
    pub async fn explain<Q: Query>(&self, q: &Q) -> Result<Vec<Row>> {
        let (sql, params) = self.db.explain(q)?;
        self.fetch_all_raw(&sql, params).await
    }

    /// `EXPLAIN ANALYZE` / `EXPLAIN QUERY PLAN`. Ejecuta la query y
    /// reporta el plan ejecutado + tiempos.
    pub async fn explain_analyze<Q: Query>(&self, q: &Q) -> Result<Vec<Row>> {
        let (sql, params) = self.db.explain_analyze(q)?;
        self.fetch_all_raw(&sql, params).await
    }

    // FromRow: mapeo a struct propio
    pub async fn fetch_all_as<T: FromRow, Q: Query>(&self, q: &Q) -> Result<Vec<T>> {
        let rows = self.fetch_all(q).await?;
        rows.iter().map(T::from_row).collect()
    }
    pub async fn fetch_one_as<T: FromRow, Q: Query>(&self, q: &Q) -> Result<T> {
        let row = self.fetch_one(q).await?;
        T::from_row(&row)
    }
    pub async fn fetch_optional_as<T: FromRow, Q: Query>(&self, q: &Q) -> Result<Option<T>> {
        match self.fetch_optional(q).await? {
            Some(r) => Ok(Some(T::from_row(&r)?)),
            None => Ok(None),
        }
    }

    /// Ejecuta varias queries dentro de una sola transacción.
    /// Si una falla, hace rollback de todas. Retorna suma de filas afectadas.
    pub async fn execute_many<Q: Query>(&self, queries: &[Q]) -> Result<u64> {
        let mut tx = self.begin().await?;
        let mut total = 0;
        for q in queries {
            total += tx.execute(q).await?;
        }
        tx.commit().await?;
        Ok(total)
    }

    /// Ejecuta varias queries reutilizando una sola conexión, **sin
    /// transacción**. Cada query se confirma sola. Si una falla, las
    /// anteriores ya quedaron aplicadas. Retorna `Vec<rows_affected>`.
    /// Más rápido que múltiples `execute()` cuando el pool está lejos.
    pub async fn execute_batch<Q: Query>(&self, queries: &[Q]) -> Result<Vec<u64>> {
        let mut out = Vec::with_capacity(queries.len());
        match &self.inner {
            #[cfg(feature = "runtime-mysql")]
            PoolInner::MySql(p) => {
                let mut conn = p.acquire().await.map_err(driver)?;
                for q in queries {
                    let (sql, params) = self.db.build(q)?;
                    let qx = bind_mysql(sqlx::query(&sql), &params);
                    let r = qx.execute(&mut *conn).await
                        .map_err(|e| driver_ctx(e, &sql, &params))?;
                    out.push(r.rows_affected());
                }
            }
            #[cfg(feature = "runtime-postgres")]
            PoolInner::Postgres(p) => {
                let mut conn = p.acquire().await.map_err(driver)?;
                for q in queries {
                    let (sql, params) = self.db.build(q)?;
                    let qx = bind_pg(sqlx::query(&sql), &params);
                    let r = qx.execute(&mut *conn).await
                        .map_err(|e| driver_ctx(e, &sql, &params))?;
                    out.push(r.rows_affected());
                }
            }
            #[cfg(feature = "runtime-sqlite")]
            PoolInner::Sqlite(p) => {
                let mut conn = p.acquire().await.map_err(driver)?;
                for q in queries {
                    let (sql, params) = self.db.build(q)?;
                    let qx = bind_sqlite(sqlx::query(&sql), &params);
                    let r = qx.execute(&mut *conn).await
                        .map_err(|e| driver_ctx(e, &sql, &params))?;
                    out.push(r.rows_affected());
                }
            }
        }
        Ok(out)
    }

    /// Versión raw del batch sin transacción.
    pub async fn execute_batch_raw(
        &self,
        statements: &[(String, Vec<Value>)],
    ) -> Result<Vec<u64>> {
        let mut out = Vec::with_capacity(statements.len());
        match &self.inner {
            #[cfg(feature = "runtime-mysql")]
            PoolInner::MySql(p) => {
                let mut conn = p.acquire().await.map_err(driver)?;
                for (sql, params) in statements {
                    let qx = bind_mysql(sqlx::query(sql), params);
                    let r = qx.execute(&mut *conn).await
                        .map_err(|e| driver_ctx(e, sql, params))?;
                    out.push(r.rows_affected());
                }
            }
            #[cfg(feature = "runtime-postgres")]
            PoolInner::Postgres(p) => {
                let mut conn = p.acquire().await.map_err(driver)?;
                for (sql, params) in statements {
                    let qx = bind_pg(sqlx::query(sql), params);
                    let r = qx.execute(&mut *conn).await
                        .map_err(|e| driver_ctx(e, sql, params))?;
                    out.push(r.rows_affected());
                }
            }
            #[cfg(feature = "runtime-sqlite")]
            PoolInner::Sqlite(p) => {
                let mut conn = p.acquire().await.map_err(driver)?;
                for (sql, params) in statements {
                    let qx = bind_sqlite(sqlx::query(sql), params);
                    let r = qx.execute(&mut *conn).await
                        .map_err(|e| driver_ctx(e, sql, params))?;
                    out.push(r.rows_affected());
                }
            }
        }
        Ok(out)
    }

    /// Tx con cierre: commit si Ok, rollback si Err. Más ergonómico.
    /// Por estable Rust requiere `Box::pin(async move { ... })`.
    ///
    /// ```ignore
    /// pool.transaction(|tx| Box::pin(async move {
    ///     tx.execute(...).await?;
    ///     tx.execute(...).await?;
    ///     Ok::<_, QueryError>(())
    /// })).await?;
    /// ```
    pub async fn transaction<T, F>(&self, f: F) -> Result<T>
    where
        F: for<'a> FnOnce(
            &'a mut Tx,
        )
            -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>,
    {
        let mut tx = self.begin().await?;
        match f(&mut tx).await {
            Ok(v) => {
                tx.commit().await?;
                Ok(v)
            }
            Err(e) => {
                let _ = tx.rollback().await;
                Err(e)
            }
        }
    }
}

// helpers
async fn connect_with_retry<F, Fut>(max_attempts: usize, mut f: F) -> Result<Pool>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Pool>>,
{
    let max = max_attempts.max(1);
    let mut attempt = 0;
    loop {
        attempt += 1;
        match f().await {
            Ok(p) => return Ok(p),
            Err(e) if attempt < max && is_transient(&e) => {
                sleep_backoff(attempt).await;
            }
            Err(e) => return Err(e),
        }
    }
}

fn driver(e: sqlx::Error) -> QueryError {
    QueryError::Driver(e.to_string())
}

/// Versión enriquecida: agrega SQL y params (truncados) al mensaje.
/// Útil para debug en producción sin envolver el error en otro tipo.
fn driver_ctx(e: sqlx::Error, sql: &str, params: &[Value]) -> QueryError {
    let mut s = e.to_string();
    s.push_str("\n  sql: ");
    if sql.len() > 300 {
        s.push_str(&sql[..300]);
        s.push_str("...");
    } else {
        s.push_str(sql);
    }
    if !params.is_empty() {
        let cap = params.len().min(20);
        s.push_str("\n  params: ");
        s.push_str(&format!("{:?}", &params[..cap]));
        if params.len() > cap {
            s.push_str(&format!(" (+{} más)", params.len() - cap));
        }
    }
    QueryError::Driver(s)
}

// bind helpers por backend

#[cfg(feature = "runtime-mysql")]
fn bind_mysql<'q>(
    mut q: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    params: &'q [Value],
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    for v in params {
        q = match v {
            Value::Null => q.bind(Option::<i64>::None),
            Value::Bool(b) => q.bind(*b),
            Value::Int(i) => q.bind(*i),
            Value::Float(f) => q.bind(*f),
            Value::Text(s) | Value::Json(s) => q.bind(s.clone()),
            Value::Bytes(b) => q.bind(b.clone()),
        };
    }
    q
}

#[cfg(feature = "runtime-postgres")]
fn bind_pg<'q>(
    mut q: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    params: &'q [Value],
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    for v in params {
        q = match v {
            Value::Null => q.bind(Option::<i64>::None),
            Value::Bool(b) => q.bind(*b),
            Value::Int(i) => q.bind(*i),
            Value::Float(f) => q.bind(*f),
            Value::Text(s) | Value::Json(s) => q.bind(s.clone()),
            Value::Bytes(b) => q.bind(b.clone()),
        };
    }
    q
}

#[cfg(feature = "runtime-sqlite")]
fn bind_sqlite<'q>(
    mut q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    params: &'q [Value],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    for v in params {
        q = match v {
            Value::Null => q.bind(Option::<i64>::None),
            Value::Bool(b) => q.bind(*b),
            Value::Int(i) => q.bind(*i),
            Value::Float(f) => q.bind(*f),
            Value::Text(s) | Value::Json(s) => q.bind(s.clone()),
            Value::Bytes(b) => q.bind(b.clone()),
        };
    }
    q
}

// row -> HashMap por backend

#[cfg(feature = "runtime-mysql")]
fn mysql_row_to_map(r: &sqlx::mysql::MySqlRow) -> Row {
    use sqlx::Column as _;
    use sqlx::Row as _;
    use sqlx::TypeInfo as _;
    let mut map = HashMap::new();
    for (i, col) in r.columns().iter().enumerate() {
        let name = col.name().to_string();
        let v = decode_mysql(r, i, col.type_info().name());
        map.insert(name, v);
    }
    map
}

#[cfg(feature = "runtime-mysql")]
fn decode_mysql(r: &sqlx::mysql::MySqlRow, i: usize, ty: &str) -> Value {
    use sqlx::Row as _;
    if let Ok(None::<i64>) = r.try_get::<Option<i64>, _>(i) {
        return Value::Null;
    }
    match ty {
        "BOOLEAN" | "TINYINT" | "TINYINT UNSIGNED" => r.try_get::<i64, _>(i).map(|v| Value::Bool(v != 0)).unwrap_or(Value::Null),
        "SMALLINT" | "SMALLINT UNSIGNED" | "INT" | "INT UNSIGNED" | "BIGINT" | "BIGINT UNSIGNED" | "MEDIUMINT" | "MEDIUMINT UNSIGNED" => {
            r.try_get::<i64, _>(i).map(Value::Int).unwrap_or(Value::Null)
        }
        "FLOAT" | "DOUBLE" => r.try_get::<f64, _>(i).map(Value::Float).unwrap_or(Value::Null),
        // DECIMAL: como string para no perder precisión vs f64.
        "DECIMAL" | "NEWDECIMAL" | "NUMERIC" => r
            .try_get::<String, _>(i)
            .map(Value::Text)
            .unwrap_or(Value::Null),
        "JSON" => r.try_get::<String, _>(i).map(Value::Json).unwrap_or(Value::Null),
        "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "VARBINARY" | "BINARY" => {
            r.try_get::<Vec<u8>, _>(i).map(Value::Bytes).unwrap_or(Value::Null)
        }
        _ => r.try_get::<String, _>(i).map(Value::Text).unwrap_or(Value::Null),
    }
}

#[cfg(feature = "runtime-postgres")]
fn pg_row_to_map(r: &sqlx::postgres::PgRow) -> Row {
    use sqlx::Column as _;
    use sqlx::Row as _;
    use sqlx::TypeInfo as _;
    let mut map = HashMap::new();
    for (i, col) in r.columns().iter().enumerate() {
        let name = col.name().to_string();
        let v = decode_pg(r, i, col.type_info().name());
        map.insert(name, v);
    }
    map
}

#[cfg(feature = "runtime-postgres")]
fn decode_pg(r: &sqlx::postgres::PgRow, i: usize, ty: &str) -> Value {
    use sqlx::Row as _;
    match ty {
        "BOOL" => r.try_get::<bool, _>(i).map(Value::Bool).unwrap_or(Value::Null),
        "INT2" | "INT4" | "INT8" => r.try_get::<i64, _>(i).map(Value::Int).unwrap_or(Value::Null),
        "FLOAT4" | "FLOAT8" => r.try_get::<f64, _>(i).map(Value::Float).unwrap_or(Value::Null),
        // NUMERIC/DECIMAL en PG: string para preservar precisión.
        "NUMERIC" => r.try_get::<String, _>(i).map(Value::Text).unwrap_or(Value::Null),
        "UUID" => r.try_get::<String, _>(i).map(Value::Text).unwrap_or(Value::Null),
        "JSON" | "JSONB" => r.try_get::<String, _>(i).map(Value::Json).unwrap_or(Value::Null),
        "BYTEA" => r.try_get::<Vec<u8>, _>(i).map(Value::Bytes).unwrap_or(Value::Null),
        // Fechas/horas y demás: siempre como string. Preserva el offset
        // de TIMESTAMPTZ y TIMETZ, y evita pelearnos con chrono/time
        // sin pedir esa dep al usuario.
        "TIMESTAMP" | "TIMESTAMPTZ" | "DATE" | "TIME" | "TIMETZ" => {
            r.try_get::<String, _>(i).map(Value::Text).unwrap_or(Value::Null)
        }
        _ => r.try_get::<String, _>(i).map(Value::Text).unwrap_or(Value::Null),
    }
}

#[cfg(feature = "runtime-sqlite")]
fn sqlite_row_to_map(r: &sqlx::sqlite::SqliteRow) -> Row {
    use sqlx::Column as _;
    use sqlx::Row as _;
    use sqlx::TypeInfo as _;
    let mut map = HashMap::new();
    for (i, col) in r.columns().iter().enumerate() {
        let name = col.name().to_string();
        let ty = col.type_info().name();
        // Bindeamos como Option<T> primero — SQLite es de tipado dinámico
        // y cuando el valor real es NULL, el type_info puede no coincidir
        // con el declarado. Si hay error de tipo, caemos a probar como
        // string y finalmente Value::Null.
        let v = match ty {
            "INTEGER" | "INT" | "BIGINT" => r
                .try_get::<Option<i64>, _>(i)
                .ok()
                .flatten()
                .map(Value::Int)
                .unwrap_or(Value::Null),
            "REAL" | "DOUBLE" | "FLOAT" => r
                .try_get::<Option<f64>, _>(i)
                .ok()
                .flatten()
                .map(Value::Float)
                .unwrap_or(Value::Null),
            "BOOLEAN" => r
                .try_get::<Option<bool>, _>(i)
                .ok()
                .flatten()
                .map(Value::Bool)
                .unwrap_or(Value::Null),
            "BLOB" => r
                .try_get::<Option<Vec<u8>>, _>(i)
                .ok()
                .flatten()
                .map(Value::Bytes)
                .unwrap_or(Value::Null),
            _ => r
                .try_get::<Option<String>, _>(i)
                .ok()
                .flatten()
                .map(Value::Text)
                .unwrap_or(Value::Null),
        };
        map.insert(name, v);
    }
    map
}

/// Mantiene `Arc` por compat futura.
pub(crate) type _PoolArc = Arc<Pool>;

// Transacciones
/// Transacción activa. Soporta `execute` / `fetch_*` igual que `Pool`,
/// más `commit()` / `rollback()`. Si el `Tx` se dropea sin commit,
/// sqlx hace rollback automático.
pub struct Tx {
    db: Db,
    inner: TxInner,
}

enum TxInner {
    #[cfg(feature = "runtime-mysql")]
    MySql(sqlx::Transaction<'static, sqlx::MySql>),
    #[cfg(feature = "runtime-postgres")]
    Postgres(sqlx::Transaction<'static, sqlx::Postgres>),
    #[cfg(feature = "runtime-sqlite")]
    Sqlite(sqlx::Transaction<'static, sqlx::Sqlite>),
}

impl Tx {
    pub fn db(&self) -> &Db {
        &self.db
    }

    pub async fn execute<Q: Query>(&mut self, q: &Q) -> Result<u64> {
        let (sql, params) = self.db.build(q)?;
        self.execute_raw(&sql, params).await
    }

    pub async fn execute_raw(&mut self, sql: &str, params: Vec<Value>) -> Result<u64> {
        match &mut self.inner {
            #[cfg(feature = "runtime-mysql")]
            TxInner::MySql(tx) => {
                let q = bind_mysql(sqlx::query(sql), &params);
                let r = q.execute(&mut **tx).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(r.rows_affected())
            }
            #[cfg(feature = "runtime-postgres")]
            TxInner::Postgres(tx) => {
                let q = bind_pg(sqlx::query(sql), &params);
                let r = q.execute(&mut **tx).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(r.rows_affected())
            }
            #[cfg(feature = "runtime-sqlite")]
            TxInner::Sqlite(tx) => {
                let q = bind_sqlite(sqlx::query(sql), &params);
                let r = q.execute(&mut **tx).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(r.rows_affected())
            }
        }
    }

    pub async fn fetch_all<Q: Query>(&mut self, q: &Q) -> Result<Vec<Row>> {
        let (sql, params) = self.db.build(q)?;
        self.fetch_all_raw(&sql, params).await
    }

    pub async fn fetch_all_raw(&mut self, sql: &str, params: Vec<Value>) -> Result<Vec<Row>> {
        match &mut self.inner {
            #[cfg(feature = "runtime-mysql")]
            TxInner::MySql(tx) => {
                let q = bind_mysql(sqlx::query(sql), &params);
                let rows = q.fetch_all(&mut **tx).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(rows.into_iter().map(|r| mysql_row_to_map(&r)).collect())
            }
            #[cfg(feature = "runtime-postgres")]
            TxInner::Postgres(tx) => {
                let q = bind_pg(sqlx::query(sql), &params);
                let rows = q.fetch_all(&mut **tx).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(rows.into_iter().map(|r| pg_row_to_map(&r)).collect())
            }
            #[cfg(feature = "runtime-sqlite")]
            TxInner::Sqlite(tx) => {
                let q = bind_sqlite(sqlx::query(sql), &params);
                let rows = q.fetch_all(&mut **tx).await.map_err(|e| driver_ctx(e, sql, &params))?;
                Ok(rows.into_iter().map(|r| sqlite_row_to_map(&r)).collect())
            }
        }
    }

    pub async fn fetch_one<Q: Query>(&mut self, q: &Q) -> Result<Row> {
        let mut rows = self.fetch_all(q).await?;
        if rows.is_empty() {
            Err(QueryError::Driver("fetch_one: 0 filas".into()))
        } else {
            Ok(rows.swap_remove(0))
        }
    }

    pub async fn fetch_optional<Q: Query>(&mut self, q: &Q) -> Result<Option<Row>> {
        let mut rows = self.fetch_all(q).await?;
        Ok(if rows.is_empty() { None } else { Some(rows.swap_remove(0)) })
    }

    pub async fn commit(self) -> Result<()> {
        match self.inner {
            #[cfg(feature = "runtime-mysql")]
            TxInner::MySql(tx) => tx.commit().await.map_err(driver),
            #[cfg(feature = "runtime-postgres")]
            TxInner::Postgres(tx) => tx.commit().await.map_err(driver),
            #[cfg(feature = "runtime-sqlite")]
            TxInner::Sqlite(tx) => tx.commit().await.map_err(driver),
        }
    }

    pub async fn rollback(self) -> Result<()> {
        match self.inner {
            #[cfg(feature = "runtime-mysql")]
            TxInner::MySql(tx) => tx.rollback().await.map_err(driver),
            #[cfg(feature = "runtime-postgres")]
            TxInner::Postgres(tx) => tx.rollback().await.map_err(driver),
            #[cfg(feature = "runtime-sqlite")]
            TxInner::Sqlite(tx) => tx.rollback().await.map_err(driver),
        }
    }

    /// Crea un savepoint dentro de la transacción. El nombre se valida
    /// como identifier. Después podés `rollback_to_savepoint` o
    /// `release_savepoint`.
    pub async fn savepoint(&mut self, name: &str) -> Result<()> {
        crate::ident::validate(name)?;
        self.exec_unprepared(&format!("SAVEPOINT {}", name)).await
    }
    pub async fn rollback_to_savepoint(&mut self, name: &str) -> Result<()> {
        crate::ident::validate(name)?;
        self.exec_unprepared(&format!("ROLLBACK TO SAVEPOINT {}", name)).await
    }
    pub async fn release_savepoint(&mut self, name: &str) -> Result<()> {
        crate::ident::validate(name)?;
        self.exec_unprepared(&format!("RELEASE SAVEPOINT {}", name)).await
    }

    /// MySQL rechaza ciertas sentencias (SAVEPOINT, SET ...) en el
    /// protocolo de prepared statements. Usamos texto directo.
    async fn exec_unprepared(&mut self, sql: &str) -> Result<()> {
        use sqlx::Executor;
        match &mut self.inner {
            #[cfg(feature = "runtime-mysql")]
            TxInner::MySql(tx) => {
                (&mut **tx).execute(sqlx::raw_sql(sql))
                    .await
                    .map_err(|e| driver_ctx(e, sql, &[]))?;
            }
            #[cfg(feature = "runtime-postgres")]
            TxInner::Postgres(tx) => {
                (&mut **tx).execute(sqlx::raw_sql(sql))
                    .await
                    .map_err(|e| driver_ctx(e, sql, &[]))?;
            }
            #[cfg(feature = "runtime-sqlite")]
            TxInner::Sqlite(tx) => {
                (&mut **tx).execute(sqlx::raw_sql(sql))
                    .await
                    .map_err(|e| driver_ctx(e, sql, &[]))?;
            }
        }
        Ok(())
    }
}

impl Pool {
    /// Abre una transacción nueva.
    pub async fn begin(&self) -> Result<Tx> {
        let inner = match &self.inner {
            #[cfg(feature = "runtime-mysql")]
            PoolInner::MySql(p) => TxInner::MySql(p.begin().await.map_err(driver)?),
            #[cfg(feature = "runtime-postgres")]
            PoolInner::Postgres(p) => TxInner::Postgres(p.begin().await.map_err(driver)?),
            #[cfg(feature = "runtime-sqlite")]
            PoolInner::Sqlite(p) => TxInner::Sqlite(p.begin().await.map_err(driver)?),
        };
        Ok(Tx { db: self.db.clone(), inner })
    }
}

// Retry con backoff exponencial
/// Decide si un error del driver es transitorio (vale la pena reintentar)
/// o permanente (error de SQL, constraint, etc.).
fn is_transient(e: &QueryError) -> bool {
    let s = match e {
        QueryError::Driver(s) => s.to_lowercase(),
        _ => return false,
    };
    s.contains("connection") || s.contains("connect refused")
        || s.contains("broken pipe") || s.contains("reset by peer")
        || s.contains("timed out") || s.contains("timeout")
        || s.contains("deadlock") || s.contains("lock wait")
        || s.contains("serialization") || s.contains("could not serialize")
}

async fn sleep_backoff(attempt: usize) {
    // 50ms, 100, 200, 400, 800, 1600, ... cap 5s
    let ms = (50u64 << (attempt - 1)).min(5000);
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

impl Pool {
    /// Como `execute()`, pero reintenta con backoff exponencial cuando
    /// el error parece transitorio (conexión caída, deadlock,
    /// serialización, timeout). `max_attempts` incluye el primer intento.
    pub async fn execute_retry<Q: Query>(&self, q: &Q, max_attempts: usize) -> Result<u64> {
        let max = max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.execute(q).await {
                Ok(n) => return Ok(n),
                Err(e) if attempt < max && is_transient(&e) => {
                    sleep_backoff(attempt).await;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Como `fetch_all()`, pero con retry idéntico a `execute_retry`.
    pub async fn fetch_all_retry<Q: Query>(&self, q: &Q, max_attempts: usize) -> Result<Vec<Row>> {
        let max = max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.fetch_all(q).await {
                Ok(v) => return Ok(v),
                Err(e) if attempt < max && is_transient(&e) => {
                    sleep_backoff(attempt).await;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Versión raw del retry — útil para DDL u otras sentencias sin builder.
    pub async fn execute_raw_retry(
        &self,
        sql: &str,
        params: Vec<Value>,
        max_attempts: usize,
    ) -> Result<u64> {
        let max = max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.execute_raw(sql, params.clone()).await {
                Ok(n) => return Ok(n),
                Err(e) if attempt < max && is_transient(&e) => {
                    sleep_backoff(attempt).await;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

async fn stream_pump(
    inner: PoolInner,
    sql: String,
    params: Vec<Value>,
    tx: tokio::sync::mpsc::Sender<Result<Row>>,
) {
    use futures::StreamExt;
    match inner {
        #[cfg(feature = "runtime-mysql")]
        PoolInner::MySql(p) => {
            let qx = bind_mysql(sqlx::query(&sql), &params);
            let mut s = qx.fetch(&p);
            while let Some(row) = s.next().await {
                let mapped = match row {
                    Ok(r) => Ok(mysql_row_to_map(&r)),
                    Err(e) => Err(driver_ctx(e, &sql, &params)),
                };
                if tx.send(mapped).await.is_err() { break; }
            }
        }
        #[cfg(feature = "runtime-postgres")]
        PoolInner::Postgres(p) => {
            let qx = bind_pg(sqlx::query(&sql), &params);
            let mut s = qx.fetch(&p);
            while let Some(row) = s.next().await {
                let mapped = match row {
                    Ok(r) => Ok(pg_row_to_map(&r)),
                    Err(e) => Err(driver_ctx(e, &sql, &params)),
                };
                if tx.send(mapped).await.is_err() { break; }
            }
        }
        #[cfg(feature = "runtime-sqlite")]
        PoolInner::Sqlite(p) => {
            let qx = bind_sqlite(sqlx::query(&sql), &params);
            let mut s = qx.fetch(&p);
            while let Some(row) = s.next().await {
                let mapped = match row {
                    Ok(r) => Ok(sqlite_row_to_map(&r)),
                    Err(e) => Err(driver_ctx(e, &sql, &params)),
                };
                if tx.send(mapped).await.is_err() { break; }
            }
        }
    }
}
