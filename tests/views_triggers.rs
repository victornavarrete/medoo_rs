use medoo_rs::{Backend, Db, QueryError};

// VIEWS

#[test]
fn create_view_basic_postgres() {
    let db = Db::new(Backend::Postgres);
    let q = db.select("users").where_eq("active", true);
    let (sql, params) = db.create_view("users_activos").as_select(q).to_sql().unwrap();
    assert_eq!(
        sql,
        r#"CREATE VIEW "users_activos" AS SELECT * FROM "users" WHERE "active" = $1"#
    );
    assert_eq!(params.len(), 1);
}

#[test]
fn create_or_replace_view_mysql() {
    let db = Db::new(Backend::MySql);
    let q = db.select("users").where_op("age", ">", 18);
    let (sql, _) = db.create_view("adultos").or_replace().as_select(q).to_sql().unwrap();
    assert!(sql.starts_with("CREATE OR REPLACE VIEW `adultos`"));
}

#[test]
fn create_view_if_not_exists_sqlite() {
    let db = Db::new(Backend::Sqlite);
    let q = db.select("u");
    let (sql, _) = db.create_view("v").if_not_exists().as_select(q).to_sql().unwrap();
    assert!(sql.starts_with(r#"CREATE VIEW IF NOT EXISTS "v""#));
}

#[test]
fn create_view_with_column_aliases() {
    let db = Db::new(Backend::Postgres);
    let q = db.select("u").columns(vec!["id", "name AS nombre"]);
    let (sql, _) = db.create_view("v")
        .columns(vec!["uid", "nombre_completo"])
        .as_select(q)
        .to_sql()
        .unwrap();
    assert!(sql.contains(r#""v" ("uid", "nombre_completo")"#));
}

#[test]
fn materialized_view_postgres() {
    let db = Db::new(Backend::Postgres);
    let q = db.select("o").columns(vec!["user_id", "SUM(amount) AS total"]).group_by("user_id");
    let (sql, _) = db.create_view("totales_por_user").materialized().as_select(q).to_sql().unwrap();
    assert!(sql.starts_with(r#"CREATE MATERIALIZED VIEW "totales_por_user""#));
}

#[test]
fn materialized_view_rejected_in_mysql() {
    let db = Db::new(Backend::MySql);
    let q = db.select("u");
    let err = db.create_view("v").materialized().as_select(q).to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidOperator(_)));
}

#[test]
fn drop_view_with_options() {
    let db = Db::new(Backend::Postgres);
    let sql = db.drop_view("v").if_exists().cascade().to_sql().unwrap();
    assert_eq!(sql, r#"DROP VIEW IF EXISTS "v" CASCADE"#);
}

#[test]
fn drop_materialized_view_pg() {
    let db = Db::new(Backend::Postgres);
    let sql = db.drop_view("m").materialized().if_exists().to_sql().unwrap();
    assert_eq!(sql, r#"DROP MATERIALIZED VIEW IF EXISTS "m""#);
}

#[test]
fn view_subquery_placeholders_merge() {
    let db = Db::new(Backend::Postgres);
    let q = db.select("orders")
        .where_eq("status", "paid")
        .where_op("amount", ">", 100);
    let (sql, params) = db.create_view("pedidos_grandes").as_select(q).to_sql().unwrap();
    assert!(sql.contains("$1"));
    assert!(sql.contains("$2"));
    assert_eq!(params.len(), 2);
}

// TRIGGERS

#[test]
fn create_trigger_mysql_after_insert() {
    let db = Db::new(Backend::MySql);
    let sql = db.create_trigger("audit_users")
        .after().insert().on_table("users")
        .body("INSERT INTO log (msg) VALUES ('user added')")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "CREATE TRIGGER `audit_users` AFTER INSERT ON `users` FOR EACH ROW BEGIN INSERT INTO log (msg) VALUES ('user added'); END"
    );
}

#[test]
fn create_trigger_sqlite_with_when() {
    let db = Db::new(Backend::Sqlite);
    let sql = db.create_trigger("audit_high")
        .before().update().on_table("orders")
        .when("NEW.amount > 1000")
        .body("INSERT INTO alerts VALUES (NEW.id)")
        .to_sql()
        .unwrap();
    assert!(sql.contains("WHEN NEW.amount > 1000"));
    assert!(sql.contains("BEGIN INSERT INTO alerts"));
}

#[test]
fn create_trigger_postgres_uses_function() {
    let db = Db::new(Backend::Postgres);
    let sql = db.create_trigger("trg_audit")
        .after().delete().on_table("users")
        .execute_function("audit_users_fn")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"CREATE TRIGGER "trg_audit" AFTER DELETE ON "users" FOR EACH ROW EXECUTE FUNCTION "audit_users_fn"()"#
    );
}

#[test]
fn create_trigger_postgres_requires_function() {
    let db = Db::new(Backend::Postgres);
    let err = db.create_trigger("t").after().insert().on_table("u")
        .body("ignored")
        .to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn create_trigger_mysql_requires_body() {
    let db = Db::new(Backend::MySql);
    let err = db.create_trigger("t").after().insert().on_table("u").to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn drop_trigger_mysql_no_table() {
    let db = Db::new(Backend::MySql);
    let sql = db.drop_trigger("t").if_exists().to_sql().unwrap();
    assert_eq!(sql, "DROP TRIGGER IF EXISTS `t`");
}

#[test]
fn drop_trigger_postgres_requires_table() {
    let db = Db::new(Backend::Postgres);
    let err = db.drop_trigger("t").to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));

    let sql = db.drop_trigger("t").on_table("users").if_exists().to_sql().unwrap();
    assert_eq!(sql, r#"DROP TRIGGER IF EXISTS "t" ON "users""#);
}

#[test]
fn trigger_rejects_injection_in_name() {
    let db = Db::new(Backend::MySql);
    let err = db.create_trigger("t; DROP")
        .after().insert().on_table("u").body("x")
        .to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

// EVENTS

#[test]
fn create_event_mysql() {
    let db = Db::new(Backend::MySql);
    let sql = db.create_event("daily_cleanup")
        .schedule("EVERY 1 DAY")
        .body("DELETE FROM tmp WHERE created_at < NOW() - INTERVAL 30 DAY")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "CREATE EVENT `daily_cleanup` ON SCHEDULE EVERY 1 DAY DO DELETE FROM tmp WHERE created_at < NOW() - INTERVAL 30 DAY"
    );
}

#[test]
fn create_event_if_not_exists() {
    let db = Db::new(Backend::MySql);
    let sql = db.create_event("e")
        .if_not_exists()
        .schedule("EVERY 1 HOUR")
        .body("CALL refresh_cache()")
        .to_sql()
        .unwrap();
    assert!(sql.starts_with("CREATE EVENT IF NOT EXISTS"));
}

#[test]
fn create_event_pg_errors() {
    let db = Db::new(Backend::Postgres);
    let err = db.create_event("e").schedule("X").body("Y").to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidOperator(_)));
}

#[test]
fn drop_event_mysql() {
    let db = Db::new(Backend::MySql);
    let sql = db.drop_event("e").if_exists().to_sql().unwrap();
    assert_eq!(sql, "DROP EVENT IF EXISTS `e`");
}
