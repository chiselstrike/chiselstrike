use anyhow::Result;
use serde_derive::{Deserialize, Serialize};

/// An expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "exprType")]
pub(crate) enum Expr {
    /// A literal expression.
    Literal { value: Literal },
    /// A reference to an entity (sub)field (eg: BlogPost.author.birthplace.country). We currently support
    /// properties of only one entity: the filter-predicate's single parameter. We therefore only need to
    /// track the chain of nested field names (eg: ["author", "birthplace", "country"] per above).
    Field { names: Vec<String> },
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

impl From<Vec<String>> for Expr {
    fn from(names: Vec<String>) -> Self {
        Expr::Field { names }
    }
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

impl From<Option<Literal>> for Literal {
    fn from(opt: Option<Literal>) -> Literal {
        match opt {
            Some(literal) => literal,
            None => Literal::Null,
        }
    }
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
