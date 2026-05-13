# medoo_derive

Proc-macros para [`medoo_rs`](https://crates.io/crates/medoo_rs).

Crate auxiliar — **no se usa directo**. Activá la feature `derive` en
`medoo_rs` y obtenés `#[derive(FromRow)]`:

```toml
[dependencies]
medoo_rs = { version = "0.1", features = ["runtime-postgres", "derive"] }
```

```rust
use medoo_rs::FromRow;

#[derive(FromRow)]
struct User {
    id: i64,
    name: String,
    email: Option<String>,
    #[medoo(rename = "user_active")]
    active: bool,
}
```

Tipos soportados: `i8..i64`, `u8..u64`, `f32`, `f64`, `bool`, `String`,
`Vec<u8>`, `Option<T>` de cualquiera.

## Licencia

Apache-2.0. Ver [LICENSE](../LICENSE) en el repo padre.
