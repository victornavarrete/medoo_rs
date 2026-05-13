use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    /// JSON crudo (string). MySQL JSON, Postgres json/jsonb, SQLite TEXT.
    /// La capa de bind decide cómo enviarlo: para Postgres normalmente
    /// como `text` con cast `::jsonb`; para MySQL como string que la DB
    /// valida automáticamente al insertar en columna JSON.
    Json(String),
}

impl Value {
    /// Constructor para JSON desde cualquier `Into<String>`.
    /// No valida sintaxis: lo hace el motor al ejecutar.
    pub fn json<S: Into<String>>(s: S) -> Self {
        Value::Json(s.into())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(x) => write!(f, "{}", x),
            Value::Text(s) => write!(f, "{}", s),
            Value::Bytes(b) => write!(f, "<{} bytes>", b.len()),
            Value::Json(s) => write!(f, "{}", s),
        }
    }
}

pub trait IntoValue {
    fn into_value(self) -> Value;
}

impl IntoValue for Value {
    fn into_value(self) -> Value {
        self
    }
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}

macro_rules! impl_int {
    ($($t:ty),*) => {
        $(impl IntoValue for $t {
            fn into_value(self) -> Value { Value::Int(self as i64) }
        })*
    };
}
impl_int!(i8, i16, i32, i64, u8, u16, u32);

impl IntoValue for u64 {
    fn into_value(self) -> Value {
        Value::Int(self as i64)
    }
}

impl IntoValue for f32 {
    fn into_value(self) -> Value {
        Value::Float(self as f64)
    }
}
impl IntoValue for f64 {
    fn into_value(self) -> Value {
        Value::Float(self)
    }
}

impl IntoValue for &str {
    fn into_value(self) -> Value {
        Value::Text(self.to_string())
    }
}
impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::Text(self)
    }
}
impl<'a> IntoValue for &'a String {
    fn into_value(self) -> Value {
        Value::Text(self.clone())
    }
}

impl<T: IntoValue> IntoValue for Option<T> {
    fn into_value(self) -> Value {
        match self {
            Some(v) => v.into_value(),
            None => Value::Null,
        }
    }
}

#[cfg(feature = "chrono")]
mod chrono_impls {
    use super::{IntoValue, Value};
    use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};

    impl IntoValue for DateTime<Utc> {
        fn into_value(self) -> Value {
            Value::Text(self.to_rfc3339())
        }
    }
    impl<Tz: TimeZone> IntoValue for &DateTime<Tz>
    where Tz::Offset: std::fmt::Display {
        fn into_value(self) -> Value {
            Value::Text(self.to_rfc3339())
        }
    }
    impl IntoValue for NaiveDateTime {
        fn into_value(self) -> Value {
            // formato MySQL/SQLite estándar: 'YYYY-MM-DD HH:MM:SS[.ffffff]'
            Value::Text(self.format("%Y-%m-%d %H:%M:%S%.f").to_string())
        }
    }
    impl IntoValue for NaiveDate {
        fn into_value(self) -> Value {
            Value::Text(self.format("%Y-%m-%d").to_string())
        }
    }
    impl IntoValue for NaiveTime {
        fn into_value(self) -> Value {
            Value::Text(self.format("%H:%M:%S%.f").to_string())
        }
    }
}
