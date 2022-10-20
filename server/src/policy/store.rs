use std::collections::HashMap;

use super::type_policy::TypePolicy;

#[derive(Debug, Default, Clone)]
pub struct PolicyStore {
    policies: HashMap<String, TypePolicy>,
}

impl PolicyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, ty_name: &str) -> Option<&TypePolicy> {
        self.policies.get(ty_name)
    }

    pub fn insert(&mut self, ty_name: String, policy: TypePolicy) {
        self.policies.insert(ty_name, policy);
    }
}
