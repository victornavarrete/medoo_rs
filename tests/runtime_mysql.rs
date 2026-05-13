//! Integración real contra MySQL local. Requiere la feature
//! `runtime-mysql` y un servidor accesible. Por defecto:
//!   mysql://root@127.0.0.1:3306/example_medoo_db
//! Override con `MEDOO_MYSQL_URL`. La BD se crea/se limpia en el setup.
//!
//! Para correr:
//!   cargo test --features runtime-mysql --test runtime_mysql -- --test-threads=1

#![cfg(feature = "runtime-mysql")]

use medoo_rs::runtime::{Pool, PoolOptions, RowExt};
use medoo_rs::{record, Cond, QueryError, Value};
use std::time::Duration;

const DEFAULT_URL: &str = "mysql://root@127.0.0.1:3306/example_medoo_db";
const ADMIN_URL_DEFAULT: &str = "mysql://root@127.0.0.1:3306";
const DB_NAME: &str = "example_medoo_db";

fn db_url() -> String {
    std::env::var("MEDOO_MYSQL_URL").unwrap_or_else(|_| DEFAULT_URL.to_string())
}
fn admin_url() -> String {
    std::env::var("MEDOO_MYSQL_ADMIN_URL").unwrap_or_else(|_| ADMIN_URL_DEFAULT.to_string())
}

/// Crea/reinicia la BD y carga el esquema desde `examples/blog_schema.sql`.
/// Devuelve un Pool conectado a la BD lista para usar.
async fn fresh_pool() -> Pool {
    // 1. Pool admin (sin db) -> CREATE DATABASE.
    let admin = Pool::connect_mysql(&admin_url()).await
        .expect("no se pudo conectar al servidor MySQL");
    admin
        .execute_raw(&format!("DROP DATABASE IF EXISTS `{}`", DB_NAME), vec![])
        .await
        .unwrap();
    admin
        .execute_raw(
            &format!(
                "CREATE DATABASE `{}` DEFAULT CHARSET utf8mb4 COLLATE utf8mb4_unicode_ci",
                DB_NAME
            ),
            vec![],
        )
        .await
        .unwrap();

    // 2. Pool sobre la nueva BD + esquema.
    let opts = PoolOptions {
        max_connections: 8,
        min_connections: 1,
        acquire_timeout: Duration::from_secs(15),
        idle_timeout: Some(Duration::from_secs(300)),
        max_lifetime: Some(Duration::from_secs(900)),
    };
    let pool = Pool::connect_mysql_with(&db_url(), opts).await.unwrap();

    let schema = include_str!("../examples/blog_schema.sql");
    // El archivo trae CREATE DATABASE + USE + DROP/CREATE; aplicamos solo
    // las sentencias relevantes a la BD ya seleccionada.
    for stmt in schema.split(';') {
        let s = stmt.trim();
        if s.is_empty() || s.starts_with("--") {
            continue;
        }
        // Saltamos CREATE DATABASE y USE; estamos ya en la BD correcta.
        let up = s.to_uppercase();
        if up.starts_with("CREATE DATABASE") || up.starts_with("USE ") {
            continue;
        }
        pool.execute_raw(s, vec![]).await
            .unwrap_or_else(|e| panic!("DDL falló:\n{}\n→ {:?}", s, e));
    }
    pool
}

#[tokio::test]
async fn ping_and_schema() {
    let pool = fresh_pool().await;
    pool.ping().await.unwrap();
    let rows = pool
        .fetch_all_raw("SHOW TABLES", vec![])
        .await
        .unwrap();
    assert_eq!(rows.len(), 6, "esperaba 6 tablas, obtuvo {}", rows.len());
}

#[tokio::test]
async fn insert_select_basic() {
    let pool = fresh_pool().await;

    let n = pool
        .execute(&pool.insert("users")
            .set(record!{ "email" => "ana@x.com",  "name" => "Ana",  "bio" => "rust dev",          "active" => true  })
            .set(record!{ "email" => "luis@x.com", "name" => "Luis", "bio" => "backend",           "active" => true  })
            .set(record!{ "email" => "mara@x.com", "name" => "Mara", "bio" => None::<&str>,        "active" => false })
        )
        .await
        .unwrap();
    assert_eq!(n, 3);

    let rows = pool
        .fetch_all(&pool.select("users").order_asc("id"))
        .await
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].get_str("email"), Some("ana@x.com"));
    // MySQL TINYINT(1) -> Value::Bool en el row map.
    assert_eq!(rows[2].get_bool("active"), Some(false));
}

#[tokio::test]
async fn upsert_on_duplicate_key() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users").set(record!{
        "email" => "ana@x.com", "name" => "Ana"
    })).await.unwrap();

    // segundo intento con misma email -> on_conflict do_update
    let n = pool.execute(&pool.insert("users")
        .set(record!{ "email" => "ana@x.com", "name" => "Ana 2" })
        .on_conflict(vec!["email"])
        .do_update(vec!["name"])
    ).await.unwrap();
    // ON DUPLICATE KEY UPDATE devuelve 2 cuando actualiza, 1 cuando inserta.
    assert!(n == 1 || n == 2);

    let row = pool.fetch_one(
        &pool.select("users").where_eq("email", "ana@x.com")
    ).await.unwrap();
    assert_eq!(row.get_str("name"), Some("Ana 2"));
}

#[tokio::test]
async fn json_operations() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users")
        .set(record!{ "email" => "a@x", "name" => "A", "meta" => Value::json(r#"{"role":"admin","score":90}"#) })
        .set(record!{ "email" => "b@x", "name" => "B", "meta" => Value::json(r#"{"role":"user", "score":50}"#) })
        .set(record!{ "email" => "c@x", "name" => "C", "meta" => Value::json(r#"{"role":"admin","score":30}"#) })
    ).await.unwrap();

    let admins = pool.fetch_all(
        &pool.select("users").where_json("meta", "$.role", "=", "admin").order_asc("id")
    ).await.unwrap();
    assert_eq!(admins.len(), 2);

    let top = pool.fetch_all(
        &pool.select("users")
            .where_json("meta", "$.role",  "=",  "admin")
            .where_json("meta", "$.score", ">=", 50)
    ).await.unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].get_str("email"), Some("a@x"));

    let needle = Value::json(r#"{"role":"admin"}"#);
    let any_admin = pool.fetch_all(
        &pool.select("users").where_json_contains("meta", needle)
    ).await.unwrap();
    assert_eq!(any_admin.len(), 2);
}

#[tokio::test]
async fn joins_and_aggregates() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users")
        .set(record!{ "email" => "a@x", "name" => "Ana"  })
        .set(record!{ "email" => "b@x", "name" => "Beto" })
    ).await.unwrap();
    pool.execute(&pool.insert("posts")
        .set(record!{ "user_id" => 1i64, "title" => "Hola",   "slug" => "hola",   "body" => "1", "published" => true  })
        .set(record!{ "user_id" => 1i64, "title" => "Mundo",  "slug" => "mundo",  "body" => "2", "published" => true  })
        .set(record!{ "user_id" => 2i64, "title" => "Borrador","slug"=> "draft",  "body" => "3", "published" => false })
    ).await.unwrap();

    let rows = pool.fetch_all(
        &pool.select("users")
            .columns(vec!["users.name AS autor", "COUNT(posts.id) AS n"])
            .left_join("posts", "users.id = posts.user_id")
            .group_by("users.id")
            .order_asc("users.id")
    ).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get_str("autor"), Some("Ana"));
    assert_eq!(rows[0].get_i64("n"), Some(2));
    assert_eq!(rows[1].get_i64("n"), Some(1));

    // EXISTS subquery: solo el user 1 tiene posts publicados.
    let sub = pool.select("posts").columns(vec!["posts.id"]).where_raw("posts.user_id = users.id", vec![]).where_eq("published", true);
    let authors = pool.fetch_all(
        &pool.select("users").where_exists(sub).order_asc("id")
    ).await.unwrap();
    assert_eq!(authors.len(), 1);
    assert_eq!(authors[0].get_str("name"), Some("Ana"));
}

#[tokio::test]
async fn update_join_mysql() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users")
        .set(record!{ "email" => "a@x", "name" => "Ana", "active" => true  })
        .set(record!{ "email" => "b@x", "name" => "Bob", "active" => false })
    ).await.unwrap();
    pool.execute(&pool.insert("posts")
        .set(record!{ "user_id" => 1i64, "title" => "P1", "slug" => "p1", "body" => "x", "published" => true })
        .set(record!{ "user_id" => 2i64, "title" => "P2", "slug" => "p2", "body" => "x", "published" => true })
    ).await.unwrap();

    // Despublica posts cuyos autores estén inactivos (feature 3: UPDATE JOIN).
    let n = pool.execute(
        &pool.update("posts")
            .set("published", false)
            .using_join("users", "posts.user_id = users.id")
            .where_eq("users.active", false)
    ).await.unwrap();
    assert_eq!(n, 1);

    let row = pool.fetch_one(&pool.select("posts").where_eq("slug", "p2")).await.unwrap();
    assert_eq!(row.get_bool("published"), Some(false));
    let row = pool.fetch_one(&pool.select("posts").where_eq("slug", "p1")).await.unwrap();
    assert_eq!(row.get_bool("published"), Some(true));
}

#[tokio::test]
async fn delete_join_mysql() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users")
        .set(record!{ "email" => "a@x", "name" => "Ana", "active" => true  })
        .set(record!{ "email" => "b@x", "name" => "Bob", "active" => false })
    ).await.unwrap();
    pool.execute(&pool.insert("posts")
        .set(record!{ "user_id" => 1i64, "title" => "P1", "slug" => "p1", "body" => "x" })
        .set(record!{ "user_id" => 2i64, "title" => "P2", "slug" => "p2", "body" => "x" })
    ).await.unwrap();
    pool.execute(&pool.insert("comments")
        .set(record!{ "post_id" => 1i64, "user_id" => 1i64, "body" => "c-ana"  })
        .set(record!{ "post_id" => 2i64, "user_id" => 2i64, "body" => "c-bob1" })
        .set(record!{ "post_id" => 2i64, "user_id" => 2i64, "body" => "c-bob2" })
    ).await.unwrap();

    // Borra comentarios de usuarios inactivos via JOIN.
    let n = pool.execute(
        &pool.delete("comments")
            .using_join("users", "comments.user_id = users.id")
            .where_eq("users.active", false)
    ).await.unwrap();
    assert_eq!(n, 2);

    let rest = pool.fetch_all(&pool.select("comments")).await.unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].get_str("body"), Some("c-ana"));
}

#[tokio::test]
async fn raw_template_clauses() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users")
        .set(record!{ "email" => "a@x", "name" => "Ana"  })
        .set(record!{ "email" => "b@x", "name" => "Bob"  })
        .set(record!{ "email" => "c@x", "name" => "Caro" })
    ).await.unwrap();

    let q = pool.db()
        .raw_template(
            "SELECT u.id, u.name FROM users u {{WHERE}} {{ORDER}} {{LIMIT}}"
        )
        .where_op("u.name", "~", "%a%")
        .order_asc("u.name")
        .limit(10);

    let (sql, _) = q.to_sql().unwrap();
    assert!(sql.contains("LIKE"));
    let rows = pool.fetch_all(&q).await.unwrap();
    // Ana, Bob (no), Caro -> Ana, Caro
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get_str("name"), Some("Ana"));
    assert_eq!(rows[1].get_str("name"), Some("Caro"));
}

#[tokio::test]
async fn transactions_commit_and_rollback() {
    let pool = fresh_pool().await;

    // commit
    pool.transaction(|tx| Box::pin(async move {
        tx.execute(&tx.db().insert("users").set(record!{ "email" => "a@x", "name" => "A" })).await?;
        tx.execute(&tx.db().insert("users").set(record!{ "email" => "b@x", "name" => "B" })).await?;
        Ok::<_, QueryError>(())
    })).await.unwrap();
    let rows = pool.fetch_all(&pool.select("users")).await.unwrap();
    assert_eq!(rows.len(), 2);

    // rollback explícito
    let _ = pool.transaction(|tx| Box::pin(async move {
        tx.execute(&tx.db().insert("users").set(record!{ "email" => "c@x", "name" => "C" })).await?;
        Err::<(), _>(QueryError::Driver("forzando rollback".into()))
    })).await;
    let rows = pool.fetch_all(&pool.select("users")).await.unwrap();
    assert_eq!(rows.len(), 2, "el rollback debió revertir el INSERT de C");
}

#[tokio::test]
async fn savepoints() {
    let pool = fresh_pool().await;
    let mut tx = pool.begin().await.unwrap();
    tx.execute(&tx.db().insert("users").set(record!{ "email" => "a@x", "name" => "A" })).await.unwrap();
    tx.savepoint("sp1").await.unwrap();
    tx.execute(&tx.db().insert("users").set(record!{ "email" => "b@x", "name" => "B" })).await.unwrap();
    tx.rollback_to_savepoint("sp1").await.unwrap();
    tx.commit().await.unwrap();

    let rows = pool.fetch_all(&pool.select("users")).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_str("email"), Some("a@x"));
}

#[tokio::test]
async fn streaming_for_each() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users").set(record!{ "email" => "u@x", "name" => "U" })).await.unwrap();
    let mut ins = pool.insert("posts");
    for i in 0..50 {
        ins = ins.set(record!{
            "user_id" => 1i64,
            "title"   => format!("t{}", i),
            "slug"    => format!("s{}", i),
            "body"    => "x"
        });
    }
    pool.execute(&ins).await.unwrap();

    let mut count = 0usize;
    pool.for_each_row(&pool.select("posts").order_asc("id"), |row| {
        if row.get_str("slug").is_some() { count += 1; }
        Ok(())
    }).await.unwrap();
    assert_eq!(count, 50);
}

#[tokio::test]
async fn bulk_execute_batch() {
    let pool = fresh_pool().await;
    let qs = vec![
        pool.insert("users").set(record!{ "email" => "a@x", "name" => "A" }),
        pool.insert("users").set(record!{ "email" => "b@x", "name" => "B" }),
        pool.insert("users").set(record!{ "email" => "c@x", "name" => "C" }),
    ];
    let res = pool.execute_batch(&qs).await.unwrap();
    assert_eq!(res, vec![1u64, 1, 1]);
    let rows = pool.fetch_all(&pool.select("users")).await.unwrap();
    assert_eq!(rows.len(), 3);
}

#[tokio::test]
async fn explain_runs() {
    let pool = fresh_pool().await;
    let q = pool.select("users").where_eq("email", "x@x");
    let plan = pool.explain(&q).await.unwrap();
    assert!(!plan.is_empty(), "EXPLAIN debió devolver al menos una fila");
}

#[tokio::test]
async fn cascade_delete_attachments() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users").set(record!{ "email" => "a@x", "name" => "A" })).await.unwrap();
    pool.execute(&pool.insert("posts")
        .set(record!{ "user_id" => 1i64, "title" => "P", "slug" => "p", "body" => "x" })
    ).await.unwrap();
    pool.execute(&pool.insert("attachments")
        .set(record!{ "post_id" => 1i64, "filename" => "a.png", "mime" => "image/png", "size_bytes" => 100i64 })
        .set(record!{ "post_id" => 1i64, "filename" => "b.pdf", "mime" => "application/pdf", "size_bytes" => 200i64 })
    ).await.unwrap();

    pool.execute(&pool.delete("posts").where_eq("id", 1i64)).await.unwrap();
    let left = pool.fetch_all(&pool.select("attachments")).await.unwrap();
    assert_eq!(left.len(), 0, "FK ON DELETE CASCADE debió limpiar attachments");
}

#[tokio::test]
async fn where_in_subquery() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users")
        .set(record!{ "email" => "a@x", "name" => "A", "active" => true  })
        .set(record!{ "email" => "b@x", "name" => "B", "active" => false })
    ).await.unwrap();
    pool.execute(&pool.insert("posts")
        .set(record!{ "user_id" => 1i64, "title" => "P1", "slug" => "p1", "body" => "x" })
        .set(record!{ "user_id" => 2i64, "title" => "P2", "slug" => "p2", "body" => "x" })
    ).await.unwrap();

    let active_ids = pool.select("users").columns(vec!["id"]).where_eq("active", true);
    let rows = pool.fetch_all(
        &pool.select("posts").where_in_subquery("user_id", active_ids).order_asc("id")
    ).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_str("slug"), Some("p1"));
}

#[tokio::test]
async fn or_where_and_not_null() {
    let pool = fresh_pool().await;
    pool.execute(&pool.insert("users")
        .set(record!{ "email" => "a@x", "name" => "A", "bio" => "rusty"          })
        .set(record!{ "email" => "b@x", "name" => "B", "bio" => None::<&str>     })
        .set(record!{ "email" => "c@x", "name" => "C", "bio" => "go fan"          })
    ).await.unwrap();

    let rows = pool.fetch_all(
        &pool.select("users")
            .where_cond(Cond::Or(vec![
                Cond::op("name", "LIKE", "A%").unwrap(),
                Cond::op("name", "LIKE", "C%").unwrap(),
            ]))
            .where_not_null("bio")
            .order_asc("id")
    ).await.unwrap();
    assert_eq!(rows.len(), 2);
}
