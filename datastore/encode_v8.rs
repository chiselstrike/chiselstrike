use anyhow::{Result, Context, anyhow, bail};
use chisel_snapshot::schema;
use sqlx::Arguments;
use crate::layout;

pub fn encode_id_to_sql<'s>(
    id_col: &layout::IdColumn,
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
    out_args: &mut sqlx::any::AnyArguments,
) -> Result<()> {
    match id_col.repr {
        layout::IdRepr::UuidAsString =>
            out_args.add(as_string_lossy(scope, value, "uuid id")?),
    }
    Ok(())
}

pub fn encode_field_to_sql<'s>(
    schema: &schema::Schema,
    table: &layout::EntityTable,
    field_col: &layout::FieldColumn,
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
    out_args: &mut sqlx::any::AnyArguments,
) -> Result<()> {
    let entity = &schema.entities[&table.entity_name];
    let field = &entity.fields[&field_col.field_name];
    if field.optional && value.is_null_or_undefined() {
        out_args.add(Option::<String>::None);
        return Ok(())
    }

    match field_col.repr {
        layout::ColumnRepr::StringAsText =>
            out_args.add(as_string_lossy(scope, value, "string field")?),
        layout::ColumnRepr::NumberAsDouble =>
            out_args.add(as_f64(scope, value, "number field")?),
        layout::ColumnRepr::BooleanAsInt =>
            out_args.add(as_bool(value, "boolean field")?),
        layout::ColumnRepr::UuidAsText =>
            out_args.add(as_string_lossy(scope, value, "Uuid field")?),
        layout::ColumnRepr::JsDateAsDouble =>
            out_args.add(as_js_date(value, "JsDate field")?),
        layout::ColumnRepr::AsJsonText => {
            let json = encode_to_json(schema, &field.type_, scope, value)
                .context("could not convert JS value to JSON (field)")?;
            let json_str = serde_json::to_string(&json).expect("could not serialize JSON");
            out_args.add(json_str);
        },
    }
    Ok(())
}

pub fn encode_to_json<'s>(
    schema: &schema::Schema,
    type_: &schema::Type,
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
) -> Result<serde_json::Value> {
    // TODO: a malicious user can overflow our call stack with a deeply nested data structure. we
    // should rewrite this function not to be recursive.
    match type_ {
        schema::Type::Typedef(type_name) => {
            let type_ = schema.typedefs.get(type_name).unwrap();
            encode_to_json(schema, type_, scope, value)
        },
        schema::Type::Id(entity_name) | schema::Type::EagerRef(entity_name) => {
            let entity = schema.entities.get(entity_name).unwrap();
            encode_primitive_to_json(entity.id_type.as_primitive_type(), scope, value)
        },
        schema::Type::Primitive(type_) =>
            encode_primitive_to_json(*type_, scope, value),
        schema::Type::Optional(type_) => {
            if value.is_null_or_undefined() {
                Ok(serde_json::Value::Null)
            } else {
                encode_to_json(schema, type_, scope, value)
            }
        },
        schema::Type::Array(elem_type) => {
            let array = as_array(value, "array JSON value")?;
            let len = array.length();
            let mut json_array = Vec::with_capacity(len as usize);
            for i in 0..len {
                let elem_value = array.get_index(scope, i)
                    .unwrap_or_else(|| v8::undefined(scope).into());
                json_array.push(encode_to_json(schema, elem_type, scope, elem_value)?);
            }
            Ok(serde_json::Value::Array(json_array))
        },
        schema::Type::Object(object_type) => {
            let obj = as_object(value, "object JSON value")?;
            let mut json_map = serde_json::Map::new();
            for field in object_type.fields.values() {
                let key = v8::String::new(scope, &field.name).unwrap();
                match obj.get(scope, key.into()) {
                    Some(field_value) => {
                        let json_field = encode_to_json(schema, &field.type_, scope, field_value)?;
                        json_map.insert(field.name.clone(), json_field);
                    },
                    None if !field.optional =>
                        bail!("required field {:?} in JS object", field.name),
                    None => (),
                }
            }
            Ok(serde_json::Value::Object(json_map))
        },
    }
}

fn encode_primitive_to_json<'s>(
    type_: schema::PrimitiveType,
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
) -> Result<serde_json::Value> {
    Ok(match type_ {
        schema::PrimitiveType::String =>
            as_string_lossy(scope, value, "string JSON value")?.into(),
        schema::PrimitiveType::Number =>
            as_f64(scope, value, "number JSON value")?.into(),
        schema::PrimitiveType::Boolean =>
            as_bool(value, "boolean JSON value")?.into(),
        schema::PrimitiveType::Uuid =>
            as_string_lossy(scope, value, "Uuid JSON value")?.into(),
        schema::PrimitiveType::JsDate =>
            as_js_date(value, "JsDate JSON value")?.into(),
    })
}

fn as_string_lossy<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
    what: &'static str,
) -> Result<String> {
    match v8::Local::<v8::String>::try_from(value) {
        Ok(value) => Ok(value.to_rust_string_lossy(scope)),
        Err(_) => bail!("expected a JS string ({})", what),
    }
}

fn as_f64<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: v8::Local<'s, v8::Value>,
    what: &'static str,
) -> Result<f64> {
    match value.number_value(scope) {
        Some(x) => Ok(x),
        None => bail!("expected a JS number ({})", what),
    }
}

fn as_bool<'s>(value: v8::Local<'s, v8::Value>, what: &'static str) -> Result<bool> {
    if value.is_true() {
        Ok(true)
    } else if value.is_false() {
        Ok(false)
    } else {
        bail!("expected a JS true or false ({})", what)
    }
}

fn as_js_date<'s>(
    value: v8::Local<'s, v8::Value>,
    what: &'static str,
) -> Result<f64> {
    match v8::Local::<v8::Date>::try_from(value) {
        Ok(value) => Ok(value.value_of()),
        Err(_) => bail!("expected a JS Date ({})", what),
    }
}

fn as_array<'s>(
    value: v8::Local<'s, v8::Value>,
    what: &'static str,
) -> Result<v8::Local<'s, v8::Array>> {
    v8::Local::<v8::Array>::try_from(value)
        .map_err(|_| anyhow!("expected a JS Array ({})", what))
}

fn as_object<'s>(
    value: v8::Local<'s, v8::Value>,
    what: &'static str,
) -> Result<v8::Local<'s, v8::Object>> {
    v8::Local::<v8::Object>::try_from(value)
        .map_err(|_| anyhow!("expected a JS Object ({})", what))
}
