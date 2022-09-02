use chiselc::policies::LogicOp;
use serde_derive::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// An expression.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "exprType")]
pub enum Expr {
    /// A value expression.
    Value {
        value: Value,
    },
    /// Expression for addressing function parameters of the current expression
    Parameter {
        position: usize,
    },
    /// Expression for addressing entity property
    Property(PropertyAccess),
    /// A binary expression.
    Binary(BinaryExpr),
    Not(Box<Self>),
}

impl From<Value> for Expr {
    fn from(value: Value) -> Self {
        Expr::Value { value }
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

/// Various Values.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    String(String),
    Null,
}

impl From<bool> for Value {
    fn from(val: bool) -> Self {
        Value::Bool(val)
    }
}

impl From<u64> for Value {
    fn from(val: u64) -> Self {
        Value::U64(val)
    }
}

impl From<i64> for Value {
    fn from(val: i64) -> Self {
        Value::I64(val)
    }
}

impl From<f64> for Value {
    fn from(val: f64) -> Self {
        Value::F64(val)
    }
}

impl From<String> for Value {
    fn from(val: String) -> Self {
        Value::String(val)
    }
}

impl From<&str> for Value {
    fn from(val: &str) -> Self {
        Value::String(val.to_owned())
    }
}

impl From<Option<Value>> for Value {
    fn from(opt: Option<Value>) -> Value {
        match opt {
            Some(val) => val,
            None => Value::Null,
        }
    }
}

impl From<&JsonValue> for Value {
    fn from(json: &JsonValue) -> Self {
        match json {
            JsonValue::Null => Value::Null,
            JsonValue::Bool(b) => Value::Bool(*b),
            JsonValue::Number(n) if n.is_i64() => Value::I64(n.as_i64().unwrap()),
            JsonValue::Number(n) if n.is_u64() => Value::U64(n.as_u64().unwrap()),
            JsonValue::Number(n) if n.is_f64() => Value::F64(n.as_f64().unwrap()),
            JsonValue::String(s) => Value::String(s.clone()),
            JsonValue::Array(_) | JsonValue::Object(_) => {
                unimplemented!("object and arrays not part of the data model.")
            }
            _ => unreachable!(),
        }
    }
}

/// Expression of a property access on an Entity
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyAccess {
    /// Name of a property that will be accessed.
    pub property: String,
    /// Expression whose property will be accessed. The expression
    /// can be either another Property access or a Parameter representing
    /// an entity.
    pub object: Box<Expr>,
}

/// A binary operator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BinaryOp {
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

impl From<LogicOp> for BinaryOp {
    fn from(op: LogicOp) -> Self {
        match op {
            LogicOp::Eq => BinaryOp::Eq,
            LogicOp::Neq => BinaryOp::NotEq,
            LogicOp::Gt => BinaryOp::Gt,
            LogicOp::Gte => BinaryOp::GtEq,
            LogicOp::Lt => BinaryOp::Lt,
            LogicOp::Lte => BinaryOp::LtEq,
            LogicOp::And => BinaryOp::And,
            LogicOp::Or => BinaryOp::Or,
        }
    }
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
pub struct BinaryExpr {
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
    pub fn new(op: BinaryOp, lhs: Expr, rhs: Expr) -> Self {
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
    fn test_value_parsing_bool() {
        let expr: Expr = serde_json::from_str(
            r#"{
            "exprType": "Value",
            "value": true
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Value {
                value: Value::Bool(true)
            }
        ));
    }

    #[test]
    fn test_value_parsing_u64() {
        let expr = serde_json::from_str(
            r#"{
            "exprType": "Value",
            "value": 42
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Value {
                value: Value::U64(42)
            }
        ));
    }

    #[test]
    fn test_value_parsing_i64() {
        let expr = serde_json::from_str(
            r#"{
            "exprType": "Value",
            "value": -42
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Value {
                value: Value::I64(-42)
            }
        ));
    }

    #[test]
    fn test_value_parsing_f64() {
        let expr = serde_json::from_str(
            r#"{
            "exprType": "Value",
            "value": 42.0
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Value {
                value: Value::F64(_)
            }
        ));
        if let Expr::Value {
            value: Value::F64(v),
        } = expr
        {
            assert_eq!(v, 42.0);
        } else {
            panic!("failed to match the value");
        }
    }

    #[test]
    fn test_value_parsing_string() {
        let expr: Expr = serde_json::from_str(
            r#"{
            "exprType": "Value",
            "value": "I'm the best value"
        }"#,
        )
        .unwrap();

        assert!(matches!(
            expr,
            Expr::Value {
                value: Value::String(_)
            }
        ));
        if let Expr::Value {
            value: Value::String(v),
        } = expr
        {
            assert_eq!(v, "I'm the best value");
        } else {
            panic!("failed to match the value");
        }
    }

    #[test]
    fn test_value_parsing_null() {
        let expr = serde_json::from_str(
            r#"{
            "exprType": "Value",
            "value": null
        }"#,
        )
        .unwrap();

        assert!(matches!(expr, Expr::Value { value: Value::Null }));
    }

    #[test]
    #[should_panic(expected = "missing field `value`")]
    fn test_value_parsing_value_missing_panic() {
        let _expr: Expr = serde_json::from_str(
            r#"{
            "exprType": "Value"
        }"#,
        )
        .unwrap();
    }
}
