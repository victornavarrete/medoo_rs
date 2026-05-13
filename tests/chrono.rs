//! Tests del feature `chrono`. Requiere features chrono + runtime-sqlite.
//!   cargo test --features "chrono runtime-sqlite" --test chrono

#![cfg(all(feature = "chrono", feature = "runtime-sqlite"))]

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use medoo_rs::runtime::{Pool, RowExt, RowExtChrono};
use medoo_rs::record;

#[tokio::test]
async fn datetime_round_trip_utc() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE logs (id INTEGER PRIMARY KEY, ts TEXT)", vec![]).await.unwrap();
    let when = Utc.with_ymd_and_hms(2026, 5, 1, 14, 30, 0).unwrap();
    pool.execute(&pool.insert("logs").set(record!{ "ts" => when })).await.unwrap();

    let row = pool.fetch_one(&pool.select("logs").where_eq("id", 1)).await.unwrap();
    let s = row.get_str("ts").unwrap();
    assert!(s.contains("2026-05-01"), "got {}", s);

    let parsed = row.get_datetime_utc("ts").unwrap();
    assert_eq!(parsed, when);
}

#[tokio::test]
async fn naive_datetime_round_trip() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, agendado TEXT)", vec![]).await.unwrap();
    let dt = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()
        .and_hms_opt(9, 0, 0).unwrap();
    pool.execute(&pool.insert("t").set(record!{ "agendado" => dt })).await.unwrap();
    let row = pool.fetch_one(&pool.select("t").where_eq("id", 1)).await.unwrap();
    let parsed = row.get_naive_datetime("agendado").unwrap();
    assert_eq!(parsed, dt);
}

#[tokio::test]
async fn date_only_round_trip() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, d TEXT)", vec![]).await.unwrap();
    let d = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
    pool.execute(&pool.insert("t").set(record!{ "d" => d })).await.unwrap();
    let row = pool.fetch_one(&pool.select("t").where_eq("id", 1)).await.unwrap();
    assert_eq!(row.get_date("d"), Some(d));
}

#[tokio::test]
async fn time_round_trip() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE t (id INTEGER PRIMARY KEY, h TEXT)", vec![]).await.unwrap();
    let h = NaiveTime::from_hms_opt(14, 30, 45).unwrap();
    pool.execute(&pool.insert("t").set(record!{ "h" => h })).await.unwrap();
    let row = pool.fetch_one(&pool.select("t").where_eq("id", 1)).await.unwrap();
    assert_eq!(row.get_time("h"), Some(h));
}

#[tokio::test]
async fn between_dates_with_chrono() {
    let pool = Pool::connect_sqlite("sqlite::memory:").await.unwrap();
    pool.execute_raw("CREATE TABLE eventos (id INTEGER PRIMARY KEY, fecha TEXT)", vec![]).await.unwrap();
    pool.execute_raw(
        "INSERT INTO eventos (fecha) VALUES ('2026-04-15'), ('2026-05-10'), ('2026-06-01')",
        vec![]
    ).await.unwrap();

    let inicio = NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
    let fin    = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let rows = pool.fetch_all(
        &pool.select("eventos").where_between("fecha", inicio, fin)
    ).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_str("fecha"), Some("2026-05-10"));
}
