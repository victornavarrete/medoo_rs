use medoo_rs::{Backend, Db, JsonPath, QueryError, Value};

#[test]
fn json_path_parses_dotted_and_indices() {
    let p = JsonPath::parse("$.user.tags[0].name").unwrap();
    assert_eq!(p.render_mysql(), "$.user.tags[0].name");
    assert_eq!(p.render_postgres(), "{user,tags,0,name}");
}

#[test]
fn json_path_accepts_no_dollar_prefix() {
    let p = JsonPath::parse("user.id").unwrap();
    assert_eq!(p.render_mysql(), "$.user.id");
}

#[test]
fn json_path_rejects_injection() {
    for evil in ["$.a' OR 1=1--", "$.a;b", "$.a-b", "$.a.b[xx]", "$.", "$..a", ""] {
        let r = JsonPath::parse(evil);
        assert!(matches!(r, Err(QueryError::InvalidIdentifier(_))), "should reject: {:?}", evil);
    }
}

#[test]
fn where_json_mysql_uses_unquote_extract() {
    let db = Db::new(Backend::MySql);
    let (sql, params) = db
        .select("users")
        .where_json("meta", "$.role", "=", "admin")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM `users` WHERE JSON_UNQUOTE(JSON_EXTRACT(`meta`, '$.role')) = ?"
    );
    assert_eq!(params, vec![Value::Text("admin".into())]);
}

#[test]
fn where_json_postgres_uses_text_path_op() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db
        .select("users")
        .where_json("meta", "user.id", ">", 10)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" WHERE "meta" #>> '{user,id}' > $1"#
    );
}

#[test]
fn where_json_sqlite_uses_json_extract() {
    let db = Db::new(Backend::Sqlite);
    let (sql, _) = db
        .select("rows")
        .where_json("data", "$.tags[2]", "[~]", "%foo%")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "rows" WHERE json_extract("data", '$.tags[2]') LIKE ?"#
    );
}

#[test]
fn where_json_contains_mysql() {
    let db = Db::new(Backend::MySql);
    let needle = Value::json(r#"{"role":"admin"}"#);
    let (sql, params) = db
        .select("users")
        .where_json_contains("meta", needle.clone())
        .to_sql()
        .unwrap();
    assert_eq!(sql, "SELECT * FROM `users` WHERE JSON_CONTAINS(`meta`, ?)");
    assert_eq!(params, vec![needle]);
}

#[test]
fn where_json_contains_postgres_casts_jsonb() {
    let db = Db::new(Backend::Postgres);
    let (sql, _) = db
        .select("users")
        .where_json_contains("meta", Value::json(r#"{"role":"admin"}"#))
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" WHERE "meta" @> $1::jsonb"#
    );
}

#[test]
fn where_json_contains_sqlite_errors() {
    let db = Db::new(Backend::Sqlite);
    let err = db
        .select("t")
        .where_json_contains("meta", Value::json("{}"))
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, QueryError::InvalidOperator(_)));
}

#[test]
fn where_json_null_handling() {
    let db = Db::new(Backend::MySql);
    let v: Option<&str> = None;
    let (sql, params) = db
        .select("users")
        .where_json("meta", "$.deleted_at", "=", v)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM `users` WHERE JSON_UNQUOTE(JSON_EXTRACT(`meta`, '$.deleted_at')) IS NULL"
    );
    assert!(params.is_empty());
}

#[test]
fn where_json_combined_with_regular_filters() {
    let db = Db::new(Backend::Postgres);
    let (sql, params) = db
        .select("users")
        .where_eq("active", true)
        .where_json("meta", "$.role", "=", "admin")
        .where_json("meta", "$.score", ">=", 10)
        .order_desc("created_at")
        .limit(5)
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" WHERE "active" = $1 AND "meta" #>> '{role}' = $2 AND "meta" #>> '{score}' >= $3 ORDER BY "created_at" DESC LIMIT 5"#
    );
    assert_eq!(params.len(), 3);
}
