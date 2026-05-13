use medoo_rs::{tracking_table_sql, Backend, Migration, Migrator, QueryError};

fn fixture() -> Migrator {
    Migrator::new()
        .add(
            Migration::new(20260101, "create_users")
                .up("CREATE TABLE users (id BIGSERIAL PRIMARY KEY, email TEXT NOT NULL)")
                .down("DROP TABLE users"),
        )
        .add(
            Migration::new(20260201, "add_email_index")
                .up("CREATE UNIQUE INDEX users_email_idx ON users(email)")
                .down("DROP INDEX users_email_idx"),
        )
        .add(
            Migration::new(20260301, "add_meta")
                .up("ALTER TABLE users ADD COLUMN meta JSONB")
                .down("ALTER TABLE users DROP COLUMN meta"),
        )
}

#[test]
fn migrator_orders_by_version_even_when_added_unsorted() {
    let m = Migrator::new()
        .add(Migration::new(3, "c").up("c"))
        .add(Migration::new(1, "a").up("a"))
        .add(Migration::new(2, "b").up("b"));
    let ordered = m.ordered().unwrap();
    let versions: Vec<i64> = ordered.iter().map(|m| m.version).collect();
    assert_eq!(versions, vec![1, 2, 3]);
}

#[test]
fn migrator_detects_duplicate_versions() {
    let m = Migrator::new()
        .add(Migration::new(1, "a").up("a"))
        .add(Migration::new(1, "b").up("b"));
    assert!(matches!(m.ordered(), Err(QueryError::InvalidIdentifier(_))));
}

#[test]
fn pending_returns_only_unapplied_in_order() {
    let m = fixture();
    let applied = vec![20260101_i64];
    let pending = m.pending(&applied).unwrap();
    let versions: Vec<i64> = pending.iter().map(|m| m.version).collect();
    assert_eq!(versions, vec![20260201, 20260301]);
}

#[test]
fn pending_empty_when_all_applied() {
    let m = fixture();
    let applied = vec![20260101, 20260201, 20260301];
    assert!(m.pending(&applied).unwrap().is_empty());
}

#[test]
fn rollback_plan_descending_to_target() {
    let m = fixture();
    let applied = vec![20260101, 20260201, 20260301];
    let plan = m.rollback_plan(&applied, Some(20260101)).unwrap();
    let versions: Vec<i64> = plan.iter().map(|m| m.version).collect();
    assert_eq!(versions, vec![20260301, 20260201]);
}

#[test]
fn rollback_plan_full_undo_when_target_none() {
    let m = fixture();
    let applied = vec![20260101, 20260201];
    let plan = m.rollback_plan(&applied, None).unwrap();
    let versions: Vec<i64> = plan.iter().map(|m| m.version).collect();
    assert_eq!(versions, vec![20260201, 20260101]);
}

#[test]
fn tracking_table_sql_per_backend() {
    let pg = tracking_table_sql(Backend::Postgres).unwrap();
    assert!(pg.contains(r#""_medoo_migrations""#));
    assert!(pg.contains("TIMESTAMPTZ"));
    assert!(pg.contains("DEFAULT now()"));

    let my = tracking_table_sql(Backend::MySql).unwrap();
    assert!(my.contains("`_medoo_migrations`"));
    assert!(my.contains("DATETIME"));

    let sq = tracking_table_sql(Backend::Sqlite).unwrap();
    assert!(sq.contains(r#""_medoo_migrations""#));
    assert!(sq.contains("CURRENT_TIMESTAMP"));
}
