// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use super::{
    DbIndex, Entity, Field, FieldAttrDelta, FieldDelta, FieldMap, InternalObject, ObjectDelta,
    ObjectType, Type, TypeId,
};
use crate::auth::{AUTH_ACCOUNT_NAME, AUTH_SESSION_NAME, AUTH_TOKEN_NAME, AUTH_USER_NAME};
use crate::datastore::query::QueryPlan;
use crate::datastore::QueryEngine;
use anyhow::Context;
use deno_core::futures;
use derive_new::new;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub(crate) enum TypeSystemError {
    #[error["type already exists"]]
    CustomTypeExists(Entity),
    #[error["no such type: {0}"]]
    NoSuchType(String),
    #[error["no such API version: {0}"]]
    NoSuchVersion(String),
    #[error["builtin type expected, got `{0}` instead"]]
    NotABuiltinType(String),
    #[error["user defined custom type expected, got `{0}` instead"]]
    NotACustomType(String),
    #[error["unsafe to replace type: {0}. Reason: {1}"]]
    UnsafeReplacement(String, String),
    #[error["Error while trying to manipulate types: {0}"]]
    InternalServerError(String),
}

#[derive(Debug, Default, Clone, new)]
pub(crate) struct VersionTypes {
    #[new(default)]
    pub(crate) custom_types: HashMap<String, Entity>,
}

#[derive(Debug, Clone)]
pub(crate) struct TypeSystem {
    pub(crate) versions: HashMap<String, VersionTypes>,
    builtin_types: HashMap<String, Type>,
}

impl VersionTypes {
    pub(crate) fn lookup_custom_type(&self, type_name: &str) -> Result<Entity, TypeSystemError> {
        match self.custom_types.get(type_name) {
            Some(ty) => Ok(ty.to_owned()),
            None => Err(TypeSystemError::NoSuchType(type_name.to_owned())),
        }
    }

    fn add_custom_type(&mut self, ty: Entity) -> Result<(), TypeSystemError> {
        if let Entity::Auth(_) = ty {
            return Err(TypeSystemError::NotACustomType(ty.name().into()));
        }
        match self.lookup_custom_type(&ty.name) {
            Ok(old) => Err(TypeSystemError::CustomTypeExists(old)),
            Err(TypeSystemError::NoSuchType(_)) => Ok(()),
            Err(x) => Err(x),
        }?;
        self.custom_types.insert(ty.name.to_owned(), ty);
        Ok(())
    }
}

fn string_field(name: &str) -> Field {
    let api_version = "__chiselstrike".to_string();
    Field {
        id: None,
        name: name.into(),
        type_id: TypeId::String,
        labels: vec![],
        default: None,
        effective_default: None,
        is_optional: false,
        api_version,
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
        api_version: "__chiselstrike".into(),
        is_unique: false,
    }
}

impl Default for TypeSystem {
    fn default() -> Self {
        let mut ts = Self {
            versions: Default::default(),
            builtin_types: Default::default(),
        };
        ts.builtin_types.insert("string".into(), Type::String);
        ts.builtin_types.insert("number".into(), Type::Float);
        ts.builtin_types.insert("boolean".into(), Type::Boolean);
        ts.add_auth_entity(
            AUTH_USER_NAME,
            vec![
                optional_string_field("emailVerified"),
                optional_string_field("name"),
                optional_string_field("email"),
                optional_string_field("image"),
            ],
            "auth_user",
        );
        ts.add_auth_entity(
            AUTH_SESSION_NAME,
            vec![
                string_field("sessionToken"),
                string_field("userId"),
                string_field("expires"),
            ],
            "auth_session",
        );
        ts.add_auth_entity(
            AUTH_TOKEN_NAME,
            vec![
                string_field("identifier"),
                string_field("expires"),
                string_field("token"),
            ],
            "auth_token",
        );
        ts.add_auth_entity(
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

        ts
    }
}

impl TypeSystem {
    pub(crate) async fn create_builtin_backing_tables(
        &self,
        query_engine: &QueryEngine,
    ) -> anyhow::Result<()> {
        let mut transaction = query_engine.start_transaction().await?;
        for ty in self.builtin_types.values() {
            if let Type::Entity(ty) = ty {
                query_engine.create_table(&mut transaction, ty).await?;
            }
        }
        QueryEngine::commit_transaction(transaction).await?;
        Ok(())
    }

    /// Returns a mutable reference to all types from a specific version.
    ///
    /// If there are no types for this version, the version is created.
    pub(crate) fn get_version_mut(&mut self, api_version: &str) -> &mut VersionTypes {
        self.versions
            .entry(api_version.to_string())
            .or_insert_with(VersionTypes::default)
    }

    /// Returns a read-only reference to all types from a specific version.
    ///
    /// If there are no types for this version, an error is returned
    pub(crate) fn get_version(&self, api_version: &str) -> Result<&VersionTypes, TypeSystemError> {
        self.versions
            .get(api_version)
            .ok_or_else(|| TypeSystemError::NoSuchVersion(api_version.to_owned()))
    }

    /// Adds a custom type to the type system.
    ///
    /// # Arguments
    ///
    /// * `ty` type to add
    ///
    /// # Errors
    ///
    /// If type `ty` already exists in the type system isn't Entity::Custom type,
    /// the function returns `TypeSystemError`.
    pub(crate) fn add_custom_type(&mut self, ty: Entity) -> Result<(), TypeSystemError> {
        let version = self.get_version_mut(&ty.api_version);
        version.add_custom_type(ty)
    }

    /// Generate an [`ObjectDelta`] with the necessary information to evolve a specific type.
    pub(crate) fn generate_type_delta(
        old_type: &ObjectType,
        new_type: Arc<ObjectType>,
        ts: &TypeSystem,
        allow_unsafe_replacement: bool,
    ) -> Result<ObjectDelta, TypeSystemError> {
        if *old_type != *new_type {
            return Err(TypeSystemError::UnsafeReplacement(
                new_type.name.clone(),
                format!(
                    "Types don't match. This is an internal ChiselStrike error. Types: {:?} / {:?}",
                    *old_type, &*new_type
                ),
            ));
        }

        let mut old_fields = FieldMap::from(&*old_type);
        let new_fields = FieldMap::from(&*new_type);

        let mut added_fields = Vec::new();
        let mut removed_fields = Vec::new();
        let mut updated_fields = Vec::new();

        for (name, field) in new_fields.map.iter() {
            match old_fields.map.remove(name) {
                None => {
                    if !allow_unsafe_replacement && field.default.is_none() && !field.is_optional {
                        return Err(TypeSystemError::UnsafeReplacement(new_type.name.clone(), format!("Trying to add a new non-optional field ({}) without a trivial default value. Consider adding a default value or making it optional to make the types compatible", field.name)));
                    }
                    added_fields.push(field.to_owned().clone());
                }
                Some(old) => {
                    // Important: we can only issue this get() here, after we know this model is
                    // pre-existing. Otherwise this breaks in the case where we're adding a
                    // property that is itself a custom model.
                    let field_ty = ts.get(&field.type_id)?;

                    let old_ty = ts.get(&old.type_id)?;
                    if !allow_unsafe_replacement && field_ty != old_ty {
                        // FIXME: it should be almost always possible to evolve things into
                        // strings.
                        return Err(TypeSystemError::UnsafeReplacement(
                            new_type.name.clone(),
                            format!(
                                "changing types from {} into {} for field {}. Incompatible change",
                                old_ty.name(),
                                field_ty.name(),
                                field.name
                            ),
                        ));
                    }

                    if !allow_unsafe_replacement && field.is_unique && !old.is_unique {
                        // FIXME: it should be possible to do it by issuing a select count() and
                        // then a select count distinct and comparing both results. But to do this
                        // safely this needs to be protected by a transaction, so we won't allow it
                        // for now
                        return Err(TypeSystemError::UnsafeReplacement(
                            new_type.name.clone(),
                            format!(
                                "adding uniqueness to field {}. Incompatible change",
                                field.name,
                            ),
                        ));
                    }

                    let attrs = if field.default != old.default
                        || field_ty != old_ty
                        || field.is_optional != old.is_optional
                        || field.is_unique != old.is_unique
                    {
                        Some(FieldAttrDelta {
                            type_id: field.type_id.clone(),
                            default: field.default.clone(),
                            is_optional: field.is_optional,
                            is_unique: field.is_unique,
                        })
                    } else {
                        None
                    };

                    let mut old_labels = old.labels.clone();
                    old_labels.sort();

                    let mut new_labels = field.labels.clone();
                    new_labels.sort();

                    let labels = if old_labels != new_labels {
                        Some(new_labels)
                    } else {
                        None
                    };

                    let id = old.id.ok_or_else(|| {
                        TypeSystemError::InternalServerError(
                            "logical error! updating field without id".to_string(),
                        )
                    })?;
                    updated_fields.push(FieldDelta { id, attrs, labels });
                }
            }
        }

        // only allow the removal of fields that previously had a default value or was optional
        for (_, field) in old_fields.map.into_iter() {
            if !allow_unsafe_replacement && field.default.is_none() && !field.is_optional {
                return Err(TypeSystemError::UnsafeReplacement(
                    new_type.name.clone(),
                    format!(
                        "non-optional field {} doesn't have a default value, so it is unsafe to remove",
                        field.name
                    ),
                ));
            }
            removed_fields.push(field.to_owned().clone());
        }

        Ok(ObjectDelta {
            added_fields,
            removed_fields,
            updated_fields,
            added_indexes: Self::find_added_indexes(old_type, &new_type),
            removed_indexes: Self::find_removed_indexes(old_type, &new_type),
        })
    }

    fn find_added_indexes(old_type: &ObjectType, new_type: &ObjectType) -> Vec<DbIndex> {
        Self::index_diff(new_type.indexes(), old_type.indexes())
    }

    fn find_removed_indexes(old_type: &ObjectType, new_type: &ObjectType) -> Vec<DbIndex> {
        Self::index_diff(old_type.indexes(), new_type.indexes())
    }

    /// Computes difference of `lhs` and `rhs` index sets (with respect to their fields) returning
    /// all indexes that are contained in `lhs` but not in `rhs` (`lhs` - `rhs`).
    fn index_diff(lhs: &[DbIndex], rhs: &[DbIndex]) -> Vec<DbIndex> {
        lhs.iter()
            .filter(|lhs_idx| !rhs.iter().any(|rhs_idx| lhs_idx.fields == rhs_idx.fields))
            .cloned()
            .collect()
    }

    /// Looks up a custom type with name `type_name` across API versions
    ///
    /// # Arguments
    ///
    /// * `type_name` name of custom type to look up.
    /// * `version` the API version this objects belongs to
    ///
    /// # Errors
    ///
    /// If the looked up type does not exists, the function returns a `TypeSystemError`.
    pub(crate) fn lookup_custom_type(
        &self,
        type_name: &str,
        api_version: &str,
    ) -> Result<Entity, TypeSystemError> {
        let version = self.get_version(api_version)?;
        version.lookup_custom_type(type_name)
    }

    /// Looks up a builtin type with name `type_name`.
    pub(crate) fn lookup_builtin_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
        if let Some(element_type_str) = type_name.strip_prefix("Array<") {
            if let Some(element_type_str) = element_type_str.strip_suffix('>') {
                let element_type = self.lookup_builtin_type(element_type_str)?;
                return Ok(Type::Array(Box::new(element_type)));
            }
        }
        self.builtin_types
            .get(type_name)
            .cloned()
            .ok_or_else(|| TypeSystemError::NotABuiltinType(type_name.to_string()))
    }

    /// Tries to look up a type. It tries to match built-ins first, custom types second.
    ///
    /// # Errors
    ///
    /// If the looked up type does not exists, the function returns a `NoSuchType`.
    pub(crate) fn lookup_type(
        &self,
        type_name: &str,
        api_version: &str,
    ) -> Result<Type, TypeSystemError> {
        if let Ok(ty) = self.lookup_builtin_type(type_name) {
            Ok(ty)
        } else {
            let version = self.get_version(api_version)?;
            if let Ok(ty) = version.lookup_custom_type(type_name) {
                Ok(ty.into())
            } else {
                Err(TypeSystemError::NoSuchType(type_name.to_owned()))
            }
        }
    }

    /// Tries to lookup a type of name `type_name` of version `api_version` that
    /// is an Entity. That means it's either a built-in Entity::Auth type like
    /// `AuthUser` or a Entity::Custom.
    ///
    /// # Errors
    ///
    /// If the looked up type does not exists, the function returns a `NoSuchType`.
    pub(crate) fn lookup_entity(
        &self,
        type_name: &str,
        api_version: &str,
    ) -> Result<Entity, TypeSystemError> {
        match self.lookup_builtin_type(type_name) {
            Ok(Type::Entity(ty)) => Ok(ty),
            Err(TypeSystemError::NotABuiltinType(_)) => {
                self.lookup_custom_type(type_name, api_version)
            }
            _ => Err(TypeSystemError::NoSuchType(type_name.to_owned())),
        }
    }

    pub(crate) async fn populate_types<T: AsRef<str>, F: AsRef<str>>(
        &self,
        engine: Arc<QueryEngine>,
        api_version_to: T,
        api_version_from: F,
    ) -> anyhow::Result<()> {
        let to = match self.versions.get(api_version_to.as_ref()) {
            Some(x) => Ok(x),
            None => Err(TypeSystemError::NoSuchVersion(
                api_version_to.as_ref().to_owned(),
            )),
        }?;

        let from = match self.versions.get(api_version_from.as_ref()) {
            Some(x) => Ok(x),
            None => Err(TypeSystemError::NoSuchVersion(
                api_version_from.as_ref().to_owned(),
            )),
        }?;

        for (ty_name, ty_obj) in from.custom_types.iter() {
            if let Some(ty_obj_to) = to.custom_types.get(ty_name) {
                // Either the TO type is a safe replacement of FROM, of we need to have a lens
                ty_obj_to
                    .check_if_safe_to_populate(ty_obj)
                    .with_context(|| {
                        format!(
                            "Not possible to evolve type {} ({} -> {})",
                            ty_name,
                            api_version_from.as_ref(),
                            api_version_to.as_ref()
                        )
                    })?;

                let tr = engine.clone().start_transaction_static().await?;
                let query_plan = QueryPlan::from_type(ty_obj);
                let mut row_streams = engine.query(tr.clone(), query_plan)?;

                while let Some(row) = row_streams.next().await {
                    // FIXME: basic rate limit?
                    let row = row
                        .with_context(|| format!("population can't proceed as reading from the underlying database for type {} failed", ty_obj_to.name))?;
                    engine.add_row_shallow(ty_obj_to, &row).await?;
                }
                drop(row_streams);
                QueryEngine::commit_transaction_static(tr).await?;
            }
        }
        Ok(())
    }

    fn add_auth_entity(
        &mut self,
        type_name: &'static str,
        fields: Vec<Field>,
        backing_table_name: &'static str,
    ) {
        self.builtin_types.insert(type_name.into(), {
            let desc = InternalObject {
                name: type_name,
                backing_table: backing_table_name,
            };
            Entity::Auth(Arc::new(ObjectType::new(desc, fields, vec![]).unwrap())).into()
        });
    }

    pub(crate) fn get(&self, ty: &TypeId) -> Result<Type, TypeSystemError> {
        match ty {
            TypeId::String | TypeId::Float | TypeId::Boolean | TypeId::Id | TypeId::Array(_) => {
                self.lookup_builtin_type(&ty.name())
            }
            TypeId::Entity { name, api_version } => {
                self.lookup_entity(name, api_version).map(Type::Entity)
            }
        }
    }
}
