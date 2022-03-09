use anyhow::Result;
use serde_derive::{Deserialize, Serialize};

/// An expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "expr_type")]
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

/// Various literals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Literal {
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    String(String),
}

/// Expression of a property access on an Entity
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BinaryExpr {
    pub left: Box<Expr>,
    pub op: BinaryOp,
    pub right: Box<Expr>,
}

pub(crate) fn from_json(json: serde_json::Value) -> Result<Expr> {
    Ok(serde_json::from_value(json)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_parsing_bool() {
        let expr: Expr = serde_json::from_str(
            r#"{
            "expr_type": "Literal",
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
            "expr_type": "Literal",
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
            "expr_type": "Literal",
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
            "expr_type": "Literal",
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
            "expr_type": "Literal",
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
}
