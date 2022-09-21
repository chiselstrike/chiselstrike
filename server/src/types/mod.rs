// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

pub use self::builtin::BuiltinTypes;
pub use self::type_system::TypeSystem;
use crate::datastore::query::{truncate_identifier, QueryPlan};
use crate::datastore::QueryEngine;
use crate::policies::EntityPolicy;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;
use uuid::Uuid;

mod builtin;
mod type_system;

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    String,
    Float,
    Boolean,
    Entity(Entity),
    Array(Box<Type>),
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::Float => "number".to_string(),
            Type::String => "string".to_string(),
            Type::Boolean => "boolean".to_string(),
            Type::Entity(ty) => ty.name.to_string(),
            Type::Array(ty) => format!("Array<{}>", ty.name()),
        }
    }
}

impl From<Entity> for Type {
    fn from(entity: Entity) -> Self {
        Type::Entity(entity)
    }
}

#[derive(Clone, Debug)]
pub enum Entity {
    /// User defined Custom entity.
    Custom {
        object: Arc<ObjectType>,
        #[allow(dead_code)]
        policy: Option<EntityPolicy>,
    },
    /// Built-in Auth entity.
    Auth(Arc<ObjectType>),
}

impl Entity {
    /// Checks whether `Entity` is Auth builtin type.
    pub fn is_auth(&self) -> bool {
        matches!(self, Entity::Auth(_))
    }
}

impl Deref for Entity {
    type Target = ObjectType;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Custom { object, .. } | Self::Auth(object) => object,
        }
    }
}

impl PartialEq for Entity {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Entity::Custom { object: o1, .. }, Entity::Custom { object: o2, .. })
            | (Entity::Auth(o1), Entity::Auth(o2)) => o1 == o2,
            _ => false,
        }
    }
}

/// Uniquely describes a representation of a type.
///
/// This is passed as a parameter to [`ObjectType`]'s constructor
/// identifying a type.
///
/// This exists as a trait because types that are created in memory
/// behave slightly differently than types that are persisted to the database.
///
/// For example:
///  * Types that are created in memory don't yet have an ID, since the type ID is assigned at
///    insert time.
///  * Types that are created in memory can pick any string they want for the backing table, but
///    once that is persisted we need to keep referring to that table.
///
/// There are two implementations provided: one used for reading types back from the datastore
/// (mandatory IDs, backing table, etc), and one from generating types in memory.
///
/// There are two situations where types are generated in memory:
///  * Type lookups, to make sure a user-proposed type is compatible with an existing type
///  * Type creation, where a type fails the lookup above (does not exist) and then has to
///    be created.
///
/// In the first, an ID is never needed. In the second, an ID is needed once the type is about
/// to be used. To avoid dealing with mutexes, internal mutability, and synchronization, we just
/// reload the type system after changes are made to the database.
///
/// This may become a problem if a user has many types, but it is simple, robust, and elegant.
pub trait ObjectDescriptor {
    fn name(&self) -> String;
    fn id(&self) -> Option<i32>;
    fn backing_table(&self) -> String;
    fn version_id(&self) -> String;
}

pub struct InternalObject {
    name: &'static str,
    backing_table: &'static str,
}

impl ObjectDescriptor for InternalObject {
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn id(&self) -> Option<i32> {
        None
    }

    fn backing_table(&self) -> String {
        self.backing_table.to_string()
    }

    fn version_id(&self) -> String {
        "__chiselstrike".to_string()
    }
}

pub struct ExistingObject<'a> {
    name: String,
    version_id: String,
    backing_table: &'a str,
    id: i32,
}

impl<'a> ExistingObject<'a> {
    pub fn new(name: &str, backing_table: &'a str, id: i32) -> anyhow::Result<Self> {
        let split: Vec<&str> = name.split('.').collect();

        anyhow::ensure!(
            split.len() == 2,
            "Expected version information as part of the type name. Got {}. Database corrupted?",
            name
        );
        let version_id = split[0].to_owned();
        let name = split[1].to_owned();

        Ok(Self {
            name,
            backing_table,
            version_id,
            id,
        })
    }
}

impl<'a> ObjectDescriptor for ExistingObject<'a> {
    fn name(&self) -> String {
        self.name.to_owned()
    }

    fn id(&self) -> Option<i32> {
        Some(self.id)
    }

    fn backing_table(&self) -> String {
        self.backing_table.to_owned()
    }

    fn version_id(&self) -> String {
        self.version_id.to_owned()
    }
}

pub struct NewObject<'a> {
    name: &'a str,
    backing_table: String, // store at object creation time so consecutive calls to backing_table() return the same value
    version_id: &'a str,
}

impl<'a> NewObject<'a> {
    pub fn new(name: &'a str, version_id: &'a str) -> Self {
        let mut buf = Uuid::encode_buffer();
        let uuid = Uuid::new_v4();
        let backing_table = format!("ty_{}_{}", name, uuid.to_simple().encode_upper(&mut buf));

        Self {
            name,
            version_id,
            backing_table,
        }
    }
}

impl<'a> ObjectDescriptor for NewObject<'a> {
    fn name(&self) -> String {
        self.name.to_owned()
    }

    fn id(&self) -> Option<i32> {
        None
    }

    fn backing_table(&self) -> String {
        self.backing_table.clone()
    }

    fn version_id(&self) -> String {
        self.version_id.to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeId {
    String,
    Float,
    Boolean,
    Id,
    Entity { name: String, version_id: String },
    Array(Box<TypeId>),
}

impl TypeId {
    pub fn name(&self) -> String {
        match self {
            TypeId::Id | TypeId::String => "string".to_string(),
            TypeId::Float => "number".to_string(),
            TypeId::Boolean => "boolean".to_string(),
            TypeId::Entity { ref name, .. } => name.to_string(),
            TypeId::Array(elem_type) => format!("Array<{}>", elem_type.name()),
        }
    }
}

impl From<Type> for TypeId {
    fn from(other: Type) -> Self {
        match other {
            Type::String => Self::String,
            Type::Float => Self::Float,
            Type::Boolean => Self::Boolean,
            Type::Entity(e) => Self::Entity {
                name: e.name().to_string(),
                version_id: e.version_id.clone(),
            },
            Type::Array(elem_type) => {
                let element_type_id: Self = (*elem_type).into();
                Self::Array(Box::new(element_type_id))
            }
        }
    }
}

impl<T> From<T> for TypeId
where
    T: FieldDescriptor,
{
    fn from(other: T) -> Self {
        other.ty().into()
    }
}

impl From<&dyn FieldDescriptor> for TypeId {
    fn from(other: &dyn FieldDescriptor) -> Self {
        other.ty().into()
    }
}

#[derive(Debug)]
pub struct ObjectType {
    /// id of this object in the meta-database. Will be None for objects that are not persisted yet
    pub meta_id: Option<i32>,
    /// Name of this type.
    name: String,
    /// Fields of this type.
    fields: Vec<Field>,
    /// Indexes that are to be created in the database to accelerate queries.
    indexes: Vec<DbIndex>,
    /// user-visible ID of this object.
    chisel_id: Field,
    /// Name of the backing table for this type.
    backing_table: String,

    pub version_id: String,
}

impl ObjectType {
    pub fn new(
        desc: &dyn ObjectDescriptor,
        fields: Vec<Field>,
        indexes: Vec<DbIndex>,
    ) -> anyhow::Result<Self> {
        let backing_table = desc.backing_table();
        let version_id = desc.version_id();

        for field in fields.iter() {
            anyhow::ensure!(
                version_id == field.version_id,
                "Versions of fields don't match: Got {} and {}",
                version_id,
                field.version_id
            );
        }
        for index in &indexes {
            for field_name in &index.fields {
                if field_name == "id" {
                    continue;
                }
                anyhow::ensure!(
                    fields.iter().any(|f| &f.name == field_name),
                    "trying to create an index over field '{}' which is not present on type '{}'",
                    field_name,
                    desc.name()
                );
            }
        }
        let chisel_id = Field {
            id: None,
            name: "id".to_string(),
            type_id: TypeId::Id,
            labels: Vec::default(),
            default: None,
            effective_default: None,
            is_optional: false,
            version_id: "__chiselstrike".into(),
            is_unique: true,
        };

        Ok(Self {
            meta_id: desc.id(),
            name: desc.name(),
            version_id,
            backing_table,
            fields,
            indexes,
            chisel_id,
        })
    }

    pub fn user_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter()
    }

    pub fn all_fields(&self) -> impl Iterator<Item = &Field> {
        std::iter::once(&self.chisel_id).chain(self.fields.iter())
    }

    pub fn has_field(&self, field_name: &str) -> bool {
        self.all_fields().any(|f| f.name == field_name)
    }

    pub fn get_field(&self, field_name: &str) -> Option<&Field> {
        self.all_fields().find(|f| f.name == field_name)
    }

    pub fn backing_table(&self) -> &str {
        &self.backing_table
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn persisted_name(&self) -> String {
        format!("{}.{}", self.version_id, self.name)
    }

    fn check_if_safe_to_populate(&self, source_type: &ObjectType) -> anyhow::Result<()> {
        let source_map: FieldMap<'_> = source_type.into();
        let to_map: FieldMap<'_> = self.into();
        to_map.check_populate_from(&source_map)
    }

    pub fn indexes(&self) -> &Vec<DbIndex> {
        &self.indexes
    }
}

impl PartialEq for ObjectType {
    fn eq(&self, another: &Self) -> bool {
        self.name == another.name && self.version_id == another.version_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbIndex {
    /// Id of this index in the meta database. Before its creation, it will be None.
    pub meta_id: Option<i32>,
    /// Name of the index in database. Before its creation, it will be None.
    backing_table: Option<String>,
    pub fields: Vec<String>,
}

impl DbIndex {
    pub fn new(meta_id: i32, backing_table: String, fields: Vec<String>) -> Self {
        Self {
            meta_id: Some(meta_id),
            backing_table: Some(backing_table),
            fields,
        }
    }

    pub fn new_from_fields(fields: Vec<String>) -> Self {
        Self {
            meta_id: None,
            backing_table: None,
            fields,
        }
    }

    pub fn name(&self) -> Option<String> {
        self.meta_id.map(|id| {
            let name = format!(
                "index_{id}_{}__{}",
                self.backing_table.as_ref().unwrap(),
                self.fields.join("_")
            );
            truncate_identifier(&name).to_owned()
        })
    }
}

#[derive(Debug)]
struct FieldMap<'a> {
    map: BTreeMap<&'a str, &'a Field>,
}

impl<'a> From<&'a ObjectType> for FieldMap<'a> {
    fn from(ty: &'a ObjectType) -> Self {
        let mut map = BTreeMap::new();
        for field in ty.fields.iter() {
            map.insert(field.name.as_str(), field);
        }
        Self { map }
    }
}

impl<'a> FieldMap<'a> {
    /// Similar to is_safe_replacement_for, but will be able to work across backing tables. Useful
    /// when evolving versions
    fn check_populate_from(&self, source_type: &Self) -> anyhow::Result<()> {
        // to -> from, always ok to remove fields, so only loop over self.
        //
        // Adding fields: Ok, if there is a default value or lens
        //
        // Fields in common: Ok if the type is the same, or if there is a lens
        for (name, field) in self.map.iter() {
            if let Some(existing) = source_type.map.get(name) {
                anyhow::ensure!(
                    existing.type_id.name() == field.type_id.name(),
                    "Type name mismatch on field {} ({} -> {}). We don't support that yet, but that's coming soon! 🙏",
                    name, existing.type_id.name(), field.type_id.name()
                );
            } else {
                anyhow::ensure!(
                    field.default.is_none(),
                    "Adding field {} without a trivial default, which is not supported yet",
                    name
                );
            }
        }
        Ok(())
    }
}

/// Uniquely describes a representation of a field.
///
/// See the [`ObjectDescriptor`] trait for details.
/// Situations where a new versus existing field are created are similar.
pub trait FieldDescriptor {
    fn name(&self) -> String;
    fn id(&self) -> Option<i32>;
    fn ty(&self) -> Type;
    fn version_id(&self) -> String;
}

pub struct ExistingField {
    name: String,
    ty_: Type,
    id: i32,
    version_id: String,
}

impl ExistingField {
    pub fn new(name: &str, ty_: Type, id: i32, version_id: &str) -> Self {
        Self {
            name: name.to_owned(),
            ty_,
            id,
            version_id: version_id.to_owned(),
        }
    }
}

impl FieldDescriptor for ExistingField {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn id(&self) -> Option<i32> {
        Some(self.id)
    }

    fn ty(&self) -> Type {
        self.ty_.clone()
    }

    fn version_id(&self) -> String {
        self.version_id.to_owned()
    }
}

pub struct NewField<'a> {
    name: &'a str,
    ty_: Type,
    version_id: &'a str,
}

impl<'a> NewField<'a> {
    pub fn new(name: &'a str, ty_: Type, version_id: &'a str) -> anyhow::Result<Self> {
        Ok(Self {
            name,
            ty_,
            version_id,
        })
    }
}

impl<'a> FieldDescriptor for NewField<'a> {
    fn name(&self) -> String {
        self.name.to_owned()
    }

    fn id(&self) -> Option<i32> {
        None
    }

    fn ty(&self) -> Type {
        self.ty_.clone()
    }

    fn version_id(&self) -> String {
        self.version_id.to_owned()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: Option<i32>,
    pub name: String,
    pub type_id: TypeId,
    pub labels: Vec<String>,
    pub is_optional: bool,
    pub is_unique: bool,
    // We want to keep the default the user gave us so we can
    // return it in `chisel describe`. That's the default that is
    // valid in typescriptland.
    //
    // However when dealing with the database we need to translate
    // that default into something else. One example are booleans,
    // that come to us as either 'true' or 'false', but we store as
    // 0 or 1 in sqlite.
    default: Option<String>,
    effective_default: Option<String>,
    version_id: String,
}

impl Field {
    pub fn new(
        desc: &dyn FieldDescriptor,
        labels: Vec<String>,
        default: Option<String>,
        is_optional: bool,
        is_unique: bool,
    ) -> Self {
        let effective_default = if let Type::Boolean = &desc.ty() {
            default
                .clone()
                .map(|x| if x == "false" { "false" } else { "true" })
                .map(|x| x.to_string())
        } else {
            default.clone()
        };

        Self {
            id: desc.id(),
            name: desc.name(),
            version_id: desc.version_id(),
            type_id: desc.into(),
            labels,
            default,
            effective_default,
            is_optional,
            is_unique,
        }
    }

    pub fn user_provided_default(&self) -> &Option<String> {
        &self.default
    }

    pub fn default_value(&self) -> &Option<String> {
        &self.effective_default
    }

    pub fn generate_value(&self) -> Option<String> {
        match self.type_id {
            TypeId::Id => Some(Uuid::new_v4().to_string()),
            _ => self.default_value().clone(),
        }
    }

    pub fn persisted_name(&self, parent_type_name: &ObjectType) -> String {
        format!(
            "{}.{}.{}",
            self.version_id,
            parent_type_name.name(),
            self.name
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldAttrDelta {
    pub type_id: TypeId,
    pub default: Option<String>,
    pub is_optional: bool,
    pub is_unique: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDelta {
    pub id: i32,
    pub attrs: Option<FieldAttrDelta>,
    pub labels: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectDelta {
    pub added_fields: Vec<Field>,
    pub removed_fields: Vec<Field>,
    pub updated_fields: Vec<FieldDelta>,
    pub added_indexes: Vec<DbIndex>,
    pub removed_indexes: Vec<DbIndex>,
}

#[derive(thiserror::Error, Debug)]
pub enum TypeSystemError {
    #[error["type already exists"]]
    CustomTypeExists(Entity),
    #[error["no such type: {0}"]]
    NoSuchType(String),
    #[error["builtin type expected, got `{0}` instead"]]
    NotABuiltinType(String),
    #[error["user defined custom type expected, got `{0}` instead"]]
    NotACustomType(String),
    #[error["unsafe to replace type: {0}. Reason: {1}"]]
    UnsafeReplacement(String, String),
    #[error["Error while trying to manipulate types: {0}"]]
    InternalServerError(String),
}
