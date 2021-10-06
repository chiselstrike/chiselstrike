// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum TypeSystemError {
    #[error["type already exists"]]
    TypeAlreadyExists,
    #[error["no such type"]]
    NoSuchType,
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
            return Err(TypeSystemError::TypeAlreadyExists);
        }
        self.types.insert(ty.name.to_owned(), ty);
        Ok(())
    }

    pub fn lookup_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
        match type_name {
            "String" => Ok(Type::String),
            type_name => match self.types.get(type_name) {
                Some(ty) => Ok(Type::Object(ty.to_owned())),
                None => Err(TypeSystemError::NoSuchType),
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
    pub name: String,
    pub fields: Vec<(String, Type)>,
}
