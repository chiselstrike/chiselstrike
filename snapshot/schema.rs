use indexmap::IndexMap;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Database schema as defined by the user.
///
/// This describes the abstract TypeScript types, not how we actually store them in the database.
#[derive(Debug, Serialize, Deserialize)]
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
    pub type_: Arc<Type>,
    /// True for fields declared with `?` in TypeScript. If true, then the [`type_`][Self::type_]
    /// must be [`Type::Optional`].
    pub optional: bool,
    /// Default value when the field is not stored in the database. Note that this does *not* make
    /// the field optional.
    pub default: Option<Value>,
    /// Should every instance of the entity have unique value for this field?
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeName {
    /// Module specifier (URL) where the type was declared.
    pub module: String,
    /// Name of the declared type.
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Type {
    /// Transparent reference to a named type defined in [`Schema::typedefs`]. Note that this means
    /// that types may be circular.
    Typedef(TypeName),
    /// `Id<E>` from TypeScript: an identifier of another entity.
    Id(EntityName),
    /// `E` from TypeScript: a reference to another entity, loaded eagerly.
    EagerRef(EntityName),
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
#[serde(rename_all = "PascalCase")]
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
#[serde(rename_all = "PascalCase")]
pub enum IdType {
    Uuid,
    String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectType {
    #[serde(with = "object_type_fields")]
    pub fields: IndexMap<String, Arc<ObjectField>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectField {
    pub name: String,
    pub type_: Arc<Type>,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Value {
    String(String),
    Number(f64),
    Undefined,
}

impl IdType {
    pub fn as_primitive_type(self) -> PrimitiveType {
        match self {
            Self::Uuid => PrimitiveType::Uuid,
            Self::String => PrimitiveType::String,
        }
    }
}

serde_map_as_vec!(mod schema_entities, HashMap<EntityName, Arc<Entity>>, name);
serde_map_as_tuples!(mod schema_typedefs, HashMap<TypeName, Arc<Type>>);
serde_map_as_vec!(mod entity_fields, IndexMap<String, Arc<EntityField>>, name);
serde_map_as_vec!(mod object_type_fields, IndexMap<String, Arc<ObjectField>>, name);
