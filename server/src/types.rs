// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum TypeSystemError {
    #[error["type already exists"]]
    TypeAlreadyExists,
    #[error["no such type"]]
    NoSuchType,
    #[error["compound type expected, got string instead"]]
    TypeMustBeCompound,
}

pub type Policies = HashMap<String, &'static dyn Fn(Value) -> Value>;

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

    pub fn remove_type(&mut self, type_name: &str) -> Result<(), TypeSystemError> {
        if !self.types.contains_key(type_name) {
            return Err(TypeSystemError::NoSuchType);
        }
        self.types.remove(type_name);
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

    /// Adds the current policies of ty to policies.
    pub fn get_policies(&self, ty: &ObjectType, policies: &mut Policies) {
        for f in &ty.fields {
            // TODO: Read the policies from the metadatabase.
            if f.labels.contains(&"pii".into()) {
                policies.insert(f.name.clone(), &anonymize);
            }
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

fn anonymize(_: Value) -> Value {
    // TODO: use type-specific anonymization.
    json!("xxxxx")
}
