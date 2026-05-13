use medoo_rs::{Backend, Db, Value};

#[test]
fn update_using_pg() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db
        .update("orders")
        .set("status", "shipped")
        .using_join("users", "orders.user_id = users.id")
        .where_eq("users.vip", true)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"UPDATE "orders" SET "status" = $1 FROM "users" WHERE "orders"."user_id" = "users"."id" AND "users"."vip" = $2"#
    );
    assert_eq!(params, vec![Value::Text("shipped".into()), Value::Bool(true)]);
}

#[test]
fn update_using_mysql() {
    let db = Db::new(Backend::MySql);
    let (sql, _params) = db
        .update("orders")
        .set("status", "shipped")
        .using_join("users", "orders.user_id = users.id")
        .where_eq("users.vip", true)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "UPDATE `orders` INNER JOIN `users` ON `orders`.`user_id` = `users`.`id` SET `status` = ? WHERE `users`.`vip` = ?"
    );
}

#[test]
fn update_using_sqlite() {
    let db = Db::new(Backend::Sqlite);
    let (sql, _) = db
        .update("orders")
        .set("status", "shipped")
        .using_join("users", "orders.user_id = users.id")
        .where_eq("users.vip", true)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"UPDATE "orders" SET "status" = ? FROM "users" WHERE "orders"."user_id" = "users"."id" AND "users"."vip" = ?"#
    );
}

#[test]
fn delete_using_pg() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db
        .delete("sessions")
        .using_join("users", "sessions.user_id = users.id")
        .where_eq("users.banned", true)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"DELETE FROM "sessions" USING "users" WHERE "sessions"."user_id" = "users"."id" AND "users"."banned" = $1"#
    );
}

#[test]
fn delete_using_mysql() {
    let db = Db::new(Backend::MySql);
    let (sql, _) = db
        .delete("sessions")
        .using_join("users", "sessions.user_id = users.id")
        .where_eq("users.banned", true)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "DELETE `sessions` FROM `sessions` INNER JOIN `users` ON `sessions`.`user_id` = `users`.`id` WHERE `users`.`banned` = ?"
    );
}

#[test]
fn delete_using_sqlite_errors() {
    let db = Db::new(Backend::Sqlite);
    let r = db
        .delete("sessions")
        .using_join("users", "sessions.user_id = users.id")
        .to_sql();
    assert!(r.is_err());
}

#[test]
fn update_using_mysql_requires_on() {
    let db = Db::new(Backend::MySql);
    let r = db.update("a").set("x", 1).using("b").to_sql();
    assert!(r.is_err());
}

#[test]
fn template_full() {
    let db = Db::new(Backend::Postgres);
    let q = db
        .raw_template(
            "SELECT u.id, u.name FROM users u {{WHERE}} {{ORDER}} {{LIMIT}} {{OFFSET}}",
        )
        .where_eq("u.status", "active")
        .where_op("u.age", ">=", 18)
        .order_desc("u.created_at")
        .limit(20)
        .offset(40);
    let (sql, params) = q.to_sql().unwrap();
    assert_eq!(
        sql,
        r#"SELECT u.id, u.name FROM users u WHERE "u"."status" = $1 AND "u"."age" >= $2 ORDER BY "u"."created_at" DESC LIMIT 20 OFFSET 40"#
    );
    assert_eq!(params.len(), 2);
}

#[test]
fn template_empty_clauses() {
    let db = Db::new(Backend::Sqlite);
    let q = db.raw_template("SELECT * FROM t {{WHERE}} {{ORDER}}");
    let (sql, params) = q.to_sql().unwrap();
    assert_eq!(sql, "SELECT * FROM t");
    assert!(params.is_empty());
}

#[test]
fn template_with_raw_placeholders() {
    let db = Db::new(Backend::Postgres);
    let q = db
        .raw_template("SELECT * FROM logs WHERE level = ? AND created_at > ? {{ORDER}}")
        .bind(vec![Value::Text("error".into()), Value::Text("2026-01-01".into())])
        .order_desc("created_at");
    let (sql, params) = q.to_sql().unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM logs WHERE level = $1 AND created_at > $2 ORDER BY "created_at" DESC"#
    );
    assert_eq!(params.len(), 2);
}

#[test]
fn template_mixed_placeholders_and_where() {
    let db = Db::new(Backend::Postgres);
    let q = db
        .raw_template("SELECT * FROM logs WHERE source = ? AND {{WHERE}}")
        .bind(vec![Value::Text("api".into())])
        .where_eq("level", "error");
    let (sql, params) = q.to_sql().unwrap();
    // ? consumes $1, then {{WHERE}} renders without leading WHERE keyword? No, it always emits "WHERE"
    // so result has "AND WHERE level..." which is invalid SQL — but that's user error.
    // Test exposes the mechanic: ? -> $1, where_eq -> $2.
    assert!(sql.contains("$1"));
    assert!(sql.contains("$2"));
    assert_eq!(params.len(), 2);
}

#[test]
fn template_unknown_token_errors() {
    let db = Db::new(Backend::Sqlite);
    let r = db.raw_template("SELECT * FROM t {{FOOBAR}}").to_sql();
    assert!(r.is_err());
}
