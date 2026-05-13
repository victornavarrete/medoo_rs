//! Logging de SQL ejecutado. Categorías como bitflags (puede combinar
//! varias), sinks intercambiables (stdout/stderr/archivo/buffer en
//! memoria para tests). El log se dispara cuando el usuario llama
//! `Db::build(&q)` o explícitamente `db.log_ddl()` / `db.log_raw()`.

use crate::value::Value;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogCategory(u32);

impl LogCategory {
    pub const READ: Self = Self(1 << 0);
    pub const INSERT: Self = Self(1 << 1);
    pub const UPDATE: Self = Self(1 << 2);
    pub const DELETE: Self = Self(1 << 3);
    pub const DDL: Self = Self(1 << 4);
    pub const RAW: Self = Self(1 << 5);

    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(!0);
    pub const WRITE: Self = Self(Self::INSERT.0 | Self::UPDATE.0 | Self::DELETE.0);

    pub const fn bits(self) -> u32 {
        self.0
    }
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0 && other.0 != 0
    }
    pub fn label(self) -> &'static str {
        match self.0 {
            x if x == Self::READ.0 => "READ",
            x if x == Self::INSERT.0 => "INSERT",
            x if x == Self::UPDATE.0 => "UPDATE",
            x if x == Self::DELETE.0 => "DELETE",
            x if x == Self::DDL.0 => "DDL",
            x if x == Self::RAW.0 => "RAW",
            _ => "MIXED",
        }
    }
}

impl std::ops::BitOr for LogCategory {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl std::ops::BitOrAssign for LogCategory {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

pub enum LogSink {
    Stdout,
    Stderr,
    File(Mutex<File>),
    /// Buffer en memoria — útil en tests para inspeccionar lo escrito.
    Buffer(Arc<Mutex<Vec<u8>>>),
}

pub struct Logger {
    sink: LogSink,
    filter: LogCategory,
}

impl Logger {
    pub fn stdout() -> Self {
        Self { sink: LogSink::Stdout, filter: LogCategory::ALL }
    }
    pub fn stderr() -> Self {
        Self { sink: LogSink::Stderr, filter: LogCategory::ALL }
    }
    pub fn file<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let f = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { sink: LogSink::File(Mutex::new(f)), filter: LogCategory::ALL })
    }
    /// Devuelve `(logger, buffer)` para que los tests inspeccionen lo
    /// que se escribió sin tocar disco ni stdout.
    pub fn buffer() -> (Self, Arc<Mutex<Vec<u8>>>) {
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        (
            Self {
                sink: LogSink::Buffer(buf.clone()),
                filter: LogCategory::ALL,
            },
            buf,
        )
    }

    /// Limita las categorías que pasan al sink.
    pub fn filter(mut self, cats: LogCategory) -> Self {
        self.filter = cats;
        self
    }

    pub fn log(&self, cat: LogCategory, sql: &str, params: &[Value]) {
        if !self.filter.contains(cat) {
            return;
        }
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut line = format!("[{}s] [{:<6}] {}", ts, cat.label(), sql);
        if !params.is_empty() {
            line.push_str(" -- params: ");
            line.push_str(&format!("{:?}", params));
        }
        line.push('\n');
        match &self.sink {
            LogSink::Stdout => {
                print!("{}", line);
            }
            LogSink::Stderr => {
                eprint!("{}", line);
            }
            LogSink::File(m) => {
                if let Ok(mut f) = m.lock() {
                    let _ = f.write_all(line.as_bytes());
                    let _ = f.flush();
                }
            }
            LogSink::Buffer(b) => {
                if let Ok(mut v) = b.lock() {
                    v.extend_from_slice(line.as_bytes());
                }
            }
        }
    }
}

/// Trait que cada builder implementa para reportar su categoría
/// y construir SQL parametrizado.
pub trait Query {
    fn category(&self) -> LogCategory;
    fn build_sql(&self) -> crate::error::Result<(String, Vec<Value>)>;
}
