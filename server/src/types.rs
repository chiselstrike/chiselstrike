// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum TypeSystemError {
    #[error["Type `{0}` already exists"]]
    TypeAlreadyExists(String),
    #[error["No such type: `{0}`"]]
    NoSuchType(String),
    #[error["`{0}` is not an object type"]]
    NotObjectType(String),
}

#[derive(Debug, Default)]
pub struct TypeSystem {
    pub types: HashMap<String, ObjectType>,
}

impl TypeSystem {
    pub fn new() -> Self {
        TypeSystem {
            types: HashMap::default(),
        }
    }

    pub fn define_type(&mut self, ty: ObjectType) -> Result<(), TypeSystemError> {
        if self.types.contains_key(&ty.name) {
            return Err(TypeSystemError::TypeAlreadyExists(ty.name));
        }
        self.types.insert(ty.name.to_owned(), ty);
        Ok(())
    }

    pub fn remove_type(&mut self, type_name: &str) -> Result<(), TypeSystemError> {
        if !self.types.contains_key(type_name) {
            return Err(TypeSystemError::NoSuchType(type_name.to_string()));
        }
        self.types.remove(type_name);
        Ok(())
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
    pub fn lookup_object_type(&self, type_name: &str) -> Result<ObjectType, TypeSystemError> {
        match self.lookup_type(type_name) {
            Ok(Type::Object(ty)) => Ok(ty),
            Ok(_) => Err(TypeSystemError::NotObjectType(type_name.to_string())),
            Err(e) => Err(e),
        }
    }

    pub fn lookup_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
        match type_name {
            "String" => Ok(Type::String),
            type_name => match self.types.get(type_name) {
                Some(ty) => Ok(Type::Object(ty.to_owned())),
                None => Err(TypeSystemError::NoSuchType(type_name.to_string())),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub enum Type {
    String,
    Object(ObjectType),
}

impl Type {
    pub fn name(&self) -> &str {
        match self {
            Type::String => "String",
            Type::Object(ty) => &ty.name,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ObjectType {
    /// Name of this type.
    pub name: String,
    /// Fields of this type.
    pub fields: Vec<Field>,
    /// Name of the backing table for this type.
    pub backing_table: String,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub type_: Type,
    pub labels: Vec<String>,
}
