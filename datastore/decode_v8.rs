//! Converting values from SQL to V8.

use anyhow::{Result, Context, bail};
use chisel_snapshot::schema;
use deno_core::v8;
use sqlx::Row;
use crate::layout;

pub fn decode_id_from_sql<'s>(
    repr: layout::IdRepr,
    scope: &mut v8::HandleScope<'s>,
    row: &sqlx::any::AnyRow,
    row_idx: usize,
) -> Result<v8::Local<'s, v8::Value>> {
    Ok(match repr {
        layout::IdRepr::UuidAsText =>
            to_string(scope, row.try_get(row_idx)?),
        layout::IdRepr::StringAsText =>
            to_string(scope, row.try_get(row_idx)?),
    })
}

pub fn decode_field_from_sql<'s>(
    schema: &schema::Schema,
    repr: layout::FieldRepr,
    type_: &schema::Type,
    scope: &mut v8::HandleScope<'s>,
    row: &sqlx::any::AnyRow,
    row_idx: usize,
) -> Result<v8::Local<'s, v8::Value>> {
    Ok(match repr {
        layout::FieldRepr::StringAsText | layout::FieldRepr::UuidAsText =>
            to_string(scope, row.try_get(row_idx)?),
        layout::FieldRepr::NumberAsDouble =>
            to_number(scope, row.try_get(row_idx)?),
        layout::FieldRepr::BooleanAsInt =>
            to_boolean(scope, row.try_get(row_idx)?),
        layout::FieldRepr::JsDateAsDouble =>
            to_js_date(scope, row.try_get(row_idx)?)?,
        layout::FieldRepr::AsJsonText => {
            let json_str: &str = row.try_get(row_idx)?;
            let json = serde_json::from_str(json_str)
                .context("could not parse JSON")?;
            decode_from_json(schema, type_, scope, &json)?
        },
    })
}

pub fn decode_from_json<'s>(
    schema: &schema::Schema,
    type_: &schema::Type,
    out_scope: &mut v8::HandleScope<'s>,
    json: &serde_json::Value,
) -> Result<v8::Local<'s, v8::Value>> {
    let scope = &mut v8::EscapableHandleScope::new(out_scope);
    let value: v8::Local<v8::Value> = match type_ {
        schema::Type::Typedef(type_name) => {
            let type_ = schema.typedefs.get(type_name).unwrap();
            decode_from_json(schema, type_, scope, json)?
        },
        schema::Type::Id(entity_name) | schema::Type::EagerRef(entity_name) => {
            let entity = schema.entities.get(entity_name).unwrap();
            decode_primitive_from_json(entity.id_type.as_primitive_type(), scope, json)?
        },
        schema::Type::Primitive(type_) =>
            decode_primitive_from_json(*type_, scope, json)?,
        schema::Type::Optional(type_) => {
            if json.is_null() {
                v8::undefined(scope).into()
            } else {
                decode_from_json(schema, type_, scope, json)?
            }
        },
        schema::Type::Array(elem_type) => {
            let json_array = json.as_array().context("expected a JSON array")?;
            let array_scope = &mut v8::EscapableHandleScope::new(scope);
            let array = v8::Array::new(array_scope, json_array.len() as i32);
            for (i, json_elem) in json_array.iter().enumerate() {
                let elem = decode_from_json(schema, elem_type, array_scope, json_elem)?;
                array.set_index(array_scope, i as u32, elem);
            }
            array_scope.escape(array).into()
        },
        schema::Type::Object(object_type) => {
            let json_obj = json.as_object().context("expected a JSON object")?;
            let obj_scope = &mut v8::EscapableHandleScope::new(scope);
            let obj = v8::Object::new(obj_scope);
            for field in object_type.fields.values() {
                match json_obj.get(&field.name) {
                    Some(json_value) => {
                        let key = v8::String::new(obj_scope, &field.name).unwrap();
                        let value = decode_from_json(schema, &field.type_, obj_scope, json_value)?;
                        obj.set(obj_scope, key.into(), value).unwrap();
                    },
                    None if !field.optional =>
                        bail!("expected field {:?} in JSON object", field.name),
                    None => (),
                }
            }
            obj_scope.escape(obj).into()
        },
    };
    Ok(scope.escape(value))
}

fn decode_primitive_from_json<'s>(
    type_: schema::PrimitiveType,
    scope: &mut v8::HandleScope<'s>,
    json: &serde_json::Value,
) -> Result<v8::Local<'s, v8::Value>> {
    Ok(match type_ {
        schema::PrimitiveType::String | schema::PrimitiveType::Uuid =>
            to_string(scope, json.as_str().context("expected a JSON string")?),
        schema::PrimitiveType::Number =>
            to_number(scope, json.as_f64().context("expected a JSON number")?),
        schema::PrimitiveType::Boolean =>
            to_boolean(scope, json.as_bool().context("expected a JSON boolean")?),
        schema::PrimitiveType::JsDate =>
            to_js_date(scope, json.as_f64().context("expected a JSON number (representing a JS Date)")?)?,
    })
}

fn to_string<'s>(scope: &mut v8::HandleScope<'s>, value: &str) -> v8::Local<'s, v8::Value> {
    v8::String::new(scope, value).unwrap().into()
}

fn to_number<'s>(scope: &mut v8::HandleScope<'s>, value: f64) -> v8::Local<'s, v8::Value> {
    v8::Number::new(scope, value).into()
}

fn to_boolean<'s>(scope: &mut v8::HandleScope<'s>, value: bool) -> v8::Local<'s, v8::Value> {
    v8::Boolean::new(scope, value).into()
}

fn to_js_date<'s>(scope: &mut v8::HandleScope<'s>, value: f64) -> Result<v8::Local<'s, v8::Value>> {
    Ok(v8::Date::new(scope, value).context("cannot create JS Date from number")?.into())
}
