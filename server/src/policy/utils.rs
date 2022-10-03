use std::collections::HashMap;

use boa_engine::object::{JsArray, JsMap};
use boa_engine::prelude::JsObject;
use boa_engine::{JsString, JsValue};
use itertools::Itertools;
use serde_json::{Map, Value as JsonValue};

use crate::datastore::value::{EntityMap, EntityValue};

pub fn json_map_to_js_value(
    ctx: &mut boa_engine::Context,
    map: &Map<String, JsonValue>,
) -> JsValue {
    let obj = JsMap::new(ctx);
    for (k, v) in map.iter() {
        obj.set(JsString::from(k.as_str()), json_to_js_value(ctx, v), ctx)
            .unwrap();
    }

    JsValue::Object(JsObject::from(obj))
}

pub fn json_to_js_value(ctx: &mut boa_engine::Context, json: &JsonValue) -> JsValue {
    match json {
        JsonValue::Null => JsValue::Null,
        JsonValue::Bool(b) => JsValue::Boolean(*b),
        JsonValue::Number(n) if n.is_u64() => JsValue::Integer(n.as_u64().unwrap() as i32),
        JsonValue::Number(n) if n.is_i64() => JsValue::Integer(n.as_i64().unwrap() as i32),
        JsonValue::Number(n) if n.is_f64() => JsValue::Rational(n.as_f64().unwrap() as f64),
        JsonValue::String(s) => JsValue::String(JsString::new(s)),
        JsonValue::Array(arr) => {
            let obj = JsArray::new(ctx);
            for val in arr.iter() {
                let val = json_to_js_value(ctx, val);
                obj.push(val, ctx).unwrap();
            }

            JsValue::Object(JsObject::from(obj))
        }
        JsonValue::Object(ref map) => json_map_to_js_value(ctx, map),
        _ => unreachable!(),
    }
}

pub fn entity_value_to_js_value(
    ctx: &mut boa_engine::Context,
    val: &EntityValue,
    writable: bool,
) -> JsValue {
    match val {
        EntityValue::Null => JsValue::Null,
        EntityValue::String(s) => JsValue::String(JsString::new(s)),
        EntityValue::Float64(f) => JsValue::Rational(*f),
        EntityValue::Boolean(b) => JsValue::Boolean(*b),
        // TODO: v8 and boa handle date quite differently, need to think how to handle those.
        EntityValue::JsDate(_) => todo!("dates not yet handled"),
        EntityValue::Array(arr) => {
            let obj = JsArray::new(ctx);
            for val in arr.iter() {
                obj.push(entity_value_to_js_value(ctx, val, writable), ctx)
                    .unwrap();
            }

            JsValue::Object(JsObject::from(obj))
        }
        EntityValue::Map(map) => entity_map_to_js_value(ctx, map, writable),
    }
}

pub fn entity_map_to_js_value(
    ctx: &mut boa_engine::Context,
    map: &EntityMap,
    writable: bool,
) -> JsValue {
    let object = JsMap::new(ctx);

    for (prop, value) in map.iter() {
        object
            .set(
                JsString::new(prop),
                entity_value_to_js_value(ctx, value, writable),
                ctx,
            )
            .unwrap();
    }

    JsValue::Object(JsObject::from(object))
}

pub fn js_value_to_entity_value(val: &JsValue) -> EntityValue {
    match val {
        JsValue::Null => EntityValue::Null,
        JsValue::Undefined => EntityValue::Null,
        JsValue::Boolean(b) => EntityValue::Boolean(*b),
        JsValue::String(ref s) => EntityValue::String(s.to_string()),
        JsValue::Rational(f) => EntityValue::Float64(*f),
        JsValue::Integer(n) => EntityValue::Float64(*n as _),
        JsValue::BigInt(_) => todo!("big int not supported"),
        JsValue::Object(ref o) if o.borrow().is_date() => todo!("handle date"),
        JsValue::Object(ref o) => {
            let o = o.borrow();

            if let Some(js_map) = o.as_map_ref() {
                let mut map = HashMap::new();
                for (k, v) in js_map.iter() {
                    map.insert(
                        k.as_string().unwrap().to_string(),
                        js_value_to_entity_value(v),
                    );
                }

                EntityValue::Map(map)
            } else if o.is_array() {
                let arr = o
                    .properties()
                    .index_properties()
                    .sorted_by_key(|(i, _)| *i)
                    .map(|(_, desc)| {
                        desc.value()
                            .map(js_value_to_entity_value)
                            .unwrap_or(EntityValue::Null)
                    })
                    .collect();

                EntityValue::Array(arr)
            } else {
                panic!("unexpected object type");
            }
        }
        JsValue::Symbol(_) => todo!(),
    }
}

#[cfg(test)]
mod test {
    use crate::datastore::value::EntityValue;

    use super::*;

    #[test]
    fn test_convert_array_round_trip() {
        let mut ctx = boa_engine::Context::default();
        let entity = EntityValue::Array(vec![
            EntityValue::Boolean(true),
            EntityValue::Boolean(false),
        ]);

        let js_value = entity_value_to_js_value(&mut ctx, &entity, true);
        let entity_back = js_value_to_entity_value(&js_value);
        assert_eq!(entity, entity_back);
    }
}
