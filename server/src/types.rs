// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum TypeSystemError {
    #[error["type already exists"]]
    TypeAlreadyExists,
    #[error["no such type: {0}"]]
    NoSuchType(String),
    #[error["object type expected, got `{0}` instead"]]
    ObjectTypeRequired(String),
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

    /// Adds an object type to the type system.
    ///
    /// # Arguments
    ///
    /// * `ty` object to add
    ///
    /// # Errors
    ///
    /// If type `ty` already exists in the type system, the function returns `TypeSystemError`.
    pub fn add_type(&mut self, ty: ObjectType) -> Result<(), TypeSystemError> {
        if self.types.contains_key(&ty.name) {
            return Err(TypeSystemError::TypeAlreadyExists);
        }
        self.types.insert(ty.name.to_owned(), ty);
        Ok(())
    }

    pub fn remove_type(&mut self, type_name: &str) -> Result<(), TypeSystemError> {
        if !self.types.contains_key(type_name) {
            return Err(TypeSystemError::NoSuchType(type_name.to_owned()));
        }
        self.types.remove(type_name);
        Ok(())
    }

    pub fn type_exists(&self, type_name: &str) -> bool {
        self.types.contains_key(type_name)
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
            Ok(_) => Err(TypeSystemError::ObjectTypeRequired(type_name.to_string())),
            Err(e) => Err(e),
        }
    }

    pub fn lookup_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
        match type_name {
            "String" => Ok(Type::String),
            "Int" => Ok(Type::Int),
            "Float" => Ok(Type::Float),
            "Boolean" => Ok(Type::Boolean),
            type_name => match self.types.get(type_name) {
                Some(ty) => Ok(Type::Object(ty.to_owned())),
                None => Err(TypeSystemError::NoSuchType(type_name.to_owned())),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub enum Type {
    String,
    Int,
    Float,
    Boolean,
    Object(ObjectType),
}

impl Type {
    pub fn name(&self) -> &str {
        match self {
            Type::Float => "Float",
            Type::Int => "Int",
            Type::String => "String",
            Type::Boolean => "Boolean",
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
