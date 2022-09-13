// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::query::Filter;
use indexmap::IndexSet;
use serde::Serialize;
use swc_ecmascript::ast::{ObjectLit, Prop, PropName, PropOrSpread};

/// Filter properties.
///
/// This struct defines entity properties used in a ChiselStrike query API filter call.
/// The information is used by the ChiselStrike runtime for auto-indexing.
#[derive(Debug, Serialize)]
pub struct FilterProperties {
    pub entity_name: String,
    pub properties: Vec<String>,
}

impl FilterProperties {
    pub fn from_filter(entity_name: String, filter: &Filter) -> Option<Self> {
        let properties = filter.properties();
        if !properties.is_empty() {
            Some(FilterProperties {
                entity_name,
                properties: properties.iter().cloned().collect(),
            })
        } else {
            None
        }
    }

    pub fn from_object_lit(entity_name: String, object_lit: &ObjectLit) -> Option<Self> {
        let mut properties = IndexSet::new();
        for prop in &object_lit.props {
            match prop {
                PropOrSpread::Prop(prop) => match &**prop {
                    Prop::KeyValue(key_value_prop) => {
                        if let Some(prop_name) = prop_name_to_string(&key_value_prop.key) {
                            properties.insert(prop_name);
                        }
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
        if !properties.is_empty() {
            Some(FilterProperties {
                entity_name,
                properties: properties.iter().cloned().collect(),
            })
        } else {
            None
        }
    }
}

fn prop_name_to_string(prop_name: &PropName) -> Option<String> {
    match prop_name {
        PropName::Ident(ident) => Some(ident.sym.to_string()),
        PropName::Str(s) => Some(s.value.to_string()),
        _ => None,
    }
}
