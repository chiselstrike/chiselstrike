use crate::query::Filter;
use serde::Serialize;
use indexmap::IndexSet;
use swc_ecmascript::ast::{ObjectLit, Prop, PropName, PropOrSpread};

/// An index.
#[derive(Debug, Serialize)]
pub struct Index {
    pub entity_name: String,
    pub properties: Vec<String>,
}

impl Index {
    pub fn from_filter(entity_name: String, filter: &Filter) -> Self {
        let properties = filter.properties();
        Index {
            entity_name,
            properties: properties.iter().cloned().collect(),
        }
    }

    pub fn from_object_lit(entity_name: String, object_lit: &ObjectLit) -> Self {
        let mut properties = IndexSet::new();
        for prop in &object_lit.props {
            match prop {
                PropOrSpread::Prop(prop) => match &**prop {
                    Prop::KeyValue(key_value_prop) => {
                        properties.insert(prop_name_to_string(&key_value_prop.key));
                    }
                    Prop::Shorthand(ident) => {
                        properties.insert(ident.sym.to_string());
                    }
                    _ => {
                        todo!();
                    }
                },
                _ => {
                    todo!();
                }
            }
        }
        Index {
            entity_name,
            properties: properties.iter().cloned().collect(),
        }
    }
}

fn prop_name_to_string(prop_name: &PropName) -> String {
    match prop_name {
        PropName::Ident(ident) => ident.sym.to_string(),
        PropName::Str(s) => s.value.to_string(),
        _ => {
            todo!();
        }
    }
}
