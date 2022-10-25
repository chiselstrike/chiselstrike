use indexmap::IndexMap;
use once_cell::sync::Lazy;
use serde::{Serialize, Deserialize, ser, de};
use std::fmt;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Database schema as defined by the user.
///
/// This describes the abstract TypeScript types, not how we actually store them in the database.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema {
    /// All entities, either defined by the user or builtin.
    #[serde(with = "schema_entities")]
    pub entities: HashMap<EntityName, Arc<Entity>>,
    /// Named types (see [`Type::Typedef`]).
    #[serde(with = "schema_typedefs")]
    #[serde(default)]
    pub typedefs: HashMap<TypeName, Arc<Type>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntityName {
    User(String),
    Builtin(String),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub name: EntityName,
    /// Type of the `id` field.
    pub id_type: IdType,
    /// Information about all non-`id` fields.
    #[serde(with = "entity_fields")]
    pub fields: IndexMap<String, Arc<EntityField>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: Arc<Type>,
    /// True for fields declared with `?` in TypeScript. If true, then the [`type_`][Self::type_]
    /// must be [`Type::Optional`].
    #[serde(default)]
    pub optional: bool,
    /// Default value when the field is not stored in the database. Note that this does *not* make
    /// the field optional.
    #[serde(default)]
    pub default: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeName {
    /// Module specifier (URL) where the type was declared.
    pub module: String,
    /// Name of the declared type.
    pub name: String,
}

#[derive(Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Type {
    /// Transparent reference to a named type defined in [`Schema::typedefs`]. Note that this means
    /// that types may be recursive.
    Typedef(TypeName),
    /// A reference to another entity `E`
    Ref(EntityName, RefKind),
    /// A primitive type.
    Primitive(PrimitiveType),
    /// `T | undefined` from TypeScript.
    Optional(Arc<Type>),
    /// `Array<T>` (or `T[]`) from TypeScript.
    Array(Arc<Type>),
    /// An object type ("struct") from TypeScript.
    Object(Arc<ObjectType>),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefKind {
    /// `Id<E>`, a lazy reference
    Id,
    /// `E`, an eager reference
    Eager,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrimitiveType {
    /// `string`
    String,
    /// `number` (double precision float, `f64)
    Number,
    /// `boolean`
    Boolean,
    /// UUID, represented as a JavaScript `string` at the moment.
    Uuid,
    /// A JavaScript `Date` object.
    JsDate,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IdType {
    Uuid,
    String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectType {
    #[serde(with = "object_type_fields")]
    pub fields: IndexMap<String, Arc<ObjectField>>,
}

#[derive(Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: Arc<Type>,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Value {
    String(String),
    Number(NumberValue),
    Undefined,
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum NumberValue {
    NegInf,
    Finite(serde_json::Number),
    PosInf,
}

impl IdType {
    pub fn as_primitive_type(self) -> PrimitiveType {
        match self {
            Self::Uuid => PrimitiveType::Uuid,
            Self::String => PrimitiveType::String,
        }
    }
}

impl NumberValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Finite(num) => num.as_f64(),
            Self::NegInf => Some(f64::NEG_INFINITY),
            Self::PosInf => Some(f64::INFINITY),
        }
    }
}

pub static TYPE_STRING: Lazy<Arc<Type>> = Lazy::new(|| Arc::new(Type::Primitive(PrimitiveType::String)));
pub static TYPE_NUMBER: Lazy<Arc<Type>> = Lazy::new(|| Arc::new(Type::Primitive(PrimitiveType::Number)));
pub static TYPE_BOOLEAN: Lazy<Arc<Type>> = Lazy::new(|| Arc::new(Type::Primitive(PrimitiveType::Boolean)));
pub static TYPE_UUID: Lazy<Arc<Type>> = Lazy::new(|| Arc::new(Type::Primitive(PrimitiveType::Uuid)));
pub static TYPE_JS_DATE: Lazy<Arc<Type>> = Lazy::new(|| Arc::new(Type::Primitive(PrimitiveType::JsDate)));

serde_map_as_vec!(mod schema_entities, HashMap<EntityName, Arc<Entity>>, name);
serde_map_as_tuples!(mod schema_typedefs, HashMap<TypeName, Arc<Type>>);
serde_map_as_vec!(mod entity_fields, IndexMap<String, Arc<EntityField>>, name);
serde_map_as_vec!(mod object_type_fields, IndexMap<String, Arc<ObjectField>>, name);

impl Serialize for NumberValue {
    fn serialize<S: ser::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Finite(number) => number.serialize(ser),
            Self::NegInf => ser.serialize_str("negInf"),
            Self::PosInf => ser.serialize_str("posInf"),
        }
    }
}

impl<'de> Deserialize<'de> for NumberValue {
    fn deserialize<D: de::Deserializer<'de>>(deser: D) -> Result<Self, D::Error> {
        struct NumberVisitor;
        impl<'de> de::Visitor<'de> for NumberVisitor {
            type Value = NumberValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(r#"a JSON number, "negInf" or "posInf""#)
            }

            fn visit_i64<E>(self, value: i64) -> Result<NumberValue, E> {
                Ok(NumberValue::Finite(value.into()))
            }

            fn visit_u64<E>(self, value: u64) -> Result<NumberValue, E> {
                Ok(NumberValue::Finite(value.into()))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<NumberValue, E> {
                match serde_json::Number::from_f64(value) {
                    Some(number) => Ok(NumberValue::Finite(number)),
                    None => Err(de::Error::custom("invalid JSON number")),
                }
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<NumberValue, E> {
                match value {
                    "posInf" => Ok(NumberValue::PosInf),
                    "negInf" => Ok(NumberValue::NegInf),
                    _ => Err(de::Error::custom("invalid string encoding number value")),
                }
            }
        }
        deser.deserialize_any(NumberVisitor)
    }
}

impl Hash for ObjectType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.fields.len());
        for field in self.fields.values() {
            field.hash(state);
        }
    }
}
