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
    #[error["unsafe to replace type: {0}"]]
    UnsafeReplacement(String),
}

#[derive(Debug, Default, Clone)]
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
        if self.lookup_type(&ty.name).is_ok() {
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

    pub fn replace_type(&mut self, new_type: ObjectType) -> Result<(), TypeSystemError> {
        let old_type = self.lookup_type(&new_type.name)?;
        if new_type.is_safe_replacement_for(&old_type) {
            self.types.remove(&new_type.name);
            self.types.insert(new_type.name.clone(), new_type);
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

    /// Update the current TypeSystem object from another instance
    pub fn update(&mut self, other: &TypeSystem) {
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

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectType {
    /// Name of this type.
    pub name: String,
    /// Fields of this type.
    pub fields: Vec<Field>,
    /// Name of the backing table for this type.
    pub backing_table: String,
}

impl ObjectType {
    /// True iff self can replace another type in the type system without any changes to the backing table.
    fn is_safe_replacement_for(&self, another_type: &Type) -> bool {
        match another_type {
            Type::Object(another_type) => {
                self.name == another_type.name
                    && self.backing_table == another_type.backing_table
                    && self.fields.len() == another_type.fields.len()
                    && self
                        .fields
                        .iter()
                        .zip(&another_type.fields)
                        .all(|(f1, f2)| f1.name == f2.name && f1.type_ == f2.type_)
            }
            _ => false, // We cannot replace an elemental type.
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub type_: Type,
    pub labels: Vec<String>,
}
