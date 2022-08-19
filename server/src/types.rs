// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::auth::{AUTH_ACCOUNT_NAME, AUTH_SESSION_NAME, AUTH_TOKEN_NAME, AUTH_USER_NAME};
use crate::datastore::query::{truncate_identifier, QueryPlan};
use crate::datastore::QueryEngine;
use anyhow::Context;
use deno_core::futures;
use derive_new::new;
use futures::StreamExt;
use std::collections::{BTreeMap, HashMap};
use std::ops::Deref;
use std::sync::Arc;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum TypeSystemError {
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
pub struct VersionTypes {
    #[new(default)]
    pub custom_types: HashMap<String, Entity>,
}

#[derive(Debug, Clone)]
pub struct TypeSystem {
    pub versions: HashMap<String, VersionTypes>,
    builtin_types: HashMap<String, Type>,
}

impl VersionTypes {
    pub fn lookup_custom_type(&self, type_name: &str) -> Result<Entity, TypeSystemError> {
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
    pub async fn create_builtin_backing_tables(
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
    pub fn get_version_mut(&mut self, api_version: &str) -> &mut VersionTypes {
        self.versions
            .entry(api_version.to_string())
            .or_insert_with(VersionTypes::default)
    }

    /// Returns a read-only reference to all types from a specific version.
    ///
    /// If there are no types for this version, an error is returned
    pub fn get_version(&self, api_version: &str) -> Result<&VersionTypes, TypeSystemError> {
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
    pub fn add_custom_type(&mut self, ty: Entity) -> Result<(), TypeSystemError> {
        let version = self.get_version_mut(&ty.api_version);
        version.add_custom_type(ty)
    }

    /// Generate an [`ObjectDelta`] with the necessary information to evolve a specific type.
    pub fn generate_type_delta(
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
    pub fn lookup_custom_type(
        &self,
        type_name: &str,
        api_version: &str,
    ) -> Result<Entity, TypeSystemError> {
        let version = self.get_version(api_version)?;
        version.lookup_custom_type(type_name)
    }

    /// Looks up a builtin type with name `type_name`.
    pub fn lookup_builtin_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
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
    pub fn lookup_type(&self, type_name: &str, api_version: &str) -> Result<Type, TypeSystemError> {
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
    pub fn lookup_entity(
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

    pub async fn populate_types<T: AsRef<str>, F: AsRef<str>>(
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

    pub fn get(&self, ty: &TypeId) -> Result<Type, TypeSystemError> {
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

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    String,
    Float,
    Boolean,
    Entity(Entity),
    Array(Box<Type>),
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::Float => "number".to_string(),
            Type::String => "string".to_string(),
            Type::Boolean => "boolean".to_string(),
            Type::Entity(ty) => ty.name.to_string(),
            Type::Array(ty) => format!("Array<{}>", ty.name()),
        }
    }
}

impl From<Entity> for Type {
    fn from(entity: Entity) -> Self {
        Type::Entity(entity)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entity {
    /// User defined Custom entity.
    Custom(Arc<ObjectType>),
    /// Built-in Auth entity.
    Auth(Arc<ObjectType>),
}

impl Entity {
    /// Checks whether `Entity` is Auth builtin type.
    pub fn is_auth(&self) -> bool {
        matches!(self, Entity::Auth(_))
    }
}

impl Deref for Entity {
    type Target = ObjectType;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Custom(obj) | Self::Auth(obj) => obj,
        }
    }
}

/// Uniquely describes a representation of a type.
///
/// This is passed as a parameter to [`ObjectType`]'s constructor
/// identifying a type.
///
/// This exists as a trait because types that are created in memory
/// behave slightly differently than types that are persisted to the database.
///
/// For example:
///  * Types that are created in memory don't yet have an ID, since the type ID is assigned at
///    insert time.
///  * Types that are created in memory can pick any string they want for the backing table, but
///    once that is persisted we need to keep referring to that table.
///
/// There are two implementations provided: one used for reading types back from the datastore
/// (mandatory IDs, backing table, etc), and one from generating types in memory.
///
/// There are two situations where types are generated in memory:
///  * Type lookups, to make sure a user-proposed type is compatible with an existing type
///  * Type creation, where a type fails the lookup above (does not exist) and then has to
///    be created.
///
/// In the first, an ID is never needed. In the second, an ID is needed once the type is about
/// to be used. To avoid dealing with mutexes, internal mutability, and synchronization, we just
/// reload the type system after changes are made to the database.
///
/// This may become a problem if a user has many types, but it is simple, robust, and elegant.
pub trait ObjectDescriptor {
    fn name(&self) -> String;
    fn id(&self) -> Option<i32>;
    fn backing_table(&self) -> String;
    fn api_version(&self) -> String;
}

pub struct InternalObject {
    name: &'static str,
    backing_table: &'static str,
}

impl ObjectDescriptor for InternalObject {
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn id(&self) -> Option<i32> {
        None
    }

    fn backing_table(&self) -> String {
        self.backing_table.to_string()
    }

    fn api_version(&self) -> String {
        "__chiselstrike".to_string()
    }
}

pub struct ExistingObject<'a> {
    name: String,
    api_version: String,
    backing_table: &'a str,
    id: i32,
}

impl<'a> ExistingObject<'a> {
    pub fn new(name: &str, backing_table: &'a str, id: i32) -> anyhow::Result<Self> {
        let split: Vec<&str> = name.split('.').collect();

        anyhow::ensure!(
            split.len() == 2,
            "Expected version information as part of the type name. Got {}. Database corrupted?",
            name
        );
        let api_version = split[0].to_owned();
        let name = split[1].to_owned();

        Ok(Self {
            name,
            backing_table,
            api_version,
            id,
        })
    }
}

impl<'a> ObjectDescriptor for ExistingObject<'a> {
    fn name(&self) -> String {
        self.name.to_owned()
    }

    fn id(&self) -> Option<i32> {
        Some(self.id)
    }

    fn backing_table(&self) -> String {
        self.backing_table.to_owned()
    }

    fn api_version(&self) -> String {
        self.api_version.to_owned()
    }
}

pub struct NewObject<'a> {
    name: &'a str,
    backing_table: String, // store at object creation time so consecutive calls to backing_table() return the same value
    api_version: &'a str,
}

impl<'a> NewObject<'a> {
    pub fn new(name: &'a str, api_version: &'a str) -> Self {
        let mut buf = Uuid::encode_buffer();
        let uuid = Uuid::new_v4();
        let backing_table = format!("ty_{}_{}", name, uuid.to_simple().encode_upper(&mut buf));

        Self {
            name,
            api_version,
            backing_table,
        }
    }
}

impl<'a> ObjectDescriptor for NewObject<'a> {
    fn name(&self) -> String {
        self.name.to_owned()
    }

    fn id(&self) -> Option<i32> {
        None
    }

    fn backing_table(&self) -> String {
        self.backing_table.clone()
    }

    fn api_version(&self) -> String {
        self.api_version.to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeId {
    String,
    Float,
    Boolean,
    Id,
    Entity { name: String, api_version: String },
    Array(Box<TypeId>),
}

impl TypeId {
    pub fn name(&self) -> String {
        match self {
            TypeId::Id | TypeId::String => "string".to_string(),
            TypeId::Float => "number".to_string(),
            TypeId::Boolean => "boolean".to_string(),
            TypeId::Entity { ref name, .. } => name.to_string(),
            TypeId::Array(elem_type) => format!("Array<{}>", elem_type.name()),
        }
    }
}

impl From<Type> for TypeId {
    fn from(other: Type) -> Self {
        match other {
            Type::String => Self::String,
            Type::Float => Self::Float,
            Type::Boolean => Self::Boolean,
            Type::Entity(e) => Self::Entity {
                name: e.name().to_string(),
                api_version: e.api_version.clone(),
            },
            Type::Array(elem_type) => {
                let element_type_id: Self = (*elem_type).into();
                Self::Array(Box::new(element_type_id))
            }
        }
    }
}

impl<T> From<T> for TypeId
where
    T: FieldDescriptor,
{
    fn from(other: T) -> Self {
        other.ty().into()
    }
}

#[derive(Debug)]
pub struct ObjectType {
    /// id of this object in the meta-database. Will be None for objects that are not persisted yet
    pub meta_id: Option<i32>,
    /// Name of this type.
    name: String,
    /// Fields of this type.
    fields: Vec<Field>,
    /// Indexes that are to be created in the database to accelerate queries.
    indexes: Vec<DbIndex>,
    /// user-visible ID of this object.
    chisel_id: Field,
    /// Name of the backing table for this type.
    backing_table: String,

    pub api_version: String,
}

impl ObjectType {
    pub fn new<D: ObjectDescriptor>(
        desc: D,
        fields: Vec<Field>,
        indexes: Vec<DbIndex>,
    ) -> anyhow::Result<Self> {
        let backing_table = desc.backing_table();
        let api_version = desc.api_version();

        for field in fields.iter() {
            anyhow::ensure!(
                api_version == field.api_version,
                "API version of fields don't match: Got {} and {}",
                api_version,
                field.api_version
            );
        }
        for index in &indexes {
            for field_name in &index.fields {
                if field_name == "id" {
                    continue;
                }
                anyhow::ensure!(
                    fields.iter().any(|f| &f.name == field_name),
                    "trying to create an index over field '{}' which is not present on type '{}'",
                    field_name,
                    desc.name()
                );
            }
        }
        let chisel_id = Field {
            id: None,
            name: "id".to_string(),
            type_id: TypeId::Id,
            labels: Vec::default(),
            default: None,
            effective_default: None,
            is_optional: false,
            api_version: "__chiselstrike".into(),
            is_unique: true,
        };

        Ok(Self {
            meta_id: desc.id(),
            name: desc.name(),
            api_version,
            backing_table,
            fields,
            indexes,
            chisel_id,
        })
    }

    pub fn user_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter()
    }

    pub fn all_fields(&self) -> impl Iterator<Item = &Field> {
        std::iter::once(&self.chisel_id).chain(self.fields.iter())
    }

    pub fn has_field(&self, field_name: &str) -> bool {
        self.all_fields().any(|f| f.name == field_name)
    }

    pub fn get_field(&self, field_name: &str) -> Option<&Field> {
        self.all_fields().find(|f| f.name == field_name)
    }

    pub fn backing_table(&self) -> &str {
        &self.backing_table
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn persisted_name(&self) -> String {
        format!("{}.{}", self.api_version, self.name)
    }

    fn check_if_safe_to_populate(&self, source_type: &ObjectType) -> anyhow::Result<()> {
        let source_map: FieldMap<'_> = source_type.into();
        let to_map: FieldMap<'_> = self.into();
        to_map.check_populate_from(&source_map)
    }

    pub fn indexes(&self) -> &Vec<DbIndex> {
        &self.indexes
    }
}

impl PartialEq for ObjectType {
    fn eq(&self, another: &Self) -> bool {
        self.name == another.name && self.api_version == another.api_version
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbIndex {
    /// Id of this index in the meta database. Before it's creation, it will be None.
    pub meta_id: Option<i32>,
    /// Name of the index in database. Before it's creation, it will be None.
    backing_table: Option<String>,
    pub fields: Vec<String>,
}

impl DbIndex {
    pub fn new(meta_id: i32, backing_table: String, fields: Vec<String>) -> Self {
        Self {
            meta_id: Some(meta_id),
            backing_table: Some(backing_table),
            fields,
        }
    }

    pub fn new_from_fields(fields: Vec<String>) -> Self {
        Self {
            meta_id: None,
            backing_table: None,
            fields,
        }
    }

    pub fn name(&self) -> Option<String> {
        self.meta_id.map(|id| {
            let name = format!(
                "index_{id}_{}__{}",
                self.backing_table.as_ref().unwrap(),
                self.fields.join("_")
            );
            truncate_identifier(&name).to_owned()
        })
    }
}

#[derive(Debug)]
struct FieldMap<'a> {
    map: BTreeMap<&'a str, &'a Field>,
}

impl<'a> From<&'a ObjectType> for FieldMap<'a> {
    fn from(ty: &'a ObjectType) -> Self {
        let mut map = BTreeMap::new();
        for field in ty.fields.iter() {
            map.insert(field.name.as_str(), field);
        }
        Self { map }
    }
}

impl<'a> FieldMap<'a> {
    /// Similar to is_safe_replacement_for, but will be able to work across backing tables. Useful
    /// when evolving versions
    fn check_populate_from(&self, source_type: &Self) -> anyhow::Result<()> {
        // to -> from, always ok to remove fields, so only loop over self.
        //
        // Adding fields: Ok, if there is a default value or lens
        //
        // Fields in common: Ok if the type is the same, or if there is a lens
        for (name, field) in self.map.iter() {
            if let Some(existing) = source_type.map.get(name) {
                anyhow::ensure!(
                    existing.type_id.name() == field.type_id.name(),
                    "Type name mismatch on field {} ({} -> {}). We don't support that yet, but that's coming soon! 🙏",
                    name, existing.type_id.name(), field.type_id.name()
                );
            } else {
                anyhow::ensure!(
                    field.default.is_none(),
                    "Adding field {} without a trivial default, which is not supported yet",
                    name
                );
            }
        }
        Ok(())
    }
}

/// Uniquely describes a representation of a field.
///
/// See the [`ObjectDescriptor`] trait for details.
/// Situations where a new versus existing field are created are similar.
pub trait FieldDescriptor {
    fn name(&self) -> String;
    fn id(&self) -> Option<i32>;
    fn ty(&self) -> Type;
    fn api_version(&self) -> String;
}

pub struct ExistingField {
    name: String,
    ty_: Type,
    id: i32,
    version: String,
}

impl ExistingField {
    pub fn new(name: &str, ty_: Type, id: i32, version: &str) -> Self {
        Self {
            name: name.to_owned(),
            ty_,
            id,
            version: version.to_owned(),
        }
    }
}

impl FieldDescriptor for ExistingField {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn id(&self) -> Option<i32> {
        Some(self.id)
    }

    fn ty(&self) -> Type {
        self.ty_.clone()
    }

    fn api_version(&self) -> String {
        self.version.to_owned()
    }
}

pub struct NewField<'a> {
    name: &'a str,
    ty_: Type,
    version: &'a str,
}

impl<'a> NewField<'a> {
    pub fn new(name: &'a str, ty_: Type, version: &'a str) -> anyhow::Result<Self> {
        Ok(Self { name, ty_, version })
    }
}

impl<'a> FieldDescriptor for NewField<'a> {
    fn name(&self) -> String {
        self.name.to_owned()
    }

    fn id(&self) -> Option<i32> {
        None
    }

    fn ty(&self) -> Type {
        self.ty_.clone()
    }

    fn api_version(&self) -> String {
        self.version.to_owned()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub id: Option<i32>,
    pub name: String,
    pub type_id: TypeId,
    pub labels: Vec<String>,
    pub is_optional: bool,
    pub is_unique: bool,
    // We want to keep the default the user gave us so we can
    // return it in `chisel describe`. That's the default that is
    // valid in typescriptland.
    //
    // However when dealing with the database we need to translate
    // that default into something else. One example are booleans,
    // that come to us as either 'true' or 'false', but we store as
    // 0 or 1 in sqlite.
    default: Option<String>,
    effective_default: Option<String>,
    api_version: String,
}

impl Field {
    pub fn new<D: FieldDescriptor>(
        desc: D,
        labels: Vec<String>,
        default: Option<String>,
        is_optional: bool,
        is_unique: bool,
    ) -> Self {
        let effective_default = if let Type::Boolean = &desc.ty() {
            default
                .clone()
                .map(|x| if x == "false" { "false" } else { "true" })
                .map(|x| x.to_string())
        } else {
            default.clone()
        };

        Self {
            id: desc.id(),
            name: desc.name(),
            api_version: desc.api_version(),
            type_id: desc.into(),
            labels,
            default,
            effective_default,
            is_optional,
            is_unique,
        }
    }

    pub fn user_provided_default(&self) -> &Option<String> {
        &self.default
    }

    pub fn default_value(&self) -> &Option<String> {
        &self.effective_default
    }

    pub fn generate_value(&self) -> Option<String> {
        match self.type_id {
            TypeId::Id => Some(Uuid::new_v4().to_string()),
            _ => self.default.clone(),
        }
    }

    pub fn persisted_name(&self, parent_type_name: &ObjectType) -> String {
        format!(
            "{}.{}.{}",
            self.api_version,
            parent_type_name.name(),
            self.name
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldAttrDelta {
    pub type_id: TypeId,
    pub default: Option<String>,
    pub is_optional: bool,
    pub is_unique: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDelta {
    pub id: i32,
    pub attrs: Option<FieldAttrDelta>,
    pub labels: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectDelta {
    pub added_fields: Vec<Field>,
    pub removed_fields: Vec<Field>,
    pub updated_fields: Vec<FieldDelta>,
    pub added_indexes: Vec<DbIndex>,
    pub removed_indexes: Vec<DbIndex>,
}
