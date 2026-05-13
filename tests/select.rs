use medoo_rs::{Backend, Cond, Db, OrderDir, Value};

#[test]
fn select_simple_postgres() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db.select("users").to_sql().unwrap();
    assert_eq!(sql, r#"SELECT * FROM "users""#);
    assert!(params.is_empty());
}

#[test]
fn select_columns_with_alias() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db
        .select("users")
        .columns(vec!["id", "name AS nombre", "COUNT(*)"])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT "id", "name" AS "nombre", COUNT(*) FROM "users""#
    );
}

#[test]
fn select_dynamic_filters_postgres() {
    let db = Db::new(Backend::Postgres);
    // Replica el "Objetivo Espiritual" del RUTA.md.
    let mut q = db.select("users");
    let status: Option<&str> = Some("active");
    let min_age: Option<i32> = Some(18);
    let recent = true;
    let limit: Option<u64> = Some(20);
    if let Some(s) = status {
        q = q.where_eq("status", s);
    }
    if let Some(m) = min_age {
        q = q.where_op("age", ">", m);
    }
    if recent {
        q = q.order_desc("created_at");
    }
    if let Some(n) = limit {
        q = q.limit(n);
    }
    let (sql, params) = q.to_sql().unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" WHERE "status" = $1 AND "age" > $2 ORDER BY "created_at" DESC LIMIT 20"#
    );
    assert_eq!(params, vec![Value::Text("active".into()), Value::Int(18)]);
}

#[test]
fn select_dynamic_filters_mysql_uses_qmark() {
    let db = Db::new(Backend::MySql);
    let (sql, _) = db
        .select("users")
        .where_eq("status", "active")
        .where_op("age", "[>=]", 18)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM `users` WHERE `status` = ? AND `age` >= ?"
    );
}

#[test]
fn between_cols_inclusive() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db.select("eventos")
        .where_between_cols("hoy", "fecha_inicio", "fecha_fin")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "eventos" WHERE "hoy" BETWEEN "fecha_inicio" AND "fecha_fin""#
    );
    assert!(params.is_empty());
}

#[test]
fn between_cols_exclusive_upper() {
    let db = Db::new(Backend::MySql);
    let (sql, _) = db.select("ventanas")
        .where_between_cols_with("ahora", "desde", "hasta", true, false)
        .to_sql()
        .unwrap();
    // inclusive_lo=true && inclusive_hi=false → >= y <
    assert_eq!(
        sql,
        "SELECT * FROM `ventanas` WHERE (`ahora` >= `desde` AND `ahora` < `hasta`)"
    );
}

#[test]
fn value_in_range_inclusive() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db.select("promos")
        .where_value_in_range("2026-05-01", "fecha_desde", "fecha_hasta")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "promos" WHERE ("fecha_desde" <= $1 AND $2 <= "fecha_hasta")"#
    );
    assert_eq!(params.len(), 2);
    assert_eq!(params[0], params[1]);
}

#[test]
fn value_in_range_exclusive_upper() {
    let db = Db::new(Backend::Sqlite);
    let (sql, _) = db.select("turnos")
        .where_value_in_range_with("12:00", "hora_inicio", "hora_fin", true, false)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "turnos" WHERE ("hora_inicio" <= ? AND ? < "hora_fin")"#
    );
}

#[test]
fn like_helpers() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db.select("u")
        .where_like("nombre", "Ana%")
        .where_not_like("apellido", "Test%")
        .where_starts_with("email", "ana@")
        .where_ends_with("dominio", ".cl")
        .where_contains("notas", "urgente")
        .to_sql()
        .unwrap();
    assert!(sql.contains(r#""nombre" LIKE $1"#));
    assert!(sql.contains(r#""apellido" NOT LIKE $2"#));
    assert!(sql.contains(r#""email" LIKE $3"#));
    assert_eq!(params[0], medoo_rs::Value::Text("Ana%".into()));
    assert_eq!(params[2], medoo_rs::Value::Text("ana@%".into()));
    assert_eq!(params[3], medoo_rs::Value::Text("%.cl".into()));
    assert_eq!(params[4], medoo_rs::Value::Text("%urgente%".into()));
}

#[test]
fn like_escapes_special_chars() {
    let db = Db::new(Backend::Postgres);
    let (_, params) = db.select("u")
        .where_starts_with("nick", "100%_off")
        .to_sql()
        .unwrap();
    // El `%` y `_` del input deben quedar escapados con `\`
    assert_eq!(params[0], medoo_rs::Value::Text("100\\%\\_off%".into()));
}

#[test]
fn try_where_op_returns_result_no_panic() {
    let db = Db::new(Backend::Postgres);
    let r = db.select("u").try_where_op("age", "INVALID_OP", 18);
    assert!(r.is_err());
}

#[test]
fn try_where_op_chains_ok() {
    let db = Db::new(Backend::Postgres);
    let q = db.select("u").try_where_op("age", ">", 18).unwrap()
                          .try_where_eq("active", true).unwrap();
    let (sql, _) = q.to_sql().unwrap();
    assert!(sql.contains(r#""age" > $1"#));
}

#[test]
fn null_guard_rejects_non_eq_operators() {
    let db = Db::new(Backend::Postgres);
    let v: Option<i32> = None;
    let err = db.select("u").where_op("age", ">", v).to_sql().unwrap_err();
    assert!(matches!(err, medoo_rs::QueryError::InvalidOperator(_)));
    if let medoo_rs::QueryError::InvalidOperator(s) = err {
        assert!(s.contains("NULL"));
    }
}

#[test]
fn null_with_eq_still_emits_is_null() {
    let db = Db::new(Backend::Postgres);
    let v: Option<i32> = None;
    let (sql, params) = db.select("u").where_op("age", "=", v).to_sql().unwrap();
    assert_eq!(sql, r#"SELECT * FROM "u" WHERE "age" IS NULL"#);
    assert!(params.is_empty());
}

#[test]
fn double_equals_alias_for_eq() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.select("u").where_op("id", "==", 1).to_sql().unwrap();
    assert!(sql.contains(r#""id" = $1"#));
}

#[test]
fn operator_parse_case_insensitive() {
    let db = Db::new(Backend::Postgres);
    for op in ["LIKE", "like", "Like", "lIkE"] {
        let (sql, _) = db.select("u").where_op("name", op, "Ana%").to_sql().unwrap();
        assert!(sql.contains("LIKE"));
    }
    for op in ["NOT LIKE", "not like", "Not Like"] {
        let (sql, _) = db.select("u").where_op("name", op, "Ana%").to_sql().unwrap();
        assert!(sql.contains("NOT LIKE"));
    }
    for op in ["ILIKE", "ilike", "Ilike"] {
        let (sql, _) = db.select("u").where_op("name", op, "Ana%").to_sql().unwrap();
        assert!(sql.contains("ILIKE"));
    }
}

#[test]
fn ilike_via_where_op_postgres_native() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.select("u").where_op("name", "ILIKE", "ana%").to_sql().unwrap();
    assert_eq!(sql, r#"SELECT * FROM "u" WHERE "name" ILIKE $1"#);
}

#[test]
fn ilike_via_where_op_mysql_emulated() {
    let db = Db::new(Backend::MySql);
    let (sql, _) = db.select("u").where_op("name", "ILIKE", "ana%").to_sql().unwrap();
    assert_eq!(sql, "SELECT * FROM `u` WHERE LOWER(`name`) LIKE LOWER(?)");
}

#[test]
fn ilike_short_form_tilde_star() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.select("u").where_op("name", "~*", "ana%").to_sql().unwrap();
    assert!(sql.contains("ILIKE"));
}

#[test]
fn ilike_postgres_native() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.select("u").where_ilike("name", "ana%").to_sql().unwrap();
    assert!(sql.contains("ILIKE"));
}

#[test]
fn ilike_emulated_in_mysql() {
    let db = Db::new(Backend::MySql);
    let (sql, _) = db.select("u").where_ilike("name", "ana%").to_sql().unwrap();
    assert!(sql.contains("LOWER(`name`) LIKE LOWER(?)"));
}

#[test]
fn explain_wraps_query() {
    let db = Db::new(Backend::Postgres);
    let q = db.select("u").where_eq("id", 1);
    let (sql, params) = db.explain(&q).unwrap();
    assert!(sql.starts_with("EXPLAIN SELECT"));
    assert_eq!(params.len(), 1);
}

#[test]
fn explain_analyze_per_backend() {
    let q_pg = Db::new(Backend::Postgres).select("u");
    let q_my = Db::new(Backend::MySql).select("u");
    let q_sq = Db::new(Backend::Sqlite).select("u");

    let (sql, _) = Db::new(Backend::Postgres).explain_analyze(&q_pg).unwrap();
    assert!(sql.starts_with("EXPLAIN ANALYZE"));

    let (sql, _) = Db::new(Backend::MySql).explain_analyze(&q_my).unwrap();
    assert!(sql.starts_with("EXPLAIN ANALYZE"));

    let (sql, _) = Db::new(Backend::Sqlite).explain_analyze(&q_sq).unwrap();
    assert!(sql.starts_with("EXPLAIN QUERY PLAN"));
}

#[test]
fn select_in_list_and_between() {
    let db = Db::new(Backend::Sqlite);
    let (sql, params) = db
        .select("orders")
        .where_in("status", vec!["paid", "shipped"])
        .where_between("amount", 10, 100)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "orders" WHERE "status" IN (?, ?) AND "amount" BETWEEN ? AND ?"#
    );
    assert_eq!(params.len(), 4);
}

#[test]
fn select_or_group() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db
        .select("users")
        .where_eq("active", true)
        .or_where(vec![
            Cond::eq("role", "admin"),
            Cond::eq("role", "owner"),
        ])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" WHERE "active" = $1 AND ("role" = $2 OR "role" = $3)"#
    );
    assert_eq!(params.len(), 3);
}

#[test]
fn select_null_handling_eq_null() {
    let db = Db::new(Backend::Postgres);
    let v: Option<&str> = None;
    let (sql, params) = db.select("users").where_eq("deleted_at", v).to_sql().unwrap();
    assert_eq!(sql, r#"SELECT * FROM "users" WHERE "deleted_at" IS NULL"#);
    assert!(params.is_empty());
}

#[test]
fn select_join_validates_identifiers() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db
        .select("users")
        .left_join("orders", "users.id = orders.user_id")
        .where_op("orders.amount", ">", 0)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" LEFT JOIN "orders" ON "users"."id" = "orders"."user_id" WHERE "orders"."amount" > $1"#
    );
}

#[test]
fn select_raw_where_rebinds_placeholders() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db
        .select("logs")
        .where_eq("level", "error")
        .where_raw("created_at > ? AND created_at < ?", vec![
            Value::Text("2026-01-01".into()),
            Value::Text("2026-12-31".into()),
        ])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "logs" WHERE "level" = $1 AND created_at > $2 AND created_at < $3"#
    );
    assert_eq!(params.len(), 3);
}

#[test]
fn cross_join_simple() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db.select("a").cross_join("b").to_sql().unwrap();
    assert_eq!(sql, r#"SELECT * FROM "a" CROSS JOIN "b""#);
}

#[test]
fn left_join_lateral_postgres() {
    let db = Db::new(Backend::Postgres);
    let sub = db.select("orders")
        .columns(vec!["id", "amount"])
        .where_raw("orders.user_id = u.id", vec![])
        .order_desc("amount")
        .limit(3);
    let (sql, _) = db.select("users")
        .columns(vec!["users.id"])
        .left_join_lateral(sub, "u", "true")
        .to_sql()
        .unwrap();
    assert!(sql.contains(r#"LEFT JOIN LATERAL (SELECT "id", "amount" FROM "orders""#));
    assert!(sql.contains(r#") AS "u" ON"#));
}

#[test]
fn cross_join_lateral_no_on() {
    let db = Db::new(Backend::Postgres);
    let sub = db.select("generate_series(1, 10) AS gs");
    // Acepta el "table" tipo función — pero validate() lo rechazaría.
    // Mejor con tabla simple:
    let sub = db.select("nums").columns(vec!["n"]);
    let (sql, _) = db.select("u")
        .cross_join_lateral(sub, "n")
        .to_sql()
        .unwrap();
    assert!(sql.contains(r#"CROSS JOIN LATERAL (SELECT "n" FROM "nums") AS "n""#));
    assert!(!sql.contains(" ON "));
}

#[test]
fn lateral_merges_placeholders_with_outer() {
    let db = Db::new(Backend::Postgres);
    let sub = db.select("orders")
        .columns(vec!["amount"])
        .where_op("amount", ">", 100);
    let (sql, params) = db.select("users")
        .where_eq("active", true)
        .left_join_lateral(sub, "o", "true")
        .to_sql()
        .unwrap();
    // JOINs se renderean antes del WHERE outer:
    //   subquery LATERAL toma $1 (amount > 100)
    //   outer WHERE toma $2 (active = true)
    assert!(sql.contains(r#""amount" > $1"#), "got {}", sql);
    assert!(sql.contains(r#""active" = $2"#), "got {}", sql);
    assert_eq!(params.len(), 2);
}

#[test]
fn cte_simple_with_clause() {
    let db = Db::new(Backend::Postgres);
    let inner = db.select("orders").columns(vec!["user_id"]).where_op("amount", ">", 100);
    let (sql, params) = db.select("big")
        .with("big", inner)
        .where_eq("user_id", 5)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"WITH "big" AS (SELECT "user_id" FROM "orders" WHERE "amount" > $1) SELECT * FROM "big" WHERE "user_id" = $2"#
    );
    assert_eq!(params.len(), 2);
}

#[test]
fn cte_multiple_clauses() {
    let db = Db::new(Backend::Sqlite);
    let a = db.select("users").columns(vec!["id"]).where_eq("active", true);
    let b = db.select("orders").columns(vec!["user_id", "amount"]).where_op("amount", ">", 50);
    let (sql, _) = db.select("a")
        .with("a", a)
        .with("b", b)
        .to_sql()
        .unwrap();
    assert!(sql.starts_with(r#"WITH "a" AS ("#));
    assert!(sql.contains(r#""b" AS ("#));
    assert!(sql.contains(r#") SELECT * FROM "a""#));
}

#[test]
fn cte_recursive_flag() {
    let db = Db::new(Backend::Postgres);
    let base = db.select("tree").columns(vec!["id", "parent"]);
    let (sql, _) = db.select("ancestors")
        .with("ancestors", base)
        .with_recursive_flag()
        .to_sql()
        .unwrap();
    assert!(sql.starts_with("WITH RECURSIVE "));
}

#[test]
fn scalar_subquery_in_where() {
    let db = Db::new(Backend::Postgres);
    let avg = db.select("orders").columns(vec!["AVG(amount)"]);
    let (sql, params) = db.select("orders")
        .where_scalar("amount", ">", avg)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "orders" WHERE "amount" > (SELECT AVG(amount) FROM "orders")"#
    );
    assert!(params.is_empty());
}

#[test]
fn scalar_subquery_with_outer_params() {
    let db = Db::new(Backend::Sqlite);
    let max_user = db.select("orders")
        .columns(vec!["MAX(amount)"])
        .where_eq("user_id", 5);
    let (sql, params) = db.select("orders")
        .where_eq("status", "paid")
        .where_scalar("amount", "=", max_user)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "orders" WHERE "status" = ? AND "amount" = (SELECT MAX(amount) FROM "orders" WHERE "user_id" = ?)"#
    );
    assert_eq!(params.len(), 2);
}

#[test]
fn select_in_subquery_merges_placeholders() {
    let db = Db::new(Backend::Postgres);
    let sub = db.select("orders")
        .columns(vec!["user_id"])
        .where_op("amount", ">", 100);
    let (sql, params) = db.select("users")
        .where_eq("active", true)
        .where_in_subquery("id", sub)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" WHERE "active" = $1 AND "id" IN (SELECT "user_id" FROM "orders" WHERE "amount" > $2)"#
    );
    assert_eq!(params.len(), 2);
}

#[test]
fn select_not_in_subquery() {
    let db = Db::new(Backend::MySql);
    let sub = db.select("ban").columns(vec!["user_id"]);
    let (sql, _) = db.select("users")
        .where_not_in_subquery("id", sub)
        .to_sql()
        .unwrap();
    assert_eq!(sql, "SELECT * FROM `users` WHERE `id` NOT IN (SELECT `user_id` FROM `ban`)");
}

#[test]
fn select_exists_and_not_exists() {
    let db = Db::new(Backend::Postgres);
    // Subquery correlacionada: usamos where_raw porque la igualdad es
    // entre dos columnas, no col=valor.
    let sub = db.select("orders")
        .columns(vec!["id"])
        .where_raw("orders.user_id = users.id", vec![]);
    let (sql, _) = db.select("users")
        .where_exists(sub)
        .to_sql()
        .unwrap();
    assert!(sql.contains("EXISTS (SELECT"), "got {}", sql);
    assert!(sql.contains("orders.user_id = users.id"));

    // not_exists
    let sub = db.select("ban").columns(vec!["id"]).where_raw("ban.user_id = users.id", vec![]);
    let (sql, _) = db.select("users").where_not_exists(sub).to_sql().unwrap();
    assert!(sql.contains("NOT EXISTS (SELECT"), "got {}", sql);
}

#[test]
fn select_subquery_combined_with_regular_filters() {
    let db = Db::new(Backend::Sqlite);
    let inner = db.select("orders")
        .columns(vec!["user_id"])
        .where_in("status", vec!["paid", "shipped"]);
    let (sql, params) = db.select("users")
        .where_eq("active", true)
        .where_in_subquery("id", inner)
        .where_op("age", ">=", 18)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" WHERE "active" = ? AND "id" IN (SELECT "user_id" FROM "orders" WHERE "status" IN (?, ?)) AND "age" >= ?"#
    );
    assert_eq!(params.len(), 4);
}

#[test]
fn select_group_having_order_limit_offset() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db
        .select("orders")
        .columns(vec!["status", "COUNT(*) AS n"])
        .group_by("status")
        .having(Cond::op("status", "<>", "draft").unwrap())
        .order_by("status", OrderDir::Asc)
        .limit(10)
        .offset(20)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT "status", COUNT(*) AS "n" FROM "orders" GROUP BY "status" HAVING "status" <> $1 ORDER BY "status" ASC LIMIT 10 OFFSET 20"#
    );
}
