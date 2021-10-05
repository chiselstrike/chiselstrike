// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum TypeSystemError {
    #[error["type already exists"]]
    TypeAlreadyExists,
}

#[derive(Debug, Default)]
pub struct TypeSystem {
    pub types: HashMap<String, Type>,
}

impl TypeSystem {
    pub fn new() -> Self {
        TypeSystem {
            types: HashMap::default(),
        }
    }

    pub fn define_type(&mut self, ty: Type) -> Result<(), TypeSystemError> {
        if self.types.contains_key(&ty.name) {
            return Err(TypeSystemError::TypeAlreadyExists);
        }
        self.types.insert(ty.name.to_owned(), ty);
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Type {
    pub name: String,
    pub fields: Vec<(String, String)>,
}
