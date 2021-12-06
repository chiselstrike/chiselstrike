// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

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

    pub(crate) fn replace_type(
        &mut self,
        old_type: &ObjectType,
        new_type: Arc<ObjectType>,
    ) -> Result<ObjectDelta, TypeSystemError> {
        if old_type.name != new_type.name || old_type.backing_table != new_type.backing_table {
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

#[derive(Debug)]
pub(crate) struct ObjectType {
    pub(crate) id: Option<i32>,
    /// Name of this type.
    pub(crate) name: String,
    /// Fields of this type.
    pub(crate) fields: Vec<Field>,
    /// Name of the backing table for this type.
    pub(crate) backing_table: String,
}

impl PartialEq for ObjectType {
    fn eq(&self, another: &Self) -> bool {
        self.name == another.name && self.backing_table == another.backing_table
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
