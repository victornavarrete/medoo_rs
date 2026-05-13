//! Tests del proc-macro #[derive(FromRow)]. Requiere features derive
//! + runtime-sqlite (para tener la trait FromRow disponible).
//!   cargo test --features "derive runtime-sqlite" --test derive_from_row

#![cfg(all(feature = "derive", feature = "runtime-sqlite"))]

use medoo_rs::runtime::Pool;
use medoo_rs::{record, FromRow};

#[derive(FromRow)]
struct User {
    id: i64,
    name: String,
    email: Option<String>,
    active: bool,
    score: i32,
}

#[tokio::test]
async fn derive_basic_struct() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT, active INTEGER, score INTEGER)",
        vec![],
    ).await.unwrap();
    pool.execute(&pool.insert("users").set(record!{
        "name" => "Ana", "email" => "ana@x.cl", "active" => true, "score" => 42_i32
    })).await.unwrap();

    let u: User = pool.fetch_one_as(&pool.select("users").where_eq("id", 1)).await.unwrap();
    assert_eq!(u.id, 1);
    assert_eq!(u.name, "Ana");
    assert_eq!(u.email.as_deref(), Some("ana@x.cl"));
    assert!(u.active);
    assert_eq!(u.score, 42);
}

#[derive(FromRow)]
struct WithNulls {
    id: i64,
    nick: Option<String>,
    age: Option<i32>,
}

#[tokio::test]
async fn derive_handles_null_columns() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, nick TEXT, age INTEGER)", vec![]).await.unwrap();
    let nullstr: Option<&str> = None;
    let nullint: Option<i32> = None;
    pool.execute(&pool.insert("t").set(record!{ "nick" => nullstr, "age" => nullint })).await.unwrap();

    let r: WithNulls = pool.fetch_one_as(&pool.select("t").where_eq("id", 1)).await.unwrap();
    assert_eq!(r.id, 1);
    assert!(r.nick.is_none());
    assert!(r.age.is_none());
}

#[derive(FromRow)]
struct Renamed {
    #[medoo(rename = "user_id")]
    id: i64,
    #[medoo(rename = "user_name")]
    name: String,
}

#[tokio::test]
async fn derive_rename_attribute() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (user_id INTEGER PRIMARY KEY, user_name TEXT)", vec![]).await.unwrap();
    pool.execute_raw("INSERT INTO t (user_id, user_name) VALUES (5, 'Luis')", vec![]).await.unwrap();

    let r: Renamed = pool.fetch_one_as(&pool.select("t").where_eq("user_id", 5)).await.unwrap();
    assert_eq!(r.id, 5);
    assert_eq!(r.name, "Luis");
}

#[derive(FromRow)]
struct Sized {
    a: i8,
    b: i16,
    c: u32,
    d: u64,
}

#[tokio::test]
async fn derive_int_size_variants() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (a INTEGER, b INTEGER, c INTEGER, d INTEGER)", vec![]).await.unwrap();
    pool.execute_raw("INSERT INTO t VALUES (1, 200, 3000, 4000000)", vec![]).await.unwrap();

    let r: Sized = pool.fetch_one_as(&pool.select("t")).await.unwrap();
    assert_eq!(r.a, 1);
    assert_eq!(r.b, 200);
    assert_eq!(r.c, 3000);
    assert_eq!(r.d, 4000000);
}

#[derive(FromRow)]
struct Floats {
    x: f64,
    y: Option<f32>,
}

#[tokio::test]
async fn derive_float_variants() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (x REAL, y REAL)", vec![]).await.unwrap();
    pool.execute_raw("INSERT INTO t VALUES (3.14, 2.71)", vec![]).await.unwrap();

    let r: Floats = pool.fetch_one_as(&pool.select("t")).await.unwrap();
    assert!((r.x - 3.14).abs() < 1e-9);
    assert!((r.y.unwrap() - 2.71).abs() < 1e-3);
}

#[derive(FromRow)]
struct UserMin {
    id: i64,
    name: String,
}

#[tokio::test]
async fn derive_fetch_all_as_collection() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE u (id INTEGER PRIMARY KEY, name TEXT)", vec![]).await.unwrap();
    pool.execute(&pool.insert("u")
        .set(record!{ "name" => "a" })
        .set(record!{ "name" => "b" })
        .set(record!{ "name" => "c" })
    ).await.unwrap();

    let xs: Vec<UserMin> = pool.fetch_all_as(&pool.select("u").order_asc("id")).await.unwrap();
    assert_eq!(xs.len(), 3);
    assert_eq!(xs[0].name, "a");
    assert_eq!(xs[2].name, "c");
}

#[tokio::test]
async fn derive_required_field_missing_errors() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY)", vec![]).await.unwrap();
    pool.execute_raw("INSERT INTO t VALUES (1)", vec![]).await.unwrap();

    // UserMin requiere `name`, que no existe en esta tabla
    let r: medoo_rs::Result<UserMin> = pool.fetch_one_as(&pool.select("t")).await;
    assert!(r.is_err(), "esperaba error por columna name faltante");
}
