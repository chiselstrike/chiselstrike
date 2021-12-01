// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use convert_case::{Case, Casing};
use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum TypeSystemError {
    #[error["type already exists"]]
    TypeAlreadyExists,
    #[error["no such type: {0}"]]
    NoSuchType(String),
    #[error["no such version: {0}"]]
    NoSuchVersion(String),
    #[error["object type expected, got `{0}` instead"]]
    ObjectTypeRequired(String),
    #[error["unsafe to replace type: {0}"]]
    UnsafeReplacement(String),
}

#[derive(Debug, Default, Clone)]
pub struct TypeSystem {
    pub types: HashMap<String, HashMap<String, ObjectType>>,
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
    pub(crate) fn add_type(&mut self, ty: ObjectType) -> Result<(), TypeSystemError> {
        if self.lookup_type(&ty.name, &ty.api_version).is_ok() {
            return Err(TypeSystemError::TypeAlreadyExists);
        }
        let types_this_version = self
            .types
            .entry(ty.api_version.to_string())
            .or_insert_with(HashMap::new);
        types_this_version.insert(ty.name.to_owned(), ty);
        Ok(())
    }

    pub(crate) fn replace_type(&mut self, new_type: ObjectType) -> Result<(), TypeSystemError> {
        let old_type = self.lookup_type(&new_type.name, &new_type.api_version)?;
        if new_type.is_safe_replacement_for(&old_type) {
            let types_this_version = self.types.get_mut(&new_type.api_version).unwrap(); // if not existent would have errored on lookup
            types_this_version.remove(&new_type.name);
            types_this_version.insert(new_type.name.clone(), new_type);
            Ok(())
        } else {
            Err(TypeSystemError::UnsafeReplacement(new_type.name))
        }
    }

    /// Looks up an object type with name `type_name`.
    ///
    /// # Arguments
    ///
    /// * `type_name` name of object type to look up.
    /// * `api_version` the API version this type belongs to
    ///
    /// # Errors
    ///
    /// If the looked up type does not exists or is a built-in type, the function returns a `TypeSystemError`.
    pub(crate) fn lookup_object_type(
        &self,
        type_name: &str,
        api_version: &str,
    ) -> Result<ObjectType, TypeSystemError> {
        match self.lookup_type(type_name, api_version) {
            Ok(Type::Object(ty)) => Ok(ty),
            Ok(_) => Err(TypeSystemError::ObjectTypeRequired(type_name.to_string())),
            Err(e) => Err(e),
        }
    }

    pub(crate) fn lookup_type(
        &self,
        type_name: &str,
        api_version: &str,
    ) -> Result<Type, TypeSystemError> {
        match type_name {
            "string" => Ok(Type::String),
            "bigint" => Ok(Type::Int),
            "number" => Ok(Type::Float),
            "boolean" => Ok(Type::Boolean),
            type_name => match self.types.get(api_version) {
                None => Err(TypeSystemError::NoSuchVersion(api_version.to_owned())),
                Some(types_this_version) => match types_this_version.get(type_name) {
                    Some(ty) => Ok(Type::Object(ty.to_owned())),
                    None => Err(TypeSystemError::NoSuchType(type_name.to_owned())),
                },
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
pub enum Type {
    String,
    Int,
    Float,
    Boolean,
    Object(ObjectType),
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

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectType {
    /// Name of this type.
    pub(crate) name: String,
    /// Fields of this type.
    pub(crate) fields: Vec<Field>,
    /// version of the api
    pub(crate) api_version: String,

    backing_table: String,
}

impl ObjectType {
    pub(crate) fn new<N, V>(name: N, fields: Vec<Field>, api_version: V) -> Self
    where
        N: Into<String>,
        V: Into<String>,
    {
        let name = name.into();
        let api_version = api_version.into();

        let backing_table = format!("cstype_{}_ty_{}", api_version, name.to_case(Case::Snake));
        Self {
            name,
            fields,
            api_version,
            backing_table,
        }
    }
    pub(crate) fn backing_table(&self) -> &str {
        &self.backing_table
    }

    /// True iff self can replace another type in the type system without any changes to the backing table.
    fn is_safe_replacement_for(&self, another_type: &Type) -> bool {
        match another_type {
            Type::Object(another_type) => {
                let mut fields = self.fields.to_vec();
                fields.sort_by(|f, k| f.name.cmp(&k.name));
                let mut newfields = another_type.fields.to_vec();
                newfields.sort_by(|f, k| f.name.cmp(&k.name));

                self.name == another_type.name
                    && self.backing_table() == another_type.backing_table()
                    && fields.len() == newfields.len()
                    && fields
                        .iter()
                        .zip(&newfields)
                        .all(|(f1, f2)| f1.name == f2.name && f1.type_ == f2.type_)
            }
            _ => false, // We cannot replace an elemental type.
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) type_: Type,
    pub(crate) labels: Vec<String>,
    pub(crate) default: Option<String>,
    pub(crate) is_optional: bool,
}
