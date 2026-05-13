use medoo_rs::{record, where_, Backend, Db, Value};

#[test]
fn record_macro_builds_pairs() {
    let r = record!{ "a" => 1, "b" => "x", "c" => true };
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].0, "a");
    assert_eq!(r[0].1, Value::Int(1));
    assert_eq!(r[1].1, Value::Text("x".into()));
    assert_eq!(r[2].1, Value::Bool(true));
}

#[test]
fn where_macro_basic_eq_and_op() {
    let db = Db::new(Backend::Postgres);
    let cond = where_!{
        "status" => "active",
        "age" => [">", 18],
    };
    let (sql, params) = db.select("users").where_cond(cond).to_sql().unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" WHERE ("status" = $1 AND "age" > $2)"#
    );
    assert_eq!(params, vec![Value::Text("active".into()), Value::Int(18)]);
}

#[test]
fn where_macro_null_helpers() {
    let db = Db::new(Backend::Postgres);
    let cond = where_!{ "deleted_at" => null, "name" => not_null };
    let (sql, _) = db.select("users").where_cond(cond).to_sql().unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "users" WHERE ("deleted_at" IS NULL AND "name" IS NOT NULL)"#
    );
}
