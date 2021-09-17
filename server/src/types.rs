use std::collections::HashMap;

#[derive(Debug)]
pub struct TypeSystem {
    pub types: HashMap<String, Type>,
}

impl TypeSystem {
    pub fn new() -> Self {
        TypeSystem {
            types: HashMap::default(),
        }
    }

    pub fn define_type(&mut self, ty: Type) {
        self.types.insert(ty.name.to_owned(), ty);
    }
}

#[derive(Debug)]
pub struct Type {
    pub name: String,
}
