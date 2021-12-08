// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use derive_new::new;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub(crate) enum TypeSystemError {
    #[error["type already exists"]]
    TypeAlreadyExists(Arc<ObjectType>),
    #[error["no such type: {0}"]]
    NoSuchType(String),
    #[error["object type expected, got `{0}` instead"]]
    ObjectTypeRequired(String),
    #[error["unsafe to replace type: {0}"]]
    UnsafeReplacement(String),
    #[error["Error while trying to manipulate types: {0}"]]
    InternalServerError(String),
}

#[derive(Debug, Default, Clone)]
pub(crate) struct TypeSystem {
    pub(crate) types: HashMap<String, Arc<ObjectType>>,
}

impl TypeSystem {
    pub(crate) fn new() -> Self {
        TypeSystem {
            types: HashMap::default(),
        }
    }

    /// Adds an object type to the type system.
    ///
    /// # Arguments
    ///
    /// * `ty` object to add
    ///
    /// # Errors
    ///
    /// If type `ty` already exists in the type system, the function returns `TypeSystemError`.
    pub(crate) fn add_type(&mut self, ty: Arc<ObjectType>) -> Result<(), TypeSystemError> {
        match self.lookup_object_type(&ty.name) {
            Ok(old) => Err(TypeSystemError::TypeAlreadyExists(old)),
            Err(TypeSystemError::NoSuchType(_)) => Ok(()),
            Err(x) => Err(x),
        }?;
        self.types.insert(ty.name.to_owned(), ty);
        Ok(())
    }

    /// Generate an [`ObjectDelta`] with the necessary information to evolve a specific type.
    pub(crate) fn generate_type_delta(
        old_type: &ObjectType,
        new_type: Arc<ObjectType>,
    ) -> Result<ObjectDelta, TypeSystemError> {
        if *old_type != *new_type {
            return Err(TypeSystemError::UnsafeReplacement(new_type.name.clone()));
        }

        let mut old_fields = FieldMap::from(&*old_type);
        let new_fields = FieldMap::from(&*new_type);

        let mut added_fields = Vec::new();
        let mut removed_fields = Vec::new();
        let mut updated_fields = Vec::new();

        for (name, field) in new_fields.map.iter() {
            match old_fields.map.remove(name) {
                None => {
                    if field.default.is_none() {
                        return Err(TypeSystemError::UnsafeReplacement(new_type.name.clone()));
                    }
                    added_fields.push(field.to_owned().clone());
                }
                Some(old) => {
                    if field.type_ != old.type_ {
                        return Err(TypeSystemError::UnsafeReplacement(new_type.name.clone()));
                    }

                    let attrs = if field.default != old.default
                        || field.type_ != old.type_
                        || field.is_optional != old.is_optional
                    {
                        Some(FieldAttrDelta {
                            type_: field.type_.clone(),
                            default: field.default.clone(),
                            is_optional: field.is_optional,
                        })
                    } else {
                        None
                    };

                    let mut old_labels = old.labels.clone();
                    old_labels.sort();

                    let mut new_labels = field.labels.clone();
                    new_labels.sort();

                    let labels = if old_labels != new_labels {
                        Some(new_labels)
                    } else {
                        None
                    };

                    let id = old.id.ok_or_else(|| {
                        TypeSystemError::InternalServerError(
                            "logical error! updating field without id".to_string(),
                        )
                    })?;
                    updated_fields.push(FieldDelta { id, attrs, labels });
                }
            }
        }

        // only allow the removal of fields that previously had a default value
        for (_, field) in old_fields.map.into_iter() {
            if field.default.is_none() {
                return Err(TypeSystemError::UnsafeReplacement(new_type.name.clone()));
            }
            removed_fields.push(field.to_owned().clone());
        }

        Ok(ObjectDelta {
            added_fields,
            removed_fields,
            updated_fields,
        })
    }

    /// Looks up an object type with name `type_name`.
    ///
    /// # Arguments
    ///
    /// * `type_name` name of object type to look up.
    ///
    /// # Errors
    ///
    /// If the looked up type does not exists or is a built-in type, the function returns a `TypeSystemError`.
    pub(crate) fn lookup_object_type(
        &self,
        type_name: &str,
    ) -> Result<Arc<ObjectType>, TypeSystemError> {
        match self.lookup_type(type_name) {
            Ok(Type::Object(ty)) => Ok(ty),
            Ok(_) => Err(TypeSystemError::ObjectTypeRequired(type_name.to_string())),
            Err(e) => Err(e),
        }
    }

    pub(crate) fn lookup_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
        match type_name {
            "string" => Ok(Type::String),
            "bigint" => Ok(Type::Int),
            "number" => Ok(Type::Float),
            "boolean" => Ok(Type::Boolean),
            type_name => match self.types.get(type_name) {
                Some(ty) => Ok(Type::Object(ty.to_owned())),
                None => Err(TypeSystemError::NoSuchType(type_name.to_owned())),
            },
        }
    }

    /// Update the current TypeSystem object from another instance
    pub(crate) fn update(&mut self, other: &TypeSystem) {
        self.types.clear();
        self.types = other.types.clone();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Type {
    String,
    Int,
    Float,
    Boolean,
    Object(Arc<ObjectType>),
}

impl Type {
    pub(crate) fn name(&self) -> &str {
        match self {
            Type::Float => "number",
            Type::Int => "bigint",
            Type::String => "string",
            Type::Boolean => "boolean",
            Type::Object(ty) => &ty.name,
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
pub(crate) trait ObjectDescriptor {
    fn name(&self) -> String;
    fn id(&self) -> Option<i32>;
    fn backing_table(&self) -> String;
}

#[derive(new)]
pub(crate) struct ExistingObject<'a> {
    name: &'a str,
    backing_table: &'a str,
    id: i32,
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
}

pub(crate) struct NewObject<'a> {
    name: &'a str,
    backing_table: String, // store at object creation time so consecutive calls to backing_table() return the same value
}

impl<'a> NewObject<'a> {
    pub(crate) fn new(name: &'a str) -> Self {
        let mut buf = Uuid::encode_buffer();
        let uuid = Uuid::new_v4();
        let backing_table = format!("ty_{}_{}", name, uuid.to_simple().encode_upper(&mut buf));

        Self {
            name,
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
}

#[derive(Debug)]
pub(crate) struct ObjectType {
    pub(crate) id: Option<i32>,
    /// Name of this type.
    name: String,
    /// Fields of this type.
    pub(crate) fields: Vec<Field>,
    /// Name of the backing table for this type.
    backing_table: String,
}

impl ObjectType {
    pub(crate) fn new<D: ObjectDescriptor>(desc: D, fields: Vec<Field>) -> Self {
        let backing_table = desc.backing_table();
        Self {
            id: desc.id(),
            name: desc.name(),
            backing_table,
            fields,
        }
    }

    pub(crate) fn backing_table(&self) -> &str {
        &self.backing_table
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

impl PartialEq for ObjectType {
    fn eq(&self, another: &Self) -> bool {
        self.name == another.name
    }
}

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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    pub(crate) id: Option<i32>,
    pub(crate) name: String,
    pub(crate) type_: Type,
    pub(crate) labels: Vec<String>,
    pub(crate) default: Option<String>,
    pub(crate) is_optional: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldAttrDelta {
    pub(crate) type_: Type,
    pub(crate) default: Option<String>,
    pub(crate) is_optional: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldDelta {
    pub(crate) id: i32,
    pub(crate) attrs: Option<FieldAttrDelta>,
    pub(crate) labels: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObjectDelta {
    pub(crate) added_fields: Vec<Field>,
    pub(crate) removed_fields: Vec<Field>,
    pub(crate) updated_fields: Vec<FieldDelta>,
}
