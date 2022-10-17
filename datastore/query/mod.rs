use chisel_snapshot::schema;
use deno_core::v8;
use std::sync::Arc;
use crate::layout;

pub mod build;
mod eval;
pub mod exec;

/// An SQL statement that reads input parameters from a JS value and converts the output rows into
/// JS values.
///
/// The SQL statement in [`sql_text`][Self::sql_text] may contain input parameters such as `?1` or
/// `$2` (syntax depends on the SQL dialect). When we execute the query, you need to provide a
/// single JS value (the "argument"), which is then used to fill the input parameters. The
/// [`InputParam`]-s in [`inputs`][Self::inputs] describe how we compute a JS value for the
/// parameter from the argument and how we convert the value into an SQL value.
///
/// If the statement returns rows, then the [`OutputExpr`] in [`output`][Self::output] describes
/// how to build a JS value from the SQL values in each row.
#[derive(Debug)]
pub struct Query {
    schema: Arc<schema::Schema>,
    sql_text: String,
    inputs: Vec<InputParam>,
    output: Option<OutputExpr>,
}

/// Parameter of an SQL query, computed from the single JS "argument" that you provide when you
/// execute a [`Query`].
#[derive(Debug)]
pub enum InputParam {
    /// A JS id encoded into SQL.
    Id(layout::IdRepr, InputExpr),
    /// A JS field encoded into SQL.
    Field(layout::FieldRepr, Arc<schema::Type>, InputExpr),
}

/// Expression that computes a JS value from the single JS "argument".
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
    /// Decode a JS id from column with given index in the SQL row.
    Id(layout::IdRepr, usize),
    /// Decode a JS field from column with given index in the SQL row.
    Field(layout::FieldRepr, Arc<schema::Type>, usize),
}





