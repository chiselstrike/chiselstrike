use boa_engine::{prelude::JsObject, JsValue};
use itertools::Itertools;

/// boa host function to debug values in policies
pub fn debug(
    _: &JsValue,
    args: &[JsValue],
    _ctx: &mut boa_engine::Context,
) -> std::result::Result<JsValue, JsValue> {
    let value = &args[0];
    println!("{}", write_value_to_string(value));
    Ok(JsValue::Null)
}

fn write_value_to_string(value: &JsValue) -> String {
    match value {
        JsValue::Null => "null".to_string(),
        JsValue::Undefined => "undefined".to_string(),
        JsValue::Boolean(true) => "true".to_string(),
        JsValue::Boolean(false) => "false".to_string(),
        JsValue::String(s) => s.as_str().to_string(),
        JsValue::Rational(f) => f.to_string(),
        JsValue::Integer(i) => i.to_string(),
        JsValue::BigInt(bi) => bi.to_string(),
        JsValue::Object(o) => {
            if o.is_array() {
                write_value_array_to_string(o)
            } else if o.is_function() {
                "<function>".to_string()
            } else {
                write_obj_value_to_string(o)
            }
        }
        JsValue::Symbol(s) => format!("<{s}>"),
    }
}

fn write_obj_value_to_string(o: &JsObject) -> String {
    let content = o
        .borrow()
        .properties()
        .string_properties()
        .map(|(k, v)| {
            format!(
                "{k}: {}",
                write_value_to_string(v.value().unwrap_or(&JsValue::Undefined))
            )
        })
        .join(", ");

    format!("{{{content}}}")
}

fn write_value_array_to_string(o: &JsObject) -> String {
    let items = o
        .borrow()
        .properties()
        .index_properties()
        .sorted_by_key(|(i, _)| *i)
        .map(|(_, v)| write_value_to_string(v.value().unwrap_or(&JsValue::Undefined)))
        .join(", ");

    format!("[{items}]")
}
