use std::fmt::Write as _;

use anyhow::Result;
use boa_engine::{prelude::JsObject, JsValue};
use itertools::Itertools;

/// boa host function to debug values in policies
pub fn debug(
    _: &JsValue,
    args: &[JsValue],
    _ctx: &mut boa_engine::Context,
) -> std::result::Result<JsValue, JsValue> {
    let value = &args[0];
    let mut out = String::new();
    write_value_to_string(value, &mut out).unwrap();

    println!("{out}");
    Ok(JsValue::Null)
}

fn write_value_to_string(value: &JsValue, out: &mut String) -> Result<()> {
    match value {
        JsValue::Null => out.push_str("null"),
        JsValue::Undefined => out.push_str("undefined"),
        JsValue::Boolean(true) => out.push_str("true"),
        JsValue::Boolean(false) => out.push_str("false"),
        JsValue::String(s) => out.push_str(s.as_str()),
        JsValue::Rational(f) => write!(out, "{f}")?,
        JsValue::Integer(i) => write!(out, "{i}")?,
        JsValue::BigInt(bi) => write!(out, "{bi}")?,
        JsValue::Object(o) => {
            if o.is_array() {
                write_value_array_to_string(o, out)?;
            } else if o.is_function() {
                out.push_str("<function>");
            } else {
                write_obj_value_to_string(o, out)?;
            }
        }
        JsValue::Symbol(s) => write!(out, "<{s}>")?,
    }

    Ok(())
}

fn write_obj_value_to_string(o: &JsObject, out: &mut String) -> Result<()> {
    out.push('{');
    o.borrow()
        .properties()
        .string_properties()
        .try_for_each(|(k, v)| -> Result<()> {
            out.push_str(k.as_str());
            out.push_str(": ");
            let v = v.value().unwrap_or(&JsValue::Null);
            write_value_to_string(v, out)?;
            out.push(',');

            Ok(())
        })?;
    out.push('}');

    Ok(())
}

fn write_value_array_to_string(o: &JsObject, out: &mut String) -> Result<()> {
    out.push('[');
    o.borrow()
        .properties()
        .index_properties()
        .sorted_by_key(|(i, _)| *i)
        .try_for_each(|(_, v)| -> Result<()> {
            let v = v.value().unwrap_or(&JsValue::Null);
            write_value_to_string(v, out)?;
            out.push(',');

            Ok(())
        })?;

    out.push(']');

    Ok(())
}
