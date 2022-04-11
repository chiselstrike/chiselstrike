// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::datastore::query::QueryPlan;
use crate::datastore::QueryEngine;
use anyhow::Context;
use derive_new::new;
use futures::StreamExt;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub(crate) enum TypeSystemError {
    #[error["type already exists"]]
    TypeAlreadyExists(Arc<ObjectType>),
    #[error["no such type: {0}"]]
    NoSuchType(String),
    #[error["no such API version: {0}"]]
    NoSuchVersion(String),
    #[error["builtin type expected, got `{0}` instead"]]
    NotABuiltinType(String),
    #[error["unsafe to replace type: {0}. Reason: {1}"]]
    UnsafeReplacement(String, String),
    #[error["Error while trying to manipulate types: {0}"]]
    InternalServerError(String),
}

#[derive(Debug, Default, Clone, new)]
pub(crate) struct VersionTypes {
    #[new(default)]
    pub(crate) custom_types: HashMap<String, Arc<ObjectType>>,
}

#[derive(Debug, Clone)]
pub(crate) struct TypeSystem {
    pub(crate) versions: HashMap<String, VersionTypes>,
    builtin_types: HashMap<String, Type>,
}

impl VersionTypes {
    pub(crate) fn lookup_custom_type(
        &self,
        type_name: &str,
    ) -> Result<Arc<ObjectType>, TypeSystemError> {
        match self.custom_types.get(type_name) {
            Some(ty) => Ok(ty.to_owned()),
            None => Err(TypeSystemError::NoSuchType(type_name.to_owned())),
        }
    }

    fn add_type(&mut self, ty: Arc<ObjectType>) -> Result<(), TypeSystemError> {
        match self.lookup_custom_type(&ty.name) {
            Ok(old) => Err(TypeSystemError::TypeAlreadyExists(old)),
            Err(TypeSystemError::NoSuchType(_)) => Ok(()),
            Err(x) => Err(x),
        }?;
        self.custom_types.insert(ty.name.to_owned(), ty);
        Ok(())
    }
}

pub(crate) const OAUTHUSER_TYPE_NAME: &str = "OAuthUser";

fn string_field(name: &str) -> Field {
    Field {
        id: None,
        name: name.into(),
        type_: Type::String,
        labels: vec![],
        default: None,
        effective_default: None,
        is_optional: false,
        api_version: "__chiselstrike".into(),
        is_unique: false,
    }
}

fn optional_string_field(name: &str) -> Field {
    let mut f = string_field(name);
    f.is_optional = true;
    f
}

impl TypeSystem {
    pub(crate) fn new() -> Self {
        let mut ts = Self {
            versions: Default::default(),
            builtin_types: Default::default(),
        };
        ts.builtin_types.insert("string".into(), Type::String);
        ts.builtin_types.insert("number".into(), Type::Float);
        ts.builtin_types.insert("boolean".into(), Type::Boolean);
        ts.builtin_types.insert(OAUTHUSER_TYPE_NAME.into(), {
            let fields = vec![Field {
                id: None,
                name: "username".into(),
                type_: Type::String,
                labels: vec![],
                default: None,
                effective_default: None,
                is_optional: false,
                api_version: "__chiselstrike".into(),
                is_unique: false,
            }];
            let desc = InternalObject {
                name: OAUTHUSER_TYPE_NAME,
                backing_table: "oauth_user",
            };
            Type::Object(Arc::new(ObjectType::new(desc, fields).unwrap()))
        });
        ts.builtin_types.insert("NextAuthUser".into(), {
            let fields = vec![
                optional_string_field("emailVerified"),
                optional_string_field("name"),
                optional_string_field("email"),
                optional_string_field("image"),
            ];

            let desc = InternalObject {
                name: "NextAuthUser",
                backing_table: "nextauth_user",
            };

            Type::Object(Arc::new(ObjectType::new(desc, fields).unwrap()))
        });
        ts.builtin_types.insert("NextAuthSession".into(), {
            let fields = vec![
                string_field("sessionToken"),
                string_field("userId"),
                string_field("expires"),
            ];

            let desc = InternalObject {
                name: "NextAuthSession",
                backing_table: "nextauth_session",
            };

            Type::Object(Arc::new(ObjectType::new(desc, fields).unwrap()))
        });

        ts
    }

    pub(crate) async fn create_builtin_backing_tables(
        &self,
        query_engine: &QueryEngine,
    ) -> anyhow::Result<()> {
        let mut transaction = query_engine.start_transaction().await?;
        for ty in self.builtin_types.values() {
            if let Type::Object(ty) = ty {
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
    /// If type `ty` already exists in the type system, the function returns `TypeSystemError`.
    pub(crate) fn add_type(&mut self, ty: Arc<ObjectType>) -> Result<(), TypeSystemError> {
        let version = self.get_version_mut(&ty.api_version);
        version.add_type(ty)
    }

    /// Generate an [`ObjectDelta`] with the necessary information to evolve a specific type.
    pub(crate) fn generate_type_delta(
        old_type: &ObjectType,
        new_type: Arc<ObjectType>,
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
                    if field.default.is_none() && !field.is_optional {
                        return Err(TypeSystemError::UnsafeReplacement(new_type.name.clone(), format!("Trying to add a new non-optional field ({}) without a default value. Consider adding a default value or making it optional to make the types compatible", field.name)));
                    }
                    added_fields.push(field.to_owned().clone());
                }
                Some(old) => {
                    if field.type_ != old.type_ {
                        // FIXME: it should be almost always possible to evolve things into
                        // strings.
                        return Err(TypeSystemError::UnsafeReplacement(
                            new_type.name.clone(),
                            format!(
                                "changing types from {} into {} for field {}. Incompatible change",
                                old.type_.name(),
                                field.type_.name(),
                                field.name
                            ),
                        ));
                    }

                    if field.is_unique && !old.is_unique {
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
                        || field.type_ != old.type_
                        || field.is_optional != old.is_optional
                        || field.is_unique != old.is_unique
                    {
                        Some(FieldAttrDelta {
                            type_: field.type_.clone(),
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
            if field.default.is_none() && !field.is_optional {
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
        })
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
    ) -> Result<Arc<ObjectType>, TypeSystemError> {
        let version = self.get_version(api_version)?;
        version.lookup_custom_type(type_name)
    }

    /// Looks up a builtin type with name `type_name`.
    pub(crate) fn lookup_builtin_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
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
                Ok(Type::Object(ty))
            } else {
                Err(TypeSystemError::NoSuchType(type_name.to_owned()))
            }
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
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Type {
    String,
    Float,
    Boolean,
    Id,
    Object(Arc<ObjectType>),
}

impl Type {
    pub(crate) fn name(&self) -> &str {
        match self {
            Type::Float => "number",
            Type::Id => "string",
            Type::String => "string",
            Type::Boolean => "boolean",
            Type::Object(ty) => &ty.name,
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
pub(crate) trait ObjectDescriptor {
    fn name(&self) -> String;
    fn id(&self) -> Option<i32>;
    fn backing_table(&self) -> String;
    fn api_version(&self) -> String;
}

pub(crate) struct InternalObject {
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

pub(crate) struct ExistingObject<'a> {
    name: String,
    api_version: String,
    backing_table: &'a str,
    id: i32,
}

impl<'a> ExistingObject<'a> {
    pub(crate) fn new(name: &str, backing_table: &'a str, id: i32) -> anyhow::Result<Self> {
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

pub(crate) struct NewObject<'a> {
    name: &'a str,
    backing_table: String, // store at object creation time so consecutive calls to backing_table() return the same value
    api_version: &'a str,
}

impl<'a> NewObject<'a> {
    pub(crate) fn new(name: &'a str, api_version: &'a str) -> Self {
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

#[derive(Debug)]
pub(crate) struct ObjectType {
    /// id of this object in the meta-database. Will be None for objects that are not persisted yet
    pub(crate) meta_id: Option<i32>,
    /// Name of this type.
    name: String,
    /// Fields of this type.
    fields: Vec<Field>,
    /// user-visible ID of this object.
    chisel_id: Field,
    /// Name of the backing table for this type.
    backing_table: String,

    pub(crate) api_version: String,
}

impl ObjectType {
    pub(crate) fn new<D: ObjectDescriptor>(desc: D, fields: Vec<Field>) -> anyhow::Result<Self> {
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
        let chisel_id = Field {
            id: None,
            name: "id".to_string(),
            type_: Type::Id,
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
            chisel_id,
        })
    }

    pub(crate) fn user_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter()
    }

    pub(crate) fn all_fields(&self) -> impl Iterator<Item = &Field> {
        std::iter::once(&self.chisel_id).chain(self.fields.iter())
    }

    pub(crate) fn has_field(&self, field_name: &str) -> bool {
        self.all_fields().any(|f| f.name == field_name)
    }

    pub(crate) fn get_field(&self, field_name: &str) -> Option<&Field> {
        self.all_fields().find(|f| f.name == field_name)
    }

    pub(crate) fn backing_table(&self) -> &str {
        &self.backing_table
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn persisted_name(&self) -> String {
        format!("{}.{}", self.api_version, self.name)
    }

    fn check_if_safe_to_populate(&self, source_type: &ObjectType) -> anyhow::Result<()> {
        let source_map: FieldMap<'_> = source_type.into();
        let to_map: FieldMap<'_> = self.into();
        to_map.check_populate_from(&source_map)
    }
}

impl PartialEq for ObjectType {
    fn eq(&self, another: &Self) -> bool {
        self.name == another.name && self.api_version == another.api_version
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
                    existing.type_.name() == field.type_.name(),
                    "Type name mismatch on field {} ({} -> {}). We don't support that yet, but that's coming soon! 🙏",
                    name, existing.type_.name(), field.type_.name()
                );
            } else {
                anyhow::ensure!(
                    field.default.is_none(),
                    "Adding field {} without a default, which is not supported yet",
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
pub(crate) trait FieldDescriptor {
    fn name(&self) -> String;
    fn id(&self) -> Option<i32>;
    fn ty(&self) -> Type;
    fn api_version(&self) -> String;
}

pub(crate) struct ExistingField {
    name: String,
    ty_: Type,
    id: i32,
    version: String,
}

impl ExistingField {
    pub(crate) fn new(name: &str, ty_: Type, id: i32, version: &str) -> Self {
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

pub(crate) struct NewField<'a> {
    name: &'a str,
    ty_: Type,
    version: &'a str,
}

impl<'a> NewField<'a> {
    pub(crate) fn new(name: &'a str, ty_: Type, version: &'a str) -> anyhow::Result<Self> {
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
pub(crate) struct Field {
    pub(crate) id: Option<i32>,
    pub(crate) name: String,
    pub(crate) type_: Type,
    pub(crate) labels: Vec<String>,
    pub(crate) is_optional: bool,
    pub(crate) is_unique: bool,
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
    pub(crate) fn new<D: FieldDescriptor>(
        desc: D,
        labels: Vec<String>,
        default: Option<String>,
        is_optional: bool,
        is_unique: bool,
    ) -> Self {
        let effective_default = if let Type::Boolean = &desc.ty() {
            default
                .clone()
                .map(|x| if x == "false" { "0" } else { "1" })
                .map(|x| x.to_string())
        } else {
            default.clone()
        };

        Self {
            id: desc.id(),
            name: desc.name(),
            type_: desc.ty(),
            api_version: desc.api_version(),
            labels,
            default,
            effective_default,
            is_optional,
            is_unique,
        }
    }

    pub(crate) fn user_provided_default(&self) -> &Option<String> {
        &self.default
    }

    pub(crate) fn default_value(&self) -> &Option<String> {
        &self.effective_default
    }

    pub(crate) fn generate_value(&self) -> Option<String> {
        match self.type_ {
            Type::Id => Some(Uuid::new_v4().to_string()),
            _ => self.default.clone(),
        }
    }

    pub(crate) fn persisted_name(&self, parent_type_name: &ObjectType) -> String {
        format!(
            "{}.{}.{}",
            self.api_version,
            parent_type_name.name(),
            self.name
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldAttrDelta {
    pub(crate) type_: Type,
    pub(crate) default: Option<String>,
    pub(crate) is_optional: bool,
    pub(crate) is_unique: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldDelta {
    pub(crate) id: i32,
    pub(crate) attrs: Option<FieldAttrDelta>,
    pub(crate) labels: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObjectDelta {
    pub(crate) added_fields: Vec<Field>,
    pub(crate) removed_fields: Vec<Field>,
    pub(crate) updated_fields: Vec<FieldDelta>,
}
