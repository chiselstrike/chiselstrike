// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

//! Query intermediate representation.
//!
//! The compiler parses TypeScripts to identify fragments in the code that
//! represent queries. The queries are then transformed into this query
//! intermediate representation.

use indexmap::IndexSet;

/// An expression.
#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Expr {
    /// A binary expression.
    BinaryExpr(BinaryExpr),
    /// An entity property.
    PropertyAccess(PropertyAccessExpr),
    /// An identifier expression.
    Identifier(String),
    /// A value expression.
    Value(Value),
}

/// A binary expression.
#[derive(Debug)]
pub struct BinaryExpr {
    pub left: Box<Expr>,
    pub op: BinaryOp,
    pub right: Box<Expr>,
}

/// A property access expression.
#[derive(Debug)]
pub struct PropertyAccessExpr {
    pub object: Box<Expr>,
    pub property: String,
}

/// A binary operator.
#[derive(Debug)]
pub enum BinaryOp {
    And,
    Eq,
    Gt,
    GtEq,
    Lt,
    LtEq,
    NotEq,
    Or,
}

/// A value expression
#[derive(Debug)]
pub enum Value {
    Bool(bool),
    Num(f64),
    Str(String),
}

/// A query operator.
#[derive(Debug)]
pub enum Operator {
    /// The filter operator filters a subset of its input given a predicate.
    Filter(Filter),
    /// The scan operator returns all entities for a given entity type.
    Scan(Scan),
}

/// Scan operator.
#[derive(Debug)]
pub struct Scan {
    /// The entity type to scan for.
    pub entity_type: String,
    /// Alias for the entity.
    pub alias: String,
}

/// Filter operator.
#[derive(Debug)]
pub struct Filter {
    /// The ChiselStrike internal function to call.
    pub function: String,
    /// The original call expression of the filter. Note that `query_expr`
    /// logically ANDed with `post_expr` is always equivalent with `call_expr`.
    pub call_expr: swc_ecmascript::ast::CallExpr,
    /// The query expression part of the filter expression AST. Note that
    /// this is the same as `predicate`, but in AST format. We need this
    /// because the internal filtering API needs a fallback predicate if
    /// runtime query transformation fails.
    pub query_expr: Box<swc_ecmascript::ast::Expr>,
    /// The post filter predicate expression AST that is always evaluateed at
    /// runtime, which allows expression with side-effects, for example.
    pub post_expr: Option<Box<swc_ecmascript::ast::Expr>>,
    /// The parameters to this filter.
    pub parameters: Vec<String>,
    /// The predicate expression to filter by.
    pub predicate: Expr,
    /// The input query operator that is filtered.
    pub input: Box<Operator>,
}

impl Filter {
    pub fn properties(&self) -> IndexSet<String> {
        let mut props = IndexSet::new();
        expr_to_props(&self.predicate, &mut props);
        props
    }
}

fn expr_to_props(expr: &Expr, props: &mut IndexSet<String>) {
    match expr {
        Expr::BinaryExpr(binary_expr) => {
            expr_to_props(&binary_expr.left, props);
            expr_to_props(&binary_expr.right, props);
        }
        Expr::PropertyAccess(property_access) => {
            props.insert(property_access.property.clone());
        }
        _ => { /* Nothing to do */ }
    }
}
