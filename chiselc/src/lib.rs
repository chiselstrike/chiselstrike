pub mod parse;
pub mod policies;
pub mod rewrite;
pub mod symbols;

mod filtering;
mod query;
mod transforms;
mod utils;

pub(crate) mod tools;

pub type Symbol = swc_atoms::JsWord;
