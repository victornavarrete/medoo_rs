use medoo_rs::{Backend, ColDef, ColType, Db, QueryError};

#[test]
fn create_table_postgres_with_serial() {
    let db = Db::new(Backend::Postgres);
    let sql = db
        .create_table("users")
        .if_not_exists()
        .col(ColDef::new("id", ColType::BigInt).primary_key().auto_increment())
        .col(ColDef::new("email", ColType::Varchar(255)).not_null().unique())
        .col(ColDef::new("active", ColType::Bool).not_null().default_raw("TRUE"))
        .col(ColDef::new("meta", ColType::Json))
        .col(ColDef::new("created_at", ColType::Timestamp).not_null().default_raw("now()"))
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"CREATE TABLE IF NOT EXISTS "users" ("id" BIGSERIAL PRIMARY KEY, "email" VARCHAR(255) NOT NULL UNIQUE, "active" BOOLEAN NOT NULL DEFAULT TRUE, "meta" JSONB, "created_at" TIMESTAMPTZ NOT NULL DEFAULT now())"#
    );
}

#[test]
fn create_table_mysql_auto_increment() {
    let db = Db::new(Backend::MySql);
    let sql = db
        .create_table("users")
        .col(ColDef::new("id", ColType::BigInt).primary_key().auto_increment())
        .col(ColDef::new("name", ColType::Text).not_null())
        .col(ColDef::new("data", ColType::Json))
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "CREATE TABLE `users` (`id` BIGINT PRIMARY KEY AUTO_INCREMENT, `name` TEXT NOT NULL, `data` JSON)"
    );
}

#[test]
fn create_table_sqlite_autoincrement_keyword() {
    let db = Db::new(Backend::Sqlite);
    let sql = db
        .create_table("users")
        .col(ColDef::new("id", ColType::Int).primary_key().auto_increment())
        .col(ColDef::new("name", ColType::Text).not_null())
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"CREATE TABLE "users" ("id" INTEGER PRIMARY KEY AUTOINCREMENT, "name" TEXT NOT NULL)"#
    );
}

#[test]
fn create_table_compound_primary_key() {
    let db = Db::new(Backend::Postgres);
    let sql = db
        .create_table("memberships")
        .col(ColDef::new("user_id", ColType::BigInt).not_null())
        .col(ColDef::new("group_id", ColType::BigInt).not_null())
        .primary_key(vec!["user_id", "group_id"])
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"CREATE TABLE "memberships" ("user_id" BIGINT NOT NULL, "group_id" BIGINT NOT NULL, PRIMARY KEY ("user_id", "group_id"))"#
    );
}

#[test]
fn char_and_varchar_render_correctly() {
    let db = Db::new(Backend::MySql);
    let sql = db
        .create_table("paises")
        .col(ColDef::new("iso2", ColType::Char(2)).not_null().unique())
        .col(ColDef::new("iso3", ColType::Char(3)).unique())
        .col(ColDef::new("nombre", ColType::Varchar(100)).not_null())
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "CREATE TABLE `paises` (`iso2` CHAR(2) NOT NULL UNIQUE, `iso3` CHAR(3) UNIQUE, `nombre` VARCHAR(100) NOT NULL)"
    );
}

#[test]
fn integer_size_variants() {
    let my = Db::new(Backend::MySql);
    let sql = my.create_table("t")
        .col(ColDef::new("a", ColType::TinyInt))
        .col(ColDef::new("b", ColType::SmallInt))
        .col(ColDef::new("c", ColType::Int))
        .col(ColDef::new("d", ColType::BigInt))
        .to_sql().unwrap();
    assert!(sql.contains("`a` TINYINT,"));
    assert!(sql.contains("`b` SMALLINT,"));
    assert!(sql.contains("`c` INTEGER,"));
    assert!(sql.contains("`d` BIGINT)"));

    let pg = Db::new(Backend::Postgres);
    let sql = pg.create_table("t")
        .col(ColDef::new("a", ColType::TinyInt))
        .col(ColDef::new("b", ColType::SmallInt))
        .to_sql().unwrap();
    assert!(sql.contains(r#""a" SMALLINT"#)); // PG no tiene TINYINT
    assert!(sql.contains(r#""b" SMALLINT"#));

    let sq = Db::new(Backend::Sqlite);
    let sql = sq.create_table("t")
        .col(ColDef::new("b", ColType::SmallInt))
        .col(ColDef::new("d", ColType::BigInt))
        .to_sql().unwrap();
    // SQLite: todo INTEGER affinity
    assert!(sql.contains(r#""b" INTEGER"#));
    assert!(sql.contains(r#""d" INTEGER"#));
}

#[test]
fn decimal_preserves_precision() {
    for backend in [Backend::Postgres, Backend::MySql, Backend::Sqlite] {
        let db = Db::new(backend);
        let sql = db.create_table("ventas")
            .col(ColDef::new("precio", ColType::Decimal(10, 2)).not_null())
            .to_sql().unwrap();
        assert!(sql.contains("DECIMAL(10,2)"), "{:?}: {}", backend, sql);
    }
}

#[test]
fn uuid_render_per_backend() {
    let pg = Db::new(Backend::Postgres);
    let sql = pg.create_table("t")
        .col(ColDef::new("id", ColType::Uuid).primary_key())
        .to_sql().unwrap();
    assert!(sql.contains(r#""id" UUID"#));

    for backend in [Backend::MySql, Backend::Sqlite] {
        let db = Db::new(backend);
        let sql = db.create_table("t")
            .col(ColDef::new("id", ColType::Uuid).primary_key())
            .to_sql().unwrap();
        assert!(sql.contains("CHAR(36)"), "{:?}: {}", backend, sql);
    }
}

#[test]
fn text_size_variants_mysql() {
    let my = Db::new(Backend::MySql);
    let sql = my.create_table("docs")
        .col(ColDef::new("a", ColType::TinyText))
        .col(ColDef::new("b", ColType::Text))
        .col(ColDef::new("c", ColType::MediumText))
        .col(ColDef::new("d", ColType::LongText))
        .to_sql().unwrap();
    assert!(sql.contains("`a` TINYTEXT"));
    assert!(sql.contains("`b` TEXT"));
    assert!(sql.contains("`c` MEDIUMTEXT"));
    assert!(sql.contains("`d` LONGTEXT"));

    // PG/SQLite: todo TEXT
    for backend in [Backend::Postgres, Backend::Sqlite] {
        let db = Db::new(backend);
        let sql = db.create_table("docs")
            .col(ColDef::new("a", ColType::TinyText))
            .col(ColDef::new("d", ColType::LongText))
            .to_sql().unwrap();
        assert!(sql.contains(r#""a" TEXT"#));
        assert!(sql.contains(r#""d" TEXT"#));
        assert!(!sql.contains("LONGTEXT"));
    }
}

#[test]
fn binary_blob_variants_mysql() {
    let my = Db::new(Backend::MySql);
    let sql = my.create_table("files")
        .col(ColDef::new("hash16",   ColType::Binary(16)))
        .col(ColDef::new("payload",  ColType::VarBinary(255)))
        .col(ColDef::new("tn",       ColType::TinyBlob))
        .col(ColDef::new("body",     ColType::Bytes))
        .col(ColDef::new("medium",   ColType::MediumBlob))
        .col(ColDef::new("huge",     ColType::LongBlob))
        .to_sql().unwrap();
    assert!(sql.contains("BINARY(16)"));
    assert!(sql.contains("VARBINARY(255)"));
    assert!(sql.contains("TINYBLOB"));
    assert!(sql.contains("`body` BLOB"));
    assert!(sql.contains("MEDIUMBLOB"));
    assert!(sql.contains("LONGBLOB"));
}

#[test]
fn binary_blob_collapse_in_pg_sqlite() {
    for backend in [Backend::Postgres, Backend::Sqlite] {
        let db = Db::new(backend);
        let sql = db.create_table("t")
            .col(ColDef::new("a", ColType::Binary(16)))
            .col(ColDef::new("b", ColType::VarBinary(255)))
            .col(ColDef::new("c", ColType::TinyBlob))
            .col(ColDef::new("d", ColType::LongBlob))
            .to_sql().unwrap();
        let expected = if backend == Backend::Postgres { "BYTEA" } else { "BLOB" };
        for col in ["a", "b", "c", "d"] {
            assert!(sql.contains(&format!(" {}", expected)), "{:?}/{}: {}", backend, col, sql);
        }
    }
}

#[test]
fn year_render_per_backend() {
    let my = Db::new(Backend::MySql);
    let sql = my.create_table("t").col(ColDef::new("y", ColType::Year)).to_sql().unwrap();
    assert!(sql.contains("`y` YEAR"));

    for backend in [Backend::Postgres, Backend::Sqlite] {
        let db = Db::new(backend);
        let sql = db.create_table("t").col(ColDef::new("y", ColType::Year)).to_sql().unwrap();
        let expected = if backend == Backend::Postgres { "SMALLINT" } else { "INTEGER" };
        assert!(sql.contains(expected), "{:?}: {}", backend, sql);
    }
}

#[test]
fn datetime_time_date_render_per_backend() {
    // Postgres: TIMESTAMPTZ vs TIMESTAMP, ambos TIME, DATE
    let pg = Db::new(Backend::Postgres);
    let sql = pg.create_table("t")
        .col(ColDef::new("ts_tz", ColType::Timestamp))
        .col(ColDef::new("ts",    ColType::DateTime))
        .col(ColDef::new("d",     ColType::Date))
        .col(ColDef::new("h",     ColType::Time))
        .to_sql().unwrap();
    assert!(sql.contains(r#""ts_tz" TIMESTAMPTZ"#));
    assert!(sql.contains(r#""ts" TIMESTAMP,"#));
    assert!(sql.contains(r#""d" DATE"#));
    assert!(sql.contains(r#""h" TIME"#));

    // MySQL: Timestamp→TIMESTAMP (UTC+session TZ), DateTime→DATETIME (naive)
    let my = Db::new(Backend::MySql);
    let sql = my.create_table("t")
        .col(ColDef::new("ts_tz", ColType::Timestamp))
        .col(ColDef::new("ts",    ColType::DateTime))
        .col(ColDef::new("d",     ColType::Date))
        .col(ColDef::new("h",     ColType::Time))
        .to_sql().unwrap();
    assert!(sql.contains("`ts_tz` TIMESTAMP"));
    assert!(!sql.contains("`ts_tz` DATETIME"));
    assert!(sql.contains("`ts` DATETIME"));
    assert!(sql.contains("`d` DATE"));
    assert!(sql.contains("`h` TIME"));

    // SQLite: todo TEXT salvo DATE
    let sq = Db::new(Backend::Sqlite);
    let sql = sq.create_table("t")
        .col(ColDef::new("ts_tz", ColType::Timestamp))
        .col(ColDef::new("ts",    ColType::DateTime))
        .col(ColDef::new("d",     ColType::Date))
        .col(ColDef::new("h",     ColType::Time))
        .to_sql().unwrap();
    assert!(sql.contains(r#""ts_tz" TEXT"#));
    assert!(sql.contains(r#""ts" TEXT"#));
    assert!(sql.contains(r#""d" DATE"#));
    assert!(sql.contains(r#""h" TEXT"#));
}

#[test]
fn timezone_aware_vs_naive_distinction() {
    // PG: el contraste es claro
    let pg = Db::new(Backend::Postgres);
    let sql = pg.create_table("eventos")
        .col(ColDef::new("instante",      ColType::Timestamp))  // con TZ
        .col(ColDef::new("agendado",      ColType::DateTime))   // sin TZ
        .col(ColDef::new("hora_evento",   ColType::Time))       // sin TZ
        .col(ColDef::new("hora_con_tz",   ColType::TimeTz))     // con TZ
        .to_sql().unwrap();
    assert!(sql.contains(r#""instante" TIMESTAMPTZ"#));
    assert!(sql.contains(r#""agendado" TIMESTAMP,"#));
    assert!(sql.contains(r#""hora_evento" TIME"#));
    assert!(sql.contains(r#""hora_con_tz" TIMETZ"#));

    // MySQL: TIMESTAMP es UTC + session TZ; DATETIME es naive
    let my = Db::new(Backend::MySql);
    let sql = my.create_table("eventos")
        .col(ColDef::new("instante", ColType::Timestamp))
        .col(ColDef::new("agendado", ColType::DateTime))
        .col(ColDef::new("hora_tz",  ColType::TimeTz))
        .to_sql().unwrap();
    assert!(sql.contains("`instante` TIMESTAMP"));
    assert!(sql.contains("`agendado` DATETIME"));
    assert!(sql.contains("`hora_tz` TEXT"));  // MySQL no tiene TIMETZ

    // SQLite: todo TEXT — el offset lo guarda el usuario en el string
    let sq = Db::new(Backend::Sqlite);
    let sql = sq.create_table("eventos")
        .col(ColDef::new("instante", ColType::Timestamp))
        .col(ColDef::new("hora_tz",  ColType::TimeTz))
        .to_sql().unwrap();
    assert!(sql.contains(r#""instante" TEXT"#));
    assert!(sql.contains(r#""hora_tz" TEXT"#));
}

#[test]
fn timestamp_default_now_with_tz() {
    // Patrón típico: TZ-aware con default 'now()' (PG) o CURRENT_TIMESTAMP (MySQL/SQLite)
    let pg = Db::new(Backend::Postgres);
    let sql = pg.create_table("logs")
        .col(ColDef::new("creado", ColType::Timestamp).not_null().default_raw("now()"))
        .to_sql().unwrap();
    assert!(sql.contains(r#""creado" TIMESTAMPTZ NOT NULL DEFAULT now()"#));

    let my = Db::new(Backend::MySql);
    let sql = my.create_table("logs")
        .col(ColDef::new("creado", ColType::Timestamp).not_null().default_raw("CURRENT_TIMESTAMP"))
        .to_sql().unwrap();
    assert!(sql.contains("`creado` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP"));
}

#[test]
fn enum_type_renders_per_backend() {
    // MySQL: ENUM('a','b','c')
    let db = Db::new(Backend::MySql);
    let sql = db.create_table("t")
        .col(ColDef::new("status", ColType::Enum(vec!["new".into(), "active".into(), "done".into()])).not_null())
        .to_sql().unwrap();
    assert_eq!(
        sql,
        "CREATE TABLE `t` (`status` ENUM('new','active','done') NOT NULL)"
    );

    // Postgres y SQLite: caen a TEXT
    for backend in [Backend::Postgres, Backend::Sqlite] {
        let db = Db::new(backend);
        let sql = db.create_table("t")
            .col(ColDef::new("status", ColType::Enum(vec!["new".into(), "done".into()])))
            .to_sql().unwrap();
        assert!(sql.contains("TEXT"), "{:?}: {}", backend, sql);
        assert!(!sql.contains("ENUM"), "{:?}: {}", backend, sql);
    }
}

#[test]
fn set_type_renders_per_backend() {
    let db = Db::new(Backend::MySql);
    let sql = db.create_table("t")
        .col(ColDef::new("flags", ColType::Set(vec!["read".into(), "write".into(), "admin".into()])))
        .to_sql().unwrap();
    assert_eq!(
        sql,
        "CREATE TABLE `t` (`flags` SET('read','write','admin'))"
    );
}

#[test]
fn enum_rejects_injection_in_values() {
    let db = Db::new(Backend::MySql);
    let err = db.create_table("t")
        .col(ColDef::new("x", ColType::Enum(vec!["ok".into(), "bad' OR '1".into()])))
        .to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));

    let err = db.create_table("t")
        .col(ColDef::new("x", ColType::Enum(vec!["a\\b".into()])))
        .to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn enum_empty_values_errors() {
    let db = Db::new(Backend::MySql);
    let err = db.create_table("t")
        .col(ColDef::new("x", ColType::Enum(vec![])))
        .to_sql().unwrap_err();
    assert!(matches!(err, QueryError::EmptyRecord));
}

#[test]
fn drop_table_with_options() {
    let db = Db::new(Backend::Postgres);
    let sql = db.drop_table("users").if_exists().cascade().to_sql().unwrap();
    assert_eq!(sql, r#"DROP TABLE IF EXISTS "users" CASCADE"#);
}

#[test]
fn alter_table_add_drop_rename() {
    let db = Db::new(Backend::Postgres);
    let sqls = db
        .alter_table("users")
        .add_column(ColDef::new("nickname", ColType::Text))
        .drop_column("legacy_field")
        .rename_column("name", "full_name")
        .rename_table("app_users")
        .to_sql()
        .unwrap();
    assert_eq!(sqls.len(), 4);
    assert_eq!(sqls[0], r#"ALTER TABLE "users" ADD COLUMN "nickname" TEXT"#);
    assert_eq!(sqls[1], r#"ALTER TABLE "users" DROP COLUMN "legacy_field""#);
    assert_eq!(sqls[2], r#"ALTER TABLE "users" RENAME COLUMN "name" TO "full_name""#);
    assert_eq!(sqls[3], r#"ALTER TABLE "users" RENAME TO "app_users""#);
}

#[test]
fn alter_convert_to_charset_mysql() {
    let db = Db::new(Backend::MySql);
    let sqls = db
        .alter_table("users")
        .convert_to_charset_collate("utf8mb4", "utf8mb4_unicode_ci")
        .to_sql()
        .unwrap();
    assert_eq!(
        sqls,
        vec!["ALTER TABLE `users` CONVERT TO CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci"]
    );
}

#[test]
fn alter_convert_to_charset_only_no_collation() {
    let db = Db::new(Backend::MySql);
    let sqls = db
        .alter_table("users")
        .convert_to_charset("utf8mb4")
        .to_sql()
        .unwrap();
    assert_eq!(
        sqls,
        vec!["ALTER TABLE `users` CONVERT TO CHARACTER SET utf8mb4"]
    );
}

#[test]
fn alter_set_default_charset_and_engine() {
    let db = Db::new(Backend::MySql);
    let sqls = db
        .alter_table("users")
        .set_default_charset_collate("utf8mb4", "utf8mb4_unicode_ci")
        .set_engine("InnoDB")
        .to_sql()
        .unwrap();
    assert_eq!(sqls.len(), 2);
    assert_eq!(sqls[0], "ALTER TABLE `users` DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci");
    assert_eq!(sqls[1], "ALTER TABLE `users` ENGINE=InnoDB");
}

#[test]
fn alter_charset_actions_skipped_in_postgres_sqlite() {
    for backend in [Backend::Postgres, Backend::Sqlite] {
        let db = Db::new(backend);
        let sqls = db
            .alter_table("t")
            .convert_to_charset("utf8mb4")
            .set_default_charset("utf8mb4")
            .set_engine("InnoDB")
            .to_sql()
            .unwrap();
        assert!(sqls.is_empty(), "{:?}: {:?}", backend, sqls);
    }
}

#[test]
fn create_database_mysql_with_charset() {
    let db = Db::new(Backend::MySql);
    let sql = db
        .create_database("terapias_x")
        .if_not_exists()
        .default_charset("utf8mb4")
        .default_collation("utf8mb4_unicode_ci")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "CREATE DATABASE IF NOT EXISTS `terapias_x` DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci"
    );
}

#[test]
fn create_database_postgres_with_encoding_and_locale() {
    let db = Db::new(Backend::Postgres);
    let sql = db
        .create_database("midb")
        .default_charset("UTF8")
        .default_collation("en_US.UTF-8")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"CREATE DATABASE "midb" ENCODING 'UTF8' LC_COLLATE 'en_US.UTF-8'"#
    );
}

#[test]
fn create_database_sqlite_errors() {
    let db = Db::new(Backend::Sqlite);
    let err = db.create_database("x").to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidOperator(_)));
}

#[test]
fn drop_database_with_if_exists() {
    let pg = Db::new(Backend::Postgres);
    let sql = pg.drop_database("midb").if_exists().to_sql().unwrap();
    assert_eq!(sql, r#"DROP DATABASE IF EXISTS "midb""#);

    let my = Db::new(Backend::MySql);
    let sql = my.drop_database("midb").to_sql().unwrap();
    assert_eq!(sql, "DROP DATABASE `midb`");
}

#[test]
fn database_rejects_injection_in_name() {
    let db = Db::new(Backend::MySql);
    let err = db.create_database("x; DROP--").to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
    let err = db.drop_database("a' OR '1").to_sql().unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn collation_name_accepts_locale_dots_and_dashes() {
    // PG locale strings need to pass validation
    let db = Db::new(Backend::Postgres);
    let sql = db
        .create_table("t")
        .col(ColDef::new("nombre", ColType::Text).collation("en_US.UTF-8"))
        .to_sql()
        .unwrap();
    assert!(sql.contains(r#"COLLATE "en_US.UTF-8""#));
}

#[test]
fn ddl_rejects_injection_in_column_default() {
    let db = Db::new(Backend::Postgres);
    let err = db
        .create_table("t")
        .col(ColDef::new("x", ColType::Int).default_raw("1; DROP TABLE users--"))
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn collation_per_column_mysql() {
    let db = Db::new(Backend::MySql);
    let sql = db
        .create_table("t")
        .col(
            ColDef::new("nombre", ColType::Varchar(100))
                .not_null()
                .charset("utf8mb4")
                .collation("utf8mb4_unicode_ci"),
        )
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "CREATE TABLE `t` (`nombre` VARCHAR(100) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL)"
    );
}

#[test]
fn collation_table_level_mysql_appends_charset_and_engine() {
    let db = Db::new(Backend::MySql);
    let sql = db
        .create_table("t")
        .col(ColDef::new("id", ColType::Int).primary_key().auto_increment())
        .engine("InnoDB")
        .default_charset("utf8mb4")
        .default_collation("utf8mb4_unicode_ci")
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        "CREATE TABLE `t` (`id` INTEGER PRIMARY KEY AUTO_INCREMENT) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci"
    );
}

#[test]
fn collation_per_column_postgres_quotes_name() {
    let db = Db::new(Backend::Postgres);
    let sql = db
        .create_table("t")
        .col(
            ColDef::new("nombre", ColType::Text)
                .not_null()
                .collation("es_CL"),
        )
        .to_sql()
        .unwrap();
    assert_eq!(
        sql,
        r#"CREATE TABLE "t" ("nombre" TEXT COLLATE "es_CL" NOT NULL)"#
    );
}

#[test]
fn collation_table_level_ignored_in_postgres_sqlite() {
    for backend in [Backend::Postgres, Backend::Sqlite] {
        let db = Db::new(backend);
        let sql = db
            .create_table("t")
            .col(ColDef::new("id", ColType::Int).primary_key())
            .default_charset("utf8mb4")
            .default_collation("utf8mb4_unicode_ci")
            .engine("InnoDB")
            .to_sql()
            .unwrap();
        assert!(!sql.contains("CHARSET"), "{:?}: {}", backend, sql);
        assert!(!sql.contains("ENGINE"), "{:?}: {}", backend, sql);
    }
}

#[test]
fn collation_rejects_injection_in_name() {
    let db = Db::new(Backend::MySql);
    let err = db
        .create_table("t")
        .col(ColDef::new("id", ColType::Int).primary_key())
        .default_collation("utf8mb4_unicode_ci; DROP--")
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));

    let err = db
        .create_table("t")
        .col(ColDef::new("x", ColType::Text).collation("a' OR '1"))
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}

#[test]
fn ddl_rejects_injection_in_table_name() {
    let db = Db::new(Backend::Postgres);
    let err = db
        .create_table("users; DROP")
        .col(ColDef::new("id", ColType::Int).primary_key())
        .to_sql()
        .unwrap_err();
    assert!(matches!(err, QueryError::InvalidIdentifier(_)));
}
