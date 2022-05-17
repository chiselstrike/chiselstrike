use serde_derive::{Deserialize, Serialize};

/// An expression.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "exprType")]
pub(crate) enum Expr {
    /// A literal expression.
    Literal { value: Literal },
    /// Expression for addressing function parameters of the current expression
    Parameter { position: usize },
    /// Expression for addressing entity property
    Property(PropertyAccess),
    /// A binary expression.
    Binary(BinaryExpr),
}

impl From<Literal> for Expr {
    fn from(literal: Literal) -> Self {
        Expr::Literal { value: literal }
    }
}

impl From<BinaryExpr> for Expr {
    fn from(expr: BinaryExpr) -> Self {
        Expr::Binary(expr)
    }
}

impl From<PropertyAccess> for Expr {
    fn from(prop_access: PropertyAccess) -> Self {
        Expr::Property(prop_access)
    }
}

/// Various literals.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Literal {
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    String(String),
    Null,
}

impl From<bool> for Literal {
    fn from(val: bool) -> Self {
        Literal::Bool(val)
    }
}

impl From<u64> for Literal {
    fn from(val: u64) -> Self {
        Literal::U64(val)
    }
}

impl From<i64> for Literal {
    fn from(val: i64) -> Self {
        Literal::I64(val)
    }
}

impl From<f64> for Literal {
    fn from(val: f64) -> Self {
        Literal::F64(val)
    }
}

impl From<String> for Literal {
    fn from(val: String) -> Self {
        Literal::String(val)
    }
}

impl From<&str> for Literal {
    fn from(val: &str) -> Self {
        Literal::String(val.to_owned())
    }
}

impl From<Option<Literal>> for Literal {
    fn from(opt: Option<Literal>) -> Literal {
        match opt {
            Some(literal) => literal,
            None => Literal::Null,
        }
    }
}

/// Expression of a property access on an Entity
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PropertyAccess {
    /// Name of a property that will be accessed.
    pub property: String,
    /// Expression whose property will be accessed. The expression
    /// can be either another Property access or a Parameter representing
    /// an entity.
    pub object: Box<Expr>,
}

/// A binary operator.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum BinaryOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    Like,
    NotLike,
}

impl BinaryOp {
    pub fn to_sql_string(&self) -> &str {
        match &self {
            Self::Eq => "=",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::LtEq => "<=",
            Self::Gt => ">",
            Self::GtEq => ">=",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Like => "LIKE",
            Self::NotLike => "NOT LIKE",
        }
    }
}

/// A binary expression.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BinaryExpr {
    pub left: Box<Expr>,
    pub op: BinaryOp,
    pub right: Box<Expr>,
}

macro_rules! make_op_method {
    ($MethodName:ident, $EnumName:ident) => {
        #[allow(dead_code)]
        pub fn $MethodName(lhs: Expr, rhs: Expr) -> Expr {
            Self::new(BinaryOp::$EnumName, lhs, rhs).into()
        }
    };
}

impl BinaryExpr {
    pub(crate) fn new(op: BinaryOp, lhs: Expr, rhs: Expr) -> Self {
        BinaryExpr {
            left: Box::new(lhs),
            op,
            right: Box::new(rhs),
        }
    }

    make_op_method! {eq, Eq}
    make_op_method! {not_eq, NotEq}
    make_op_method! {lt, Lt}
    make_op_method! {lt_eq, LtEq}
    make_op_method! {gt, Gt}
    make_op_method! {gt_eq, GtEq}
    make_op_method! {and, And}
    make_op_method! {or, Or}
    make_op_method! {like, Like}
    make_op_method! {not_like, NotLike}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_parsing_bool() {
        let expr: Expr = serde_json::from_str(
            r#"{
            "exprType": "Literal",
            "value": true
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Literal {
                value: Literal::Bool(true)
            }
        ));
    }

    #[test]
    fn test_literal_parsing_u64() {
        let expr = serde_json::from_str(
            r#"{
            "exprType": "Literal",
            "value": 42
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Literal {
                value: Literal::U64(42)
            }
        ));
    }

    #[test]
    fn test_literal_parsing_i64() {
        let expr = serde_json::from_str(
            r#"{
            "exprType": "Literal",
            "value": -42
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Literal {
                value: Literal::I64(-42)
            }
        ));
    }

    #[test]
    fn test_literal_parsing_f64() {
        let expr = serde_json::from_str(
            r#"{
            "exprType": "Literal",
            "value": 42.0
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Literal {
                value: Literal::F64(_)
            }
        ));
        if let Expr::Literal {
            value: Literal::F64(v),
        } = expr
        {
            assert_eq!(v, 42.0);
        } else {
            panic!("failed to match the literal");
        }
    }

    #[test]
    fn test_literal_parsing_string() {
        let expr: Expr = serde_json::from_str(
            r#"{
            "exprType": "Literal",
            "value": "I'm the best literal"
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Literal {
                value: Literal::String(_)
            }
        ));
        if let Expr::Literal {
            value: Literal::String(v),
        } = expr
        {
            assert_eq!(v, "I'm the best literal");
        } else {
            panic!("failed to match the literal");
        }
    }

    #[test]
    fn test_literal_parsing_null() {
        let expr = serde_json::from_str(
            r#"{
            "exprType": "Literal",
            "value": null
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Literal {
                value: Literal::Null
            }
        ));
    }

    #[test]
    #[should_panic(expected = "missing field `value`")]
    fn test_literal_parsing_value_missing_panic() {
        let _expr: Expr = serde_json::from_str(
            r#"{
            "exprType": "Literal"
        }"#,
        )
        .unwrap();
    }
}
