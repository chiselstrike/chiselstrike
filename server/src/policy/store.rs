use std::collections::HashMap;

use super::type_policy::TypePolicy;

#[derive(Debug, Default)]
pub struct Store {
    versions: HashMap<String, TypePolicyStore>,
}

impl Store {
    pub fn get_policy(&self, version: &str, ty_name: &str) -> Option<&TypePolicy> {
        self.versions.get(version).and_then(|p| p.get(ty_name))
    }

    pub fn insert(&mut self, version: String, policies: TypePolicyStore) {
        self.versions.insert(version, policies);
    }
}

#[derive(Debug, Default, Clone)]
pub struct TypePolicyStore {
    policies: HashMap<String, TypePolicy>,
}

impl TypePolicyStore {
    pub fn new() -> Self {
        Self::default()
    }
    fn get(&self, ty_name: &str) -> Option<&TypePolicy> {
        self.policies.get(ty_name)
    }

    pub fn insert(&mut self, ty_name: String, policy: TypePolicy) {
        self.policies.insert(ty_name, policy);
    }
}
