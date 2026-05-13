//! Test de integración del Pool con sqlite en memoria. Solo corre con
//! la feature `runtime-sqlite`:
//!   cargo test --features runtime-sqlite --test runtime_sqlite

#![cfg(feature = "runtime-sqlite")]

use medoo_rs::runtime::{FromRow, Pool, PoolOptions, Row, RowExt};
use medoo_rs::{record, ColDef, ColType, Logger, LogCategory, QueryError};

#[tokio::test]
async fn pool_sqlite_create_insert_select() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();

    // CREATE TABLE
    let ddl = pool.db().create_table("users")
        .col(ColDef::new("id", ColType::Int).primary_key().auto_increment())
        .col(ColDef::new("name", ColType::Text).not_null())
        .col(ColDef::new("age", ColType::Int).not_null())
        .col(ColDef::new("active", ColType::Bool).not_null().default_raw("1"))
        .to_sql().unwrap();
    pool.execute_raw(&ddl, vec![]).await.unwrap();

    // INSERT 3 filas
    let n = pool.execute(&pool.insert("users")
        .set(record!{ "name" => "Ana",  "age" => 30, "active" => true })
        .set(record!{ "name" => "Luis", "age" => 25, "active" => true })
        .set(record!{ "name" => "Mara", "age" => 17, "active" => false })
    ).await.unwrap();
    assert_eq!(n, 3);

    // SELECT WHERE
    let rows = pool.fetch_all(
        &pool.select("users")
            .where_op("age", ">=", 18)
            .order_asc("name")
    ).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get_str("name"), Some("Ana"));
    assert_eq!(rows[1].get_str("name"), Some("Luis"));
    assert_eq!(rows[0].get_i64("age"), Some(30));
}

#[tokio::test]
async fn pool_sqlite_update_and_delete() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    let ddl = pool.db().create_table("t")
        .col(ColDef::new("id", ColType::Int).primary_key().auto_increment())
        .col(ColDef::new("x", ColType::Int).not_null())
        .to_sql().unwrap();
    pool.execute_raw(&ddl, vec![]).await.unwrap();

    pool.execute(&pool.insert("t")
        .set(record!{ "x" => 1 }).set(record!{ "x" => 2 }).set(record!{ "x" => 3 })
    ).await.unwrap();

    let n = pool.execute(&pool.update("t").set("x", 99).where_op("x", ">", 1)).await.unwrap();
    assert_eq!(n, 2);

    let n = pool.execute(&pool.delete("t").where_eq("x", 99)).await.unwrap();
    assert_eq!(n, 2);

    let rows = pool.fetch_all(&pool.select("t")).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_i64("x"), Some(1));
}

#[tokio::test]
async fn pool_fetch_one_and_optional() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();
    pool.execute(&pool.insert("t").set(record!{ "n" => "hola" })).await.unwrap();

    let one = pool.fetch_one(&pool.select("t").where_eq("id", 1)).await.unwrap();
    assert_eq!(one.get_str("n"), Some("hola"));

    let opt = pool.fetch_optional(&pool.select("t").where_eq("id", 999)).await.unwrap();
    assert!(opt.is_none());

    let err = pool.fetch_one(&pool.select("t").where_eq("id", 999)).await.unwrap_err();
    assert!(matches!(err, medoo_rs::QueryError::Driver(_)));
}

#[tokio::test]
async fn pool_logger_captures_executed_sql() {
    let (logger, buf) = Logger::buffer();
    let logger = logger.filter(LogCategory::ALL);
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap().with_logger(logger);
    pool.execute_raw("CREATE TABLE u (id INTEGER PRIMARY KEY, name TEXT)", vec![]).await.unwrap();
    pool.execute(&pool.insert("u").set(record!{ "name" => "Ana" })).await.unwrap();
    pool.fetch_all(&pool.select("u")).await.unwrap();

    let captured = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(captured.contains("[INSERT"));
    assert!(captured.contains("[READ"));
    assert!(captured.contains(r#"INSERT INTO "u""#));
}

#[tokio::test]
async fn pool_handles_null_and_types() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw(
        "CREATE TABLE m (id INTEGER PRIMARY KEY, s TEXT, i INTEGER, f REAL, b INTEGER)",
        vec![],
    ).await.unwrap();
    let nullstr: Option<&str> = None;
    pool.execute(&pool.insert("m")
        .set(record!{ "s" => nullstr, "i" => 42, "f" => 3.14, "b" => true })
    ).await.unwrap();

    let row = pool.fetch_one(&pool.select("m").where_eq("id", 1)).await.unwrap();
    assert!(matches!(row.get("s"), Some(medoo_rs::Value::Null)));
    assert_eq!(row.get_i64("i"), Some(42));
    assert_eq!(row.get_f64("f"), Some(3.14));
    assert_eq!(row.get_bool("b"), Some(true));
}

#[tokio::test]
async fn tx_commit_persists_changes() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    tx.execute(&pool.insert("t").set(record!{ "n" => "ana" })).await.unwrap();
    tx.execute(&pool.insert("t").set(record!{ "n" => "luis" })).await.unwrap();
    tx.commit().await.unwrap();

    let rows = pool.fetch_all(&pool.select("t")).await.unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn tx_rollback_discards_changes() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();
    pool.execute(&pool.insert("t").set(record!{ "n" => "previa" })).await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    tx.execute(&pool.insert("t").set(record!{ "n" => "se cancela" })).await.unwrap();
    tx.rollback().await.unwrap();

    let rows = pool.fetch_all(&pool.select("t")).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_str("n"), Some("previa"));
}

#[tokio::test]
async fn tx_drop_without_commit_rolls_back() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();

    {
        let mut tx = pool.begin().await.unwrap();
        tx.execute(&pool.insert("t").set(record!{ "n" => "huérfano" })).await.unwrap();
    } // tx dropped -> rollback automático

    let rows = pool.fetch_all(&pool.select("t")).await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn tx_fetch_inside_sees_pending_changes() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    tx.execute(&pool.insert("t").set(record!{ "n" => "x" })).await.unwrap();
    let rows = tx.fetch_all(&pool.select("t")).await.unwrap();
    assert_eq!(rows.len(), 1);
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn execute_retry_succeeds_first_try() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();
    let n = pool.execute_retry(
        &pool.insert("t").set(record!{ "n" => "ok" }),
        3
    ).await.unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn execute_retry_does_not_loop_on_permanent_error() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    let t0 = std::time::Instant::now();
    let err = pool.execute_raw_retry(
        "INSERT INTO no_existe (x) VALUES (?)",
        vec![medoo_rs::Value::Int(1)],
        5
    ).await.unwrap_err();
    let elapsed = t0.elapsed();
    assert!(matches!(err, medoo_rs::QueryError::Driver(_)));
    assert!(elapsed.as_millis() < 200, "no debe esperar backoff: {:?}", elapsed);
}

#[tokio::test]
async fn fetch_stream_returns_stream() {
    use futures::StreamExt;
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE n (i INTEGER)", vec![]).await.unwrap();
    pool.execute_raw("INSERT INTO n VALUES (1), (2), (3), (4), (5)", vec![]).await.unwrap();

    let mut s = pool.fetch_stream(&pool.select("n").order_asc("i")).unwrap();
    let mut got: Vec<i64> = Vec::new();
    while let Some(row) = s.next().await {
        got.push(row.unwrap().get_i64("i").unwrap());
    }
    assert_eq!(got, vec![1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn fetch_stream_with_take_combinator() {
    use futures::StreamExt;
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE n (i INTEGER)", vec![]).await.unwrap();
    let mut ins = pool.insert("n");
    for i in 0..1000 { ins = ins.set(record!{ "i" => i }); }
    pool.execute(&ins).await.unwrap();

    // tomar solo las primeras 3, sin cargar las 1000 a memoria
    let s = pool.fetch_stream(&pool.select("n").order_asc("i")).unwrap();
    let xs: Vec<_> = s.take(3).collect().await;
    assert_eq!(xs.len(), 3);
}

#[cfg(feature = "derive")]
#[tokio::test]
async fn fetch_stream_as_typed() {
    use futures::StreamExt;
    use medoo_rs::FromRow;
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE u (id INTEGER PRIMARY KEY, name TEXT)", vec![]).await.unwrap();
    pool.execute(&pool.insert("u")
        .set(record!{ "name" => "ana" })
        .set(record!{ "name" => "luis" })
    ).await.unwrap();

    #[derive(FromRow)]
    struct User { id: i64, name: String }

    let mut s = pool.fetch_stream_as::<User, _>(&pool.select("u").order_asc("id")).unwrap();
    let a = s.next().await.unwrap().unwrap();
    let b = s.next().await.unwrap().unwrap();
    assert_eq!(a.name, "ana");
    assert_eq!(b.name, "luis");
    assert_eq!(a.id, 1);
}

#[tokio::test]
async fn pool_ping_works() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.ping().await.unwrap();
}

#[tokio::test]
async fn savepoint_rollback_partial() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    tx.execute(&pool.insert("t").set(record!{ "n" => "a" })).await.unwrap();
    tx.savepoint("sp1").await.unwrap();
    tx.execute(&pool.insert("t").set(record!{ "n" => "b" })).await.unwrap();
    tx.execute(&pool.insert("t").set(record!{ "n" => "c" })).await.unwrap();
    tx.rollback_to_savepoint("sp1").await.unwrap();
    tx.execute(&pool.insert("t").set(record!{ "n" => "d" })).await.unwrap();
    tx.commit().await.unwrap();

    let rows = pool.fetch_all(&pool.select("t").order_asc("id")).await.unwrap();
    let names: Vec<&str> = rows.iter().filter_map(|r| r.get_str("n")).collect();
    assert_eq!(names, vec!["a", "d"]);
}

#[tokio::test]
async fn savepoint_release() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY)", vec![]).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    tx.savepoint("sp").await.unwrap();
    tx.release_savepoint("sp").await.unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn for_each_row_streams_without_loading_all() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE n (i INTEGER)", vec![]).await.unwrap();
    let mut ins = pool.insert("n");
    for i in 0..100 {
        ins = ins.set(record!{ "i" => i });
    }
    pool.execute(&ins).await.unwrap();

    let mut count = 0;
    let mut sum: i64 = 0;
    pool.for_each_row(&pool.select("n").order_asc("i"), |row| {
        count += 1;
        sum += row.get_i64("i").unwrap_or(0);
        Ok(())
    }).await.unwrap();
    assert_eq!(count, 100);
    assert_eq!(sum, (0..100).sum::<i64>());
}

#[tokio::test]
async fn for_each_row_aborts_on_error() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE n (i INTEGER)", vec![]).await.unwrap();
    pool.execute_raw("INSERT INTO n VALUES (1), (2), (3), (4), (5)", vec![]).await.unwrap();
    let mut count = 0;
    let r = pool.for_each_row(&pool.select("n").order_asc("i"), |_row| {
        count += 1;
        if count >= 2 {
            Err(medoo_rs::QueryError::Driver("stop".into()))
        } else {
            Ok(())
        }
    }).await;
    assert!(r.is_err());
    assert_eq!(count, 2);
}

#[tokio::test]
async fn pool_explain_real() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE u (id INTEGER PRIMARY KEY, name TEXT)", vec![]).await.unwrap();
    let plan = pool.explain_analyze(&pool.select("u").where_eq("id", 1)).await.unwrap();
    // SQLite EXPLAIN QUERY PLAN siempre retorna al menos una fila
    assert!(!plan.is_empty());
}

#[tokio::test]
async fn execute_batch_no_tx_one_connection() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();

    let qs = vec![
        pool.insert("t").set(record!{ "n" => "a" }),
        pool.insert("t").set(record!{ "n" => "b" }),
        pool.insert("t").set(record!{ "n" => "c" }),
    ];
    let counts = pool.execute_batch(&qs).await.unwrap();
    assert_eq!(counts, vec![1, 1, 1]);

    let rows = pool.fetch_all(&pool.select("t")).await.unwrap();
    assert_eq!(rows.len(), 3);
}

#[tokio::test]
async fn execute_batch_no_rollback_on_failure() {
    // Confirma diferencia con execute_many: si una falla, las anteriores
    // quedan persistidas (no hay tx).
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT UNIQUE)", vec![]).await.unwrap();

    let stmts = vec![
        ("INSERT INTO t (n) VALUES (?)".to_string(), vec![medoo_rs::Value::Text("a".into())]),
        ("INSERT INTO t (n) VALUES (?)".to_string(), vec![medoo_rs::Value::Text("b".into())]),
        ("INSERT INTO t (n) VALUES (?)".to_string(), vec![medoo_rs::Value::Text("a".into())]), // duplica UNIQUE
    ];
    let _ = pool.execute_batch_raw(&stmts).await; // falla en la 3ra
    let rows = pool.fetch_all(&pool.select("t")).await.unwrap();
    assert_eq!(rows.len(), 2); // las 2 primeras persistieron
}

#[tokio::test]
async fn update_returning_real_data() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE u (id INTEGER PRIMARY KEY AUTOINCREMENT, n TEXT)", vec![]).await.unwrap();
    pool.execute(&pool.insert("u").set(record!{ "n" => "ana" })).await.unwrap();

    let q = pool.update("u").set("n", "ANA").where_eq("id", 1).returning(vec!["id", "n"]);
    let row = pool.fetch_one(&q).await.unwrap();
    assert_eq!(row.get_i64("id"), Some(1));
    assert_eq!(row.get_str("n"), Some("ANA"));
}

#[tokio::test]
async fn delete_returning_real_data() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE u (id INTEGER PRIMARY KEY AUTOINCREMENT, n TEXT)", vec![]).await.unwrap();
    pool.execute(&pool.insert("u").set(record!{ "n" => "x" }).set(record!{ "n" => "y" })).await.unwrap();

    let q = pool.delete("u").where_eq("n", "x").returning(vec!["id"]);
    let rows = pool.fetch_all(&q).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_i64("id"), Some(1));
}

#[tokio::test]
async fn scalar_subquery_real_data() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE o (id INTEGER PRIMARY KEY, amount INTEGER)", vec![]).await.unwrap();
    pool.execute_raw("INSERT INTO o (amount) VALUES (10), (50), (100), (200)", vec![]).await.unwrap();

    let avg = pool.select("o").columns(vec!["AVG(amount)"]);
    let rows = pool.fetch_all(&pool.select("o").where_scalar("amount", ">", avg)).await.unwrap();
    // promedio = 90; mayores: 100 y 200
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn driver_error_includes_sql_and_params() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    let err = pool.execute_raw(
        "INSERT INTO no_existe (x) VALUES (?)",
        vec![medoo_rs::Value::Int(42)]
    ).await.unwrap_err();
    let s = match err { QueryError::Driver(s) => s, _ => panic!("esperaba Driver") };
    assert!(s.contains("sql:"), "msg = {}", s);
    assert!(s.contains("INSERT INTO no_existe"), "msg = {}", s);
    assert!(s.contains("Int(42)"), "msg = {}", s);
}

#[tokio::test]
async fn upsert_sqlite_real() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw(
        "CREATE TABLE u (email TEXT PRIMARY KEY, name TEXT)",
        vec![]
    ).await.unwrap();
    pool.execute(&pool.insert("u").set(record!{ "email" => "x@y.cl", "name" => "Original" })).await.unwrap();

    let n = pool.execute(
        &pool.insert("u")
            .set(record!{ "email" => "x@y.cl", "name" => "Actualizado" })
            .on_conflict(vec!["email"])
            .do_update(vec!["name"])
    ).await.unwrap();
    assert_eq!(n, 1);

    let row = pool.fetch_one(&pool.select("u").where_eq("email", "x@y.cl")).await.unwrap();
    assert_eq!(row.get_str("name"), Some("Actualizado"));
}

#[tokio::test]
async fn cte_with_real_data() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, amount INTEGER)", vec![]).await.unwrap();
    pool.execute_raw("INSERT INTO orders (user_id, amount) VALUES (1, 200), (1, 50), (2, 500)", vec![]).await.unwrap();

    let big = pool.select("orders").columns(vec!["user_id"]).where_op("amount", ">", 100);
    let q = pool.select("big_buyers").with("big_buyers", big);
    let rows = pool.fetch_all(&q).await.unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn pool_options_with_max_connections() {
    let opts = PoolOptions { max_connections: 3, min_connections: 1, ..Default::default() };
    let pool = Pool::connect_sqlite_with("sqlite::memory:", opts).await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY)", vec![]).await.unwrap();
    let n = pool.execute_raw("INSERT INTO t (id) VALUES (1)", vec![]).await.unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn closure_transaction_commits_on_ok() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();

    pool.transaction(|tx| Box::pin(async move {
        tx.execute(&tx.db().insert("t").set(record!{ "n" => "a" })).await?;
        tx.execute(&tx.db().insert("t").set(record!{ "n" => "b" })).await?;
        Ok::<_, QueryError>(())
    })).await.unwrap();

    let rows = pool.fetch_all(&pool.select("t")).await.unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn closure_transaction_rolls_back_on_err() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();

    let res: Result<(), QueryError> = pool.transaction(|tx| Box::pin(async move {
        tx.execute(&tx.db().insert("t").set(record!{ "n" => "se cancela" })).await?;
        Err(QueryError::Driver("forzado".into()))
    })).await;
    assert!(res.is_err());

    let rows = pool.fetch_all(&pool.select("t")).await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn execute_many_atomic() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)", vec![]).await.unwrap();
    let qs = vec![
        pool.insert("t").set(record!{ "n" => "a" }),
        pool.insert("t").set(record!{ "n" => "b" }),
        pool.insert("t").set(record!{ "n" => "c" }),
    ];
    let total = pool.execute_many(&qs).await.unwrap();
    assert_eq!(total, 3);
    assert_eq!(pool.fetch_all(&pool.select("t")).await.unwrap().len(), 3);
}

struct User {
    id: i64,
    name: String,
}

impl FromRow for User {
    fn from_row(r: &Row) -> Result<Self, QueryError> {
        Ok(Self {
            id: r.get_i64("id").ok_or_else(|| QueryError::Driver("col id missing".into()))?,
            name: r.get_str("name").unwrap_or_default().to_string(),
        })
    }
}

#[tokio::test]
async fn fetch_all_as_maps_to_struct() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", vec![]).await.unwrap();
    pool.execute(&pool.insert("users").set(record!{ "name" => "ana" }).set(record!{ "name" => "luis" })).await.unwrap();

    let users: Vec<User> = pool.fetch_all_as(&pool.select("users").order_asc("id")).await.unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].id, 1);
    assert_eq!(users[0].name, "ana");
    assert_eq!(users[1].name, "luis");
}

#[tokio::test]
async fn fetch_one_as_and_optional_as() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", vec![]).await.unwrap();
    pool.execute(&pool.insert("users").set(record!{ "name" => "ana" })).await.unwrap();

    let one: User = pool.fetch_one_as(&pool.select("users").where_eq("id", 1)).await.unwrap();
    assert_eq!(one.name, "ana");

    let opt: Option<User> = pool.fetch_optional_as(&pool.select("users").where_eq("id", 999)).await.unwrap();
    assert!(opt.is_none());
}

#[tokio::test]
async fn insert_returning_works_in_sqlite() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT)", vec![]).await.unwrap();

    let q = pool.insert("users").set(record!{ "name" => "ana" }).returning(vec!["id"]);
    let row = pool.fetch_one(&q).await.unwrap();
    assert_eq!(row.get_i64("id"), Some(1));
}

#[tokio::test]
async fn subquery_with_real_data() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", vec![]).await.unwrap();
    pool.execute_raw("CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, amount INTEGER)", vec![]).await.unwrap();
    pool.execute(&pool.insert("users").set(record!{ "name" => "ana" }).set(record!{ "name" => "luis" })).await.unwrap();
    pool.execute(&pool.insert("orders").set(record!{ "user_id" => 1, "amount" => 200 })).await.unwrap();

    let sub = pool.select("orders").columns(vec!["user_id"]).where_op("amount", ">", 100);
    let rows = pool.fetch_all(&pool.select("users").where_in_subquery("id", sub)).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_str("name"), Some("ana"));
}

#[tokio::test]
async fn pool_propagates_sql_errors_as_driver() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    let err = pool.execute_raw("SELECT * FROM tabla_que_no_existe", vec![]).await.unwrap_err();
    assert!(matches!(err, medoo_rs::QueryError::Driver(_)));
}
