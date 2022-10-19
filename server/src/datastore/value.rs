use anyhow::{bail, Context as _, Result};
use deno_core::v8;
use serde::{Deserialize, Serialize};
use serde_with::base64::Base64;
use serde_with::serde_as;
use std::collections::HashMap;

#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum EntityValue {
    Null,
    String(String),
    Float64(f64),
    Int64(i64),
    Boolean(bool),
    /// Representation of JavaScript's Date represented as UNIX timestamp,
    /// specifically it's the number of milliseconds since epoch.
    JsDate(f64),
    /// Used to represent binary blobs of data. Corresponds to JS's ArrayBuffer.
    Bytes(#[serde_as(as = "Base64")] Vec<u8>),
    Array(EntityArray),
    Map(EntityMap),
}

pub type EntityArray = Vec<EntityValue>;
pub type EntityMap = HashMap<String, EntityValue>;

impl EntityValue {
    pub fn from_v8<'a>(
        v: &v8::Local<'a, v8::Value>,
        scope: &mut v8::HandleScope<'a>,
    ) -> Result<EntityValue> {
        let value = if v.is_null() {
            EntityValue::Null
        } else if v.is_string() {
            EntityValue::String(v.to_string(scope).unwrap().to_rust_string_lossy(scope))
        } else if v.is_number() {
            EntityValue::Float64(v.to_number(scope).unwrap().value())
        } else if v.is_boolean() {
            EntityValue::Boolean(v.boolean_value(scope))
        } else if v.is_date() {
            let date = v8::Local::<v8::Date>::try_from(*v).unwrap();
            EntityValue::JsDate(date.value_of())
        } else if v.is_array_buffer_view() {
            let view = v8::Local::<v8::ArrayBufferView>::try_from(*v).unwrap();
            let buff = view.buffer(scope).unwrap();
            let bs = buff.get_backing_store();
            let bytes = if let Some(data) = bs.data() {
                let data_ptr = data.as_ptr() as *const u8;
                let bytes_slice = unsafe { std::slice::from_raw_parts(data_ptr, bs.byte_length()) };
                let bytes_slice = &bytes_slice[view.byte_offset()..];
                bytes_slice.to_vec()
            } else {
                vec![]
            };
            EntityValue::Bytes(bytes)
        } else if v.is_array_buffer() {
            let buff = v8::Local::<v8::ArrayBuffer>::try_from(*v).unwrap();
            let bs = buff.get_backing_store();
            let bytes = if let Some(data) = bs.data() {
                let data_ptr = data.as_ptr() as *const u8;
                let bytes_slice = unsafe { std::slice::from_raw_parts(data_ptr, bs.byte_length()) };
                bytes_slice.to_vec()
            } else {
                vec![]
            };
            EntityValue::Bytes(bytes)
        } else if v.is_array() {
            let array = v8::Local::<v8::Array>::try_from(*v).unwrap();
            let mut value_vec = Vec::with_capacity(array.length() as usize);
            for i in 0..array.length() {
                let val = array.get_index(scope, i).unwrap();
                value_vec.push(Self::from_v8(&val, scope)?);
            }
            EntityValue::Array(value_vec)
        } else if v.is_object() {
            let obj = v.to_object(scope).unwrap();
            let prop_names = obj
                .get_own_property_names(scope, Default::default())
                .unwrap();
            let mut record = EntityMap::with_capacity(prop_names.length() as usize);
            for i in 0..prop_names.length() {
                let raw_key = prop_names.get_index(scope, i).unwrap();
                let prop_value = obj.get(scope, raw_key).unwrap();

                // Typescript has an interesting rule of ignoring undefined values
                // in objects/maps when serialized to JSON. With this code, we take the values
                // directly from JS which means we suddenly see the undefined properties. But the
                // code in engine.rs would be confused if we were to send it to it.
                if !prop_value.is_undefined() {
                    let key = v8::Local::<v8::String>::try_from(raw_key)
                        .unwrap()
                        .to_rust_string_lossy(scope);
                    record.insert(key, Self::from_v8(&prop_value, scope)?);
                }
            }
            EntityValue::Map(record)
        } else {
            bail!("trying to decode unsupported v8 value");
        };
        Ok(value)
    }

    pub fn from_json(v: &serde_json::Value) -> Result<EntityValue> {
        let v = match v {
            serde_json::Value::Null => EntityValue::Null,
            serde_json::Value::Bool(v) => EntityValue::Boolean(*v),
            serde_json::Value::Number(v) => {
                EntityValue::Float64(v.as_f64().context("cannot convert json number to f64")?)
            }
            serde_json::Value::String(v) => EntityValue::String(v.to_owned()),
            serde_json::Value::Array(v) => {
                let value_vec: Result<_> = v.iter().map(Self::from_json).collect();
                EntityValue::Array(value_vec?)
            }
            serde_json::Value::Object(v) => {
                let value_map: Result<_> = v
                    .iter()
                    .map(|(key, value)| Self::from_json(value).map(|v| (key.to_owned(), v)))
                    .collect();
                EntityValue::Map(value_map?)
            }
        };
        Ok(v)
    }

    pub fn to_v8<'a>(&self, scope: &mut v8::HandleScope<'a>) -> Result<v8::Local<'a, v8::Value>> {
        let r: v8::Local<'a, v8::Value> = match self {
            Self::Null => v8::null(scope).into(),
            Self::String(v) => v8::String::new(scope, v)
                .context("failed to create v8 string when converting EntityValue to v8")?
                .into(),
            Self::Float64(v) => v8::Number::new(scope, *v).into(),
            Self::Int64(v) => v8::Number::new(scope, *v as f64).into(),
            Self::Boolean(v) => v8::Boolean::new(scope, *v).into(),
            Self::JsDate(v) => v8::Date::new(scope, *v)
                .context("failed to create v8 Date when converting EntityValue to v8")?
                .into(),
            Self::Bytes(v) => {
                let buff = v8::ArrayBuffer::new(scope, v.len());
                let bs = buff.get_backing_store();
                // Will be none if the bytes vector `v` is empty
                if bs.data().is_some() {
                    let data_ptr = bs.data().unwrap().as_ptr() as *mut u8;
                    let bytes_slice =
                        unsafe { std::slice::from_raw_parts_mut(data_ptr, bs.byte_length()) };
                    bytes_slice.copy_from_slice(v);
                }
                buff.into()
            }
            Self::Array(v) => {
                let array = v8::Array::new(scope, v.len() as i32);
                for (i, e) in v.iter().enumerate() {
                    let element = e.to_v8(scope)?;
                    array.set_index(scope, i as u32, element);
                }
                array.into()
            }
            Self::Map(v) => {
                let obj = v8::Object::new(scope);
                for (key, value) in v {
                    let key = v8::String::new(scope, key)
                        .context("unable to create map key v8 string")?;
                    let value = value.to_v8(scope)?;
                    obj.set(scope, key.into(), value);
                }
                obj.into()
            }
        };
        Ok(r)
    }

    pub fn kind_str(&self) -> &str {
        match self {
            Self::Null => "Null",
            Self::String(_) => "String",
            Self::Float64(_) => "Float64",
            Self::Int64(_) => "Int64",
            Self::Boolean(_) => "Boolean",
            Self::JsDate(_) => "JsDate",
            Self::Bytes(_) => "Bytes",
            Self::Array(_) => "Array",
            Self::Map(_) => "Record",
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

macro_rules! define_is_method {
    ($method_name:ident, $typ:ident) => {
        pub fn $method_name(&self) -> bool {
            matches!(self, Self::$typ(_))
        }
    };
}

impl EntityValue {
    define_is_method! {is_string, String}
    define_is_method! {is_f64, Float64}
    define_is_method! {is_i64, Int64}
    define_is_method! {is_boolean, Boolean}
    define_is_method! {is_date, JsDate}
    define_is_method! {is_bytes, Bytes}
    define_is_method! {is_array, Array}
    define_is_method! {is_map, Map}
}

macro_rules! as_copy {
    ($method_name:ident, $variant:ident, $typ:ty) => {
        pub fn $method_name(&self) -> Result<$typ> {
            match self {
                Self::$variant(v) => Ok(*v),
                _ => bail!(
                    "tried to convert entity value to {}, but it is of type {}",
                    stringify!($typ),
                    self.kind_str(),
                ),
            }
        }
    };
}

macro_rules! as_ref {
    ($method_name:ident, $variant:ident, $typ:ty) => {
        pub fn $method_name(&self) -> Result<&$typ> {
            match self {
                Self::$variant(v) => Ok(v),
                _ => bail!(
                    "tried to convert entity value to {}, but it is of type {}",
                    stringify!($typ),
                    self.kind_str(),
                ),
            }
        }
    };
}

impl EntityValue {
    as_ref!(as_str, String, str);
    as_copy!(as_f64, Float64, f64);
    as_copy!(as_i64, Int64, i64);
    as_copy!(as_bool, Boolean, bool);
    as_copy!(as_date, JsDate, f64);
    as_ref!(as_bytes, Bytes, [u8]);
    as_ref!(as_array, Array, EntityArray);
    as_ref!(as_map, Map, EntityMap);
}

fn eq_f64(value: &EntityValue, other: f64) -> bool {
    value.as_f64().map_or(false, |i| i == other)
}

fn eq_bool(value: &EntityValue, other: bool) -> bool {
    value.as_bool().map_or(false, |i| i == other)
}

fn eq_str(value: &EntityValue, other: &str) -> bool {
    value.as_str().map_or(false, |i| i == other)
}

impl PartialEq<str> for EntityValue {
    fn eq(&self, other: &str) -> bool {
        eq_str(self, other)
    }
}

impl<'a> PartialEq<&'a str> for EntityValue {
    fn eq(&self, other: &&str) -> bool {
        eq_str(self, *other)
    }
}

impl PartialEq<EntityValue> for str {
    fn eq(&self, other: &EntityValue) -> bool {
        eq_str(other, self)
    }
}

impl<'a> PartialEq<EntityValue> for &'a str {
    fn eq(&self, other: &EntityValue) -> bool {
        eq_str(other, *self)
    }
}

impl PartialEq<String> for EntityValue {
    fn eq(&self, other: &String) -> bool {
        eq_str(self, other.as_str())
    }
}

impl PartialEq<EntityValue> for String {
    fn eq(&self, other: &EntityValue) -> bool {
        eq_str(other, self.as_str())
    }
}

macro_rules! partialeq_numeric {
    ($eq:ident, $ty:ty) => {
        impl PartialEq<$ty> for EntityValue {
            fn eq(&self, other: &$ty) -> bool {
                $eq(self, *other as _)
            }
        }

        impl PartialEq<EntityValue> for $ty {
            fn eq(&self, other: &EntityValue) -> bool {
                $eq(other, *self as _)
            }
        }

        impl<'a> PartialEq<$ty> for &'a EntityValue {
            fn eq(&self, other: &$ty) -> bool {
                $eq(*self, *other as _)
            }
        }

        impl<'a> PartialEq<$ty> for &'a mut EntityValue {
            fn eq(&self, other: &$ty) -> bool {
                $eq(*self, *other as _)
            }
        }
    };
}

partialeq_numeric!(eq_f64, f32);
partialeq_numeric!(eq_f64, f64);
partialeq_numeric!(eq_bool, bool);
