///! Symbol table.
use std::collections::HashSet;

/// Symbol table.
#[derive(Clone, Default)]
pub struct Symbols {
    entities: HashSet<String>,
}

impl Symbols {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_entity(&mut self, type_name: &str) {
        self.entities.insert(type_name.to_string());
    }

    pub fn is_entity(&self, type_name: &str) -> bool {
        self.entities.contains(type_name)
    }
}
