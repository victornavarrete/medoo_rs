use medoo_rs::{record, Backend, Db, LogCategory, Logger, Value};

fn read_buf(buf: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buf.lock().unwrap().clone()).unwrap()
}

#[test]
fn build_select_logs_under_read_category() {
    let (logger, buf) = Logger::buffer();
    let db = Db::new(Backend::Postgres).with_logger(logger);
    let q = db.select("users").where_eq("id", 1);
    let (_sql, _) = db.build(&q).unwrap();
    let out = read_buf(&buf);
    assert!(out.contains("[READ"), "got: {}", out);
    assert!(out.contains(r#"SELECT * FROM "users" WHERE "id" = $1"#));
    assert!(out.contains("Int(1)"));
}

#[test]
fn build_insert_logs_under_insert_category() {
    let (logger, buf) = Logger::buffer();
    let db = Db::new(Backend::Postgres).with_logger(logger);
    let q = db.insert("u").set(record!{ "name" => "ana" });
    db.build(&q).unwrap();
    let out = read_buf(&buf);
    assert!(out.contains("[INSERT"));
    assert!(out.contains("INSERT INTO"));
}

#[test]
fn build_update_and_delete_log_their_categories() {
    let (logger, buf) = Logger::buffer();
    let db = Db::new(Backend::Postgres).with_logger(logger);
    db.build(&db.update("u").set("name", "x").where_eq("id", 1)).unwrap();
    db.build(&db.delete("u").where_eq("id", 1)).unwrap();
    let out = read_buf(&buf);
    assert!(out.contains("[UPDATE"));
    assert!(out.contains("[DELETE"));
}

#[test]
fn filter_only_writes_drops_reads() {
    let (logger, buf) = Logger::buffer();
    let logger = logger.filter(LogCategory::WRITE); // INSERT|UPDATE|DELETE
    let db = Db::new(Backend::Postgres).with_logger(logger);
    db.build(&db.select("u").where_eq("id", 1)).unwrap(); // READ → descartado
    db.build(&db.insert("u").set(record!{ "x" => 1 })).unwrap();
    let out = read_buf(&buf);
    assert!(!out.contains("[READ"));
    assert!(out.contains("[INSERT"));
}

#[test]
fn filter_only_reads_drops_writes() {
    let (logger, buf) = Logger::buffer();
    let logger = logger.filter(LogCategory::READ);
    let db = Db::new(Backend::Postgres).with_logger(logger);
    db.build(&db.select("u").where_eq("id", 1)).unwrap();
    db.build(&db.insert("u").set(record!{ "x" => 1 })).unwrap();
    db.build(&db.update("u").set("x", 2).where_eq("id", 1)).unwrap();
    db.build(&db.delete("u").where_eq("id", 1)).unwrap();
    let out = read_buf(&buf);
    assert_eq!(out.matches("[READ").count(), 1);
    assert!(!out.contains("[INSERT"));
    assert!(!out.contains("[UPDATE"));
    assert!(!out.contains("[DELETE"));
}

#[test]
fn filter_combined_categories_with_bitor() {
    let (logger, buf) = Logger::buffer();
    let logger = logger.filter(LogCategory::READ | LogCategory::DELETE);
    let db = Db::new(Backend::Postgres).with_logger(logger);
    db.build(&db.select("u").where_eq("id", 1)).unwrap();
    db.build(&db.insert("u").set(record!{ "x" => 1 })).unwrap();
    db.build(&db.delete("u").where_eq("id", 1)).unwrap();
    let out = read_buf(&buf);
    assert!(out.contains("[READ"));
    assert!(!out.contains("[INSERT"));
    assert!(out.contains("[DELETE"));
}

#[test]
fn ddl_and_raw_use_explicit_helpers() {
    let (logger, buf) = Logger::buffer();
    let db = Db::new(Backend::Postgres).with_logger(logger);
    let ddl = db.create_table("t")
        .col(medoo_rs::ColDef::new("id", medoo_rs::ColType::Int).primary_key())
        .to_sql()
        .unwrap();
    db.log_ddl(&ddl);
    db.log_raw("SELECT now()", &[]);
    let out = read_buf(&buf);
    assert!(out.contains("[DDL"));
    assert!(out.contains("CREATE TABLE"));
    assert!(out.contains("[RAW"));
}

#[test]
fn no_logger_means_no_panic_no_output() {
    let db = Db::new(Backend::Postgres); // sin logger
    let q = db.select("u").where_eq("id", 1);
    db.build(&q).unwrap(); // no debe romper
    db.log_ddl("CREATE TABLE x (id INT)");
}

#[test]
fn file_logger_writes_to_disk_and_appends() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("medoo_test_{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let logger = Logger::file(&path).unwrap();
        let db = Db::new(Backend::Postgres).with_logger(logger);
        db.build(&db.select("u").where_eq("id", 1)).unwrap();
    }
    {
        // segunda sesión añade al mismo archivo
        let logger = Logger::file(&path).unwrap();
        let db = Db::new(Backend::Postgres).with_logger(logger);
        db.build(&db.insert("u").set(record!{ "n" => "ana" })).unwrap();
    }
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("[READ"));
    assert!(contents.contains("[INSERT"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn log_line_includes_params_when_present() {
    let (logger, buf) = Logger::buffer();
    let db = Db::new(Backend::Postgres).with_logger(logger);
    db.build(&db.select("u").where_eq("name", "ana").where_op("age", ">", 18)).unwrap();
    let out = read_buf(&buf);
    assert!(out.contains(r#"Text("ana")"#));
    assert!(out.contains("Int(18)"));
}

#[test]
fn category_label_is_correct() {
    assert_eq!(LogCategory::READ.label(), "READ");
    assert_eq!(LogCategory::INSERT.label(), "INSERT");
    assert_eq!(LogCategory::UPDATE.label(), "UPDATE");
    assert_eq!(LogCategory::DELETE.label(), "DELETE");
    assert_eq!(LogCategory::DDL.label(), "DDL");
    assert_eq!(LogCategory::RAW.label(), "RAW");
    let mixed = LogCategory::READ | LogCategory::INSERT;
    assert_eq!(mixed.label(), "MIXED");
}

#[test]
fn category_contains_semantics() {
    let mask = LogCategory::WRITE; // I|U|D
    assert!(mask.contains(LogCategory::INSERT));
    assert!(mask.contains(LogCategory::UPDATE));
    assert!(mask.contains(LogCategory::DELETE));
    assert!(!mask.contains(LogCategory::READ));
    assert!(LogCategory::ALL.contains(LogCategory::READ));
    assert!(!LogCategory::NONE.contains(LogCategory::READ));
}

// silenciar warnings si Value se importa pero no se usa en algún ámbito
#[allow(dead_code)]
fn _u(_: Value) {}
