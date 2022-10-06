use std::fmt::{self, Write};
use crate::layout;

#[derive(Debug)]
pub struct SqlWriter {
    kind: sqlx::any::AnyKind,
    text: String,
}

impl SqlWriter {
    pub fn new(kind: sqlx::any::AnyKind) -> Self {
        Self { kind, text: String::new() }
    }

    pub fn write<T: WriteSql + ?Sized>(&mut self, x: &T) {
        x.write_sql(self);
    }

    pub fn write_str(&mut self, x: &str) {
        self.text.push_str(x);
    }

    pub fn write_name(&mut self, name: &layout::Name) {
        let name = &name.0;
        self.text.reserve(2 + name.len());
        self.text.push('"');
        for c in name.chars() {
            if c == '"' {
                self.text.push_str("\"\"");
            } else {
                self.text.push(c);
            }
        }
    }

    pub fn write_param(&mut self, idx: usize) {
        match self.kind {
            sqlx::any::AnyKind::Sqlite => write!(self, "?{}", idx + 1),
            sqlx::any::AnyKind::Postgres => write!(self, "${}", idx + 1),
        }
    }

    pub fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) {
        self.text.write_fmt(fmt).expect("formatting failed")
    }

    pub fn build(self) -> String {
        self.text
    }
}

pub trait WriteSql {
    fn write_sql(&self, writer: &mut SqlWriter);
}

impl WriteSql for str {
    fn write_sql(&self, writer: &mut SqlWriter) {
        writer.write_str(self);
    }
}

impl WriteSql for layout::Name {
    fn write_sql(&self, writer: &mut SqlWriter) {
        writer.write_name(self);
    }
}
