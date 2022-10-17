use std::fmt::{self, Write};
use crate::layout;

/// Helper struct for generating SQL statements.
#[derive(Debug)]
pub struct SqlWriter {
    kind: sqlx::any::AnyKind,
    text: String,
}

impl SqlWriter {
    pub fn new(kind: sqlx::any::AnyKind) -> Self {
        Self { kind, text: String::new() }
    }

    /// Overloaded helper method that calls a `write_*` method depending on the type `T`.
    pub fn write<T: WriteSql + ?Sized>(&mut self, x: &T) {
        x.write_sql(self);
    }

    /// Appends the string verbatim into the SQL statement.
    pub fn write_str(&mut self, x: &str) {
        self.text.push_str(x);
    }

    /// Appends the name as a quoted identifier into the SQL statement.
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
        self.text.push('"');
    }

    /// Appends a parameter with given **zero-based** index into the SQL statement.
    ///
    /// This uses the correct syntax depending on the database (`?n` for SQLite, `$n` for
    /// Postgres). Note that the `idx` is zero-based, but the SQL parameter syntax is one-based, to
    /// `idx` of 0 produces `?1` (or `$1`).
    pub fn write_param(&mut self, idx: usize) {
        match self.kind {
            sqlx::any::AnyKind::Sqlite => write!(self, "?{}", idx + 1),
            sqlx::any::AnyKind::Postgres => write!(self, "${}", idx + 1),
        }
    }

    /// This method makes the `write!` macro work with this struct.
    pub fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) {
        self.text.write_fmt(fmt).expect("formatting failed")
    }

    /// Returns the produced SQL statement.
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
