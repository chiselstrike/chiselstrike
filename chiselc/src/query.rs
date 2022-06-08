//! Query intermediate representation.
//!
//! The compiler parses TypeScripts to identify fragments in the code that
//! represent queries. The queries are then transformed into this query
//! intermediate representation.

use indexmap::IndexSet;
use swc_ecmascript::ast::CallExpr;

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
    /// A literal expression.
    Literal(Literal),
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

/// A literal expression
#[derive(Debug)]
pub enum Literal {
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
    /// The original call expression of the filter.
    pub call_expr: CallExpr,
    /// The pure (no side-effects) part of the filter expression AST.
    pub pure: Box<swc_ecmascript::ast::Expr>,
    /// The impure (possible side-effects) part of the filter expression AST.
    pub impure: Option<Box<swc_ecmascript::ast::Expr>>,
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
