/// A type system specifies zero or more types, and their relations.
#[derive(Debug)]
pub struct TypeSystemDef {
    /// Definitions.
    pub defs: Vec<TypeDef>,
}

impl TypeSystemDef {
    pub fn new(defs: Vec<TypeDef>) -> Self {
        TypeSystemDef { defs }
    }
}

/// A type definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeDef {
    /// Name of the type.
    pub name: String,
    /// Fields of the type.
    pub fields: Vec<FieldDef>,
}

/// A type field definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldDef {
    pub name: String,
    pub ty: String,
}

impl FieldDef {
    pub fn new(name: String, ty: String) -> Self {
        FieldDef { name, ty }
    }
}
