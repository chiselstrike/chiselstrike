// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use super::{Entity, Field, InternalObject, ObjectType, Type, TypeId};
use crate::auth::{AUTH_ACCOUNT_NAME, AUTH_SESSION_NAME, AUTH_TOKEN_NAME, AUTH_USER_NAME};
use crate::datastore::QueryEngine;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct BuiltinTypes {
    pub types: HashMap<String, Type>,
}

impl BuiltinTypes {
    pub fn new() -> Self {
        let mut types = HashMap::new();
        types.insert("string".into(), Type::String);
        types.insert("number".into(), Type::Float);
        types.insert("boolean".into(), Type::Boolean);
        types.insert("jsDate".into(), Type::JsDate);
        types.insert("ArrayBuffer".into(), Type::ArrayBuffer);
        add_auth_entity(
            &mut types,
            AUTH_USER_NAME,
            vec![
                optional_string_field("emailVerified"),
                optional_string_field("name"),
                optional_string_field("email"),
                optional_string_field("image"),
            ],
            "auth_user",
        );
        add_auth_entity(
            &mut types,
            AUTH_SESSION_NAME,
            vec![
                string_field("sessionToken"),
                string_field("userId"),
                string_field("expires"),
            ],
            "auth_session",
        );
        add_auth_entity(
            &mut types,
            AUTH_TOKEN_NAME,
            vec![
                string_field("identifier"),
                string_field("expires"),
                string_field("token"),
            ],
            "auth_token",
        );
        add_auth_entity(
            &mut types,
            AUTH_ACCOUNT_NAME,
            vec![
                string_field("providerAccountId"),
                string_field("userId"),
                string_field("provider"),
                string_field("type"),
                optional_string_field("access_token"),
                optional_string_field("token_type"),
                optional_string_field("id_token"),
                optional_string_field("refresh_token"),
                optional_string_field("scope"),
                optional_string_field("session_state"),
                optional_string_field("oauth_token_secret"),
                optional_string_field("oauth_token"),
                optional_number_field("expires_at"),
            ],
            "auth_account",
        );

        Self { types }
    }

    pub async fn create_backing_tables(&self, query_engine: &QueryEngine) -> anyhow::Result<()> {
        let mut transaction = query_engine.begin_transaction().await?;
        for ty in self.types.values() {
            if let Type::Entity(ty) = ty {
                query_engine.create_table(&mut transaction, ty).await?;
            }
        }
        QueryEngine::commit_transaction(transaction).await?;
        Ok(())
    }
}

fn add_auth_entity(
    types: &mut HashMap<String, Type>,
    type_name: &'static str,
    fields: Vec<Field>,
    backing_table: &'static str,
) {
    types.insert(type_name.into(), {
        let desc = InternalObject {
            name: type_name,
            backing_table,
        };
        Entity::Auth(Arc::new(ObjectType::new(&desc, fields, vec![]).unwrap())).into()
    });
}

fn string_field(name: &str) -> Field {
    Field {
        id: None,
        name: name.into(),
        type_id: TypeId::String,
        labels: vec![],
        default: None,
        effective_default: None,
        is_optional: false,
        version_id: "__chiselstrike".into(),
        is_unique: false,
    }
}

fn optional_string_field(name: &str) -> Field {
    let mut f = string_field(name);
    f.is_optional = true;
    f
}

fn optional_number_field(name: &str) -> Field {
    Field {
        id: None,
        name: name.into(),
        type_id: TypeId::Float,
        labels: vec![],
        default: None,
        effective_default: None,
        is_optional: true,
        version_id: "__chiselstrike".into(),
        is_unique: false,
    }
}
