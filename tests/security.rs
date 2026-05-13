//! Tests de seguridad: ningún punto de entrada debe permitir SQL injection.

use medoo_rs::{Backend, Db, QueryError, Value};

#[test]
fn rejects_injection_in_table_name() {
    let db = Db::new(Backend::Postgres);
    let err = db.select("users; DROP TABLE users;--").to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn rejects_injection_in_column_name() {
    let db = Db::new(Backend::Postgres);
    let err = db
        .select("users")
        .where_eq("id\"; DROP TABLE users;--", 1)
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn rejects_injection_in_order_by() {
    let db = Db::new(Backend::Postgres);
    let err = db.select("users").order_asc("1; DROP--").to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn rejects_invalid_operator_only_via_runtime_op() {
    use medoo_rs::Cond;
    let err = Cond::op("age", "OR 1=1", 1).unwrap_err();
    assert!(matches!(err, QueryError::InvalidOperator(_)));
}

#[test]
fn user_value_with_quotes_goes_as_param_not_inline() {
    // Si el usuario manda un string malicioso como valor, NUNCA debe acabar
    // en el SQL: solo en params como Value::Text.
    let db = Db::new(Backend::Postgres);
    let evil = "x'; DROP TABLE users;--";
    let (sql, params) = db.select("users").where_eq("name", evil).to_sql().unwrap();
    assert_eq!(sql, r#"SELECT * FROM "users" WHERE "name" = $1"#);
    assert!(!sql.contains("DROP"));
    assert_eq!(params, vec![Value::Text(evil.to_string())]);
}

#[test]
fn raw_where_bind_mismatch_detected() {
    let db = Db::new(Backend::Postgres);
    let err = db
        .select("t")
        .where_raw("a = ? AND b = ?", vec![Value::Int(1)])
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, QueryError::BindMismatch { .. }));
}

#[test]
fn empty_in_list_is_error_not_silent_truthy() {
    let db = Db::new(Backend::Postgres);
    let empty: Vec<i64> = vec![];
    let err = db.select("users").where_in("id", empty).to_sql().unwrap_err();
    assert!(matches!(err, QueryError::EmptyInList(_)));
}

#[test]
fn join_on_rejects_non_identifier_chars() {
    let db = Db::new(Backend::Postgres);
    let err = db
        .select("users")
        .left_join("orders", "users.id = orders.user_id OR 1=1")
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn raw_column_blocks_semicolons() {
    let db = Db::new(Backend::Postgres);
    let err = db
        .select("users")
        .columns(vec!["COUNT(*); DROP TABLE users--"])
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}
