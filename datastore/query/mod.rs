use chisel_snapshot::schema;
use deno_core::v8;
use std::sync::Arc;
use crate::layout;

pub mod build;
mod eval;
pub mod exec;

#[derive(Debug)]
pub struct Query {
    schema: Arc<schema::Schema>,
    sql_text: String,
    inputs: Vec<InputParam>,
    output: Option<OutputExpr>,
}

/// Parameter of an SQL query, computed from the user-provided JS argument.
#[derive(Debug)]
pub enum InputParam {
    /// A JS id encoded into SQL.
    Id(layout::IdRepr, InputExpr),
    /// A JS field encoded into SQL.
    Field(layout::FieldRepr, Arc<schema::Type>, InputExpr),
}

/// Expression that computes a JS value from the user-provided JS argument.
#[derive(Debug)]
pub enum InputExpr {
    /// The JS argument provided by the user.
    Arg,
    /// Get property of a JS object.
    Get(Box<InputExpr>, v8::Global<v8::String>),
}

/// Expression that computes a JS output value from SQL row.
#[derive(Debug)]
pub enum OutputExpr {
    /// Create a JS object.
    Object(Vec<(v8::Global<v8::String>, OutputExpr)>),
    /// Decode a JS id from given index in the SQL row.
    Id(layout::IdRepr, usize),
    /// Decode a JS field from given index in the SQL row.
    Field(layout::FieldRepr, Arc<schema::Type>, usize),
}





