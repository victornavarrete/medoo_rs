use medoo_rs::{record, Backend, Db, QueryError, Value};

#[test]
fn insert_single_row() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db
        .insert("users")
        .set(record!{ "name" => "ana", "age" => 30 })
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"INSERT INTO "users" ("name", "age") VALUES ($1, $2)"#
    );
    assert_eq!(params, vec![Value::Text("ana".into()), Value::Int(30)]);
}

#[test]
fn insert_many_rows_share_columns() {
    let db = Db::new(Backend::Sqlite);
    let (sql, params) = db
        .insert("users")
        .set(record!{ "name" => "ana", "age" => 30 })
        .set(record!{ "name" => "luis", "age" => 25 })
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"INSERT INTO "users" ("name", "age") VALUES (?, ?), (?, ?)"#
    );
    assert_eq!(params.len(), 4);
}

#[test]
fn insert_empty_errors() {
    let db = Db::new(Backend::Postgres);
    let err = db.insert("users").to_sql().unwrap_err();
    assert_eq!(err, QueryError::EmptyRecord);
}

#[test]
fn update_requires_where_by_default() {
    let db = Db::new(Backend::Postgres);
    let err = db.update("users").set("name", "x").to_sql().unwrap_err();
    assert_eq!(err, QueryError::MissingWhere("UPDATE"));
}

#[test]
fn update_full_table_with_explicit_opt_in() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db
        .update("users")
        .set("active", false)
        .allow_full_table()
        .to_sql()
        .unwrap();
    assert_eq!(sql, r#"UPDATE "users" SET "active" = $1"#);
}

#[test]
fn update_with_where() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db
        .update("users")
        .set("name", "ana")
        .set("age", 31)
        .where_eq("id", 7)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"UPDATE "users" SET "name" = $1, "age" = $2 WHERE "id" = $3"#
    );
    assert_eq!(params.len(), 3);
}

#[test]
fn delete_requires_where_by_default() {
    let db = Db::new(Backend::Postgres);
    let err = db.delete("sessions").to_sql().unwrap_err();
    assert_eq!(err, QueryError::MissingWhere("DELETE"));
}

#[test]
fn update_returning_postgres() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.update("users")
        .set("name", "Ana")
        .where_eq("id", 1)
        .returning(vec!["id", "updated_at"])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"UPDATE "users" SET "name" = $1 WHERE "id" = $2 RETURNING "id", "updated_at""#
    );
}

#[test]
fn delete_returning_star() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.delete("sessions")
        .where_op("expires_at", "<", "2026-01-01")
        .returning(vec!["*"])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"DELETE FROM "sessions" WHERE "expires_at" < $1 RETURNING *"#
    );
}

#[test]
fn upsert_postgres_do_update() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.insert("users")
        .set(record!{ "email" => "x@y.cl", "name" => "Ana" })
        .on_conflict(vec!["email"])
        .do_update(vec!["name"])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"INSERT INTO "users" ("email", "name") VALUES ($1, $2) ON CONFLICT ("email") DO UPDATE SET "name" = EXCLUDED."name""#
    );
}

#[test]
fn upsert_postgres_do_nothing() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.insert("users")
        .set(record!{ "email" => "x@y.cl" })
        .on_conflict(vec!["email"])
        .do_nothing()
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"INSERT INTO "users" ("email") VALUES ($1) ON CONFLICT ("email") DO NOTHING"#
    );
}

#[test]
fn upsert_mysql_on_duplicate_key() {
    let db = Db::new(Backend::MySql);
    let (sql, _) = db.insert("users")
        .set(record!{ "email" => "x@y.cl", "name" => "Ana" })
        .do_update(vec!["name"])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "INSERT INTO `users` (`email`, `name`) VALUES (?, ?) ON DUPLICATE KEY UPDATE `name` = VALUES(`name`)"
    );
}

#[test]
fn upsert_mysql_insert_ignore() {
    let db = Db::new(Backend::MySql);
    let (sql, _) = db.insert("users")
        .set(record!{ "email" => "x@y.cl" })
        .do_nothing()
        .to_sql()
        .unwrap();
    assert_eq!(sql, "INSERT IGNORE INTO `users` (`email`) VALUES (?)");
}

#[test]
fn upsert_sqlite_compound_conflict() {
    let db = Db::new(Backend::Sqlite);
    let (sql, _) = db.insert("memberships")
        .set(record!{ "user_id" => 1, "group_id" => 2, "rol" => "admin" })
        .on_conflict(vec!["user_id", "group_id"])
        .do_update(vec!["rol"])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"INSERT INTO "memberships" ("user_id", "group_id", "rol") VALUES (?, ?, ?) ON CONFLICT ("user_id", "group_id") DO UPDATE SET "rol" = EXCLUDED."rol""#
    );
}

#[test]
fn upsert_pg_do_update_without_conflict_fails() {
    let db = Db::new(Backend::Postgres);
    let err = db.insert("users")
        .set(record!{ "email" => "x" })
        .do_update(vec!["name"])
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, medoo_rs::QueryError::InvalidIdentifier(_)));
}

#[test]
fn upsert_with_returning() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.insert("users")
        .set(record!{ "email" => "x@y.cl", "name" => "Ana" })
        .on_conflict(vec!["email"])
        .do_update(vec!["name"])
        .returning(vec!["id"])
        .to_sql()
        .unwrap();
    assert!(sql.contains("ON CONFLICT"));
    assert!(sql.ends_with(r#"RETURNING "id""#));
}

#[test]
fn insert_returning_postgres() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.insert("users")
        .set(record!{ "name" => "ana" })
        .returning(vec!["id", "created_at"])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"INSERT INTO "users" ("name") VALUES ($1) RETURNING "id", "created_at""#
    );
}

#[test]
fn insert_returning_star() {
    let db = Db::new(Backend::Sqlite);
    let (sql, _) = db.insert("users")
        .set(record!{ "name" => "ana" })
        .returning(vec!["*"])
        .to_sql()
        .unwrap();
    assert_eq!(sql, r#"INSERT INTO "users" ("name") VALUES (?) RETURNING *"#);
}

#[test]
fn insert_json_value_for_mysql() {
    let db = Db::new(Backend::MySql);
    let payload = Value::json(r#"{"role":"admin","tags":["a","b"]}"#);
    let (sql, params) = db
        .insert("users")
        .set(record!{ "name" => "ana", "meta" => payload.clone() })
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "INSERT INTO `users` (`name`, `meta`) VALUES (?, ?)"
    );
    assert_eq!(params[1], payload);
}

#[test]
fn delete_with_op() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db
        .delete("sessions")
        .where_op("expires_at", "<", "2026-01-01")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"DELETE FROM "sessions" WHERE "expires_at" < $1"#
    );
    assert_eq!(params.len(), 1);
}
