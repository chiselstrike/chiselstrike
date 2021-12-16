// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use derive_new::new;
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
    #[error["object type expected, got `{0}` instead"]]
    ObjectTypeRequired(String),
    #[error["unsafe to replace type: {0}. Reason: {1}"]]
    UnsafeReplacement(String, String),
    #[error["Error while trying to manipulate types: {0}"]]
    InternalServerError(String),
}

#[derive(Debug, Default, Clone, new)]
pub(crate) struct VersionTypes {
    #[new(default)]
    pub(crate) types: HashMap<String, Arc<ObjectType>>,
}

#[derive(Debug, Default, Clone, new)]
pub(crate) struct TypeSystem {
    #[new(default)]
    pub(crate) versions: HashMap<String, VersionTypes>,
}

impl VersionTypes {
    pub(crate) fn lookup_object_type(
        &self,
        type_name: &str,
    ) -> Result<Arc<ObjectType>, TypeSystemError> {
        match self.lookup_type(type_name) {
            Ok(Type::Object(ty)) => Ok(ty),
            Ok(_) => Err(TypeSystemError::ObjectTypeRequired(type_name.to_string())),
            Err(e) => Err(e),
        }
    }

    pub(crate) fn lookup_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
        match type_name {
            "string" => Ok(Type::String),
            "bigint" => Ok(Type::Int),
            "number" => Ok(Type::Float),
            "boolean" => Ok(Type::Boolean),
            type_name => match self.types.get(type_name) {
                Some(ty) => Ok(Type::Object(ty.to_owned())),
                None => Err(TypeSystemError::NoSuchType(type_name.to_owned())),
            },
        }
    }

    fn add_type(&mut self, ty: Arc<ObjectType>) -> Result<(), TypeSystemError> {
        match self.lookup_object_type(&ty.name) {
            Ok(old) => Err(TypeSystemError::TypeAlreadyExists(old)),
            Err(TypeSystemError::NoSuchType(_)) => Ok(()),
            Err(x) => Err(x),
        }?;
        self.types.insert(ty.name.to_owned(), ty);
        Ok(())
    }
}

impl TypeSystem {
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

    /// Adds an object type to the type system.
    ///
    /// # Arguments
    ///
    /// * `ty` object to add
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
                    if field.default.is_none() {
                        return Err(TypeSystemError::UnsafeReplacement(new_type.name.clone(), format!("Trying to add a new field ({}) without a default value. Consider adding a default value to make the types compatible", field.name)));
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

                    let attrs = if field.default != old.default
                        || field.type_ != old.type_
                        || field.is_optional != old.is_optional
                    {
                        Some(FieldAttrDelta {
                            type_: field.type_.clone(),
                            default: field.default.clone(),
                            is_optional: field.is_optional,
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

        // only allow the removal of fields that previously had a default value
        for (_, field) in old_fields.map.into_iter() {
            if field.default.is_none() {
                return Err(TypeSystemError::UnsafeReplacement(
                    new_type.name.clone(),
                    format!(
                        "field {} doesn't have a default value, so it is unsafe to remove",
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

    /// Looks up an object type with name `type_name` across API versions
    ///
    /// # Arguments
    ///
    /// * `type_name` name of object type to look up.
    /// * `version` the API version this objects belongs to
    ///
    /// # Errors
    ///
    /// If the looked up type does not exists or is a built-in type, the function returns a `TypeSystemError`.
    pub(crate) fn lookup_object_type(
        &self,
        type_name: &str,
        api_version: &str,
    ) -> Result<Arc<ObjectType>, TypeSystemError> {
        match self.lookup_type(type_name, api_version) {
            Ok(Type::Object(ty)) => Ok(ty),
            Ok(_) => Err(TypeSystemError::ObjectTypeRequired(type_name.to_string())),
            Err(e) => Err(e),
        }
    }

    /// Looks up a type with name `type_name` across API versions
    ///
    /// # Arguments
    ///
    /// * `type_name` name of object type to look up.
    /// * `version` the API version this objects belongs to
    ///
    /// # Errors
    ///
    /// If the looked up type does not exists or is a built-in type, the function returns a `TypeSystemError`.
    pub(crate) fn lookup_type(
        &self,
        type_name: &str,
        api_version: &str,
    ) -> Result<Type, TypeSystemError> {
        // Note that the base types exist and are the same in all versions, so we have to look them
        // up separately, before we call get_version, which will error out if the version doesn't
        // exist.
        match type_name {
            "string" => Ok(Type::String),
            "bigint" => Ok(Type::Int),
            "number" => Ok(Type::Float),
            "boolean" => Ok(Type::Boolean),
            _ => {
                let version = self.get_version(api_version)?;
                version.lookup_type(type_name)
            }
        }
    }

    pub(crate) fn update(&mut self, other: &TypeSystem) {
        self.versions.clear();
        self.versions = other.versions.clone();
    }

    /// Makes sure the types in this `TypeSystem` are available
    /// for usage in deno
    pub(crate) fn refresh_types(&self) -> anyhow::Result<()> {
        crate::deno::flush_types()?;
        // FIXME: flush_types just destroyed all versions, so we have to
        // reapply the other versions. Ideally we would have an implementation
        // of this that only refreshes a single version
        for (_, version) in self.versions.iter() {
            for (_, ty) in version.types.iter() {
                crate::deno::define_type(ty)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Type {
    String,
    Int,
    Float,
    Boolean,
    Object(Arc<ObjectType>),
}

impl Type {
    pub(crate) fn name(&self) -> &str {
        match self {
            Type::Float => "number",
            Type::Int => "bigint",
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
    pub(crate) id: Option<i32>,
    /// Name of this type.
    name: String,
    /// Fields of this type.
    fields: Vec<Field>,
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
        Ok(Self {
            id: desc.id(),
            name: desc.name(),
            api_version,
            backing_table,
            fields,
        })
    }

    pub(crate) fn user_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter()
    }

    pub(crate) fn all_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter()
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
}

impl PartialEq for ObjectType {
    fn eq(&self, another: &Self) -> bool {
        self.name == another.name && self.api_version == another.api_version
    }
}

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
    pub(crate) fn new(
        ts: &TypeSystem,
        name: &str,
        id: i32,
        field_type: &str,
    ) -> anyhow::Result<Self> {
        let split: Vec<&str> = name.split('.').collect();
        anyhow::ensure!(split.len() == 3, "Expected version and type information as part of the field name. Got {}. Database corrupted?", name);
        let name = split[2].to_owned();
        let version = split[0].to_owned();

        Ok(Self {
            ty_: ts.lookup_type(field_type, &version)?,
            name,
            id,
            version,
        })
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
    pub(crate) fn new(
        versions: &VersionTypes,
        name: &'a str,
        type_name: &str,
        version: &'a str,
    ) -> anyhow::Result<Self> {
        let ty_ = versions.lookup_type(type_name)?;
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
    pub(crate) default: Option<String>,
    pub(crate) is_optional: bool,
    api_version: String,
}

impl Field {
    pub(crate) fn new<D: FieldDescriptor>(
        desc: D,
        labels: Vec<String>,
        default: Option<String>,
        is_optional: bool,
    ) -> Self {
        Self {
            id: desc.id(),
            name: desc.name(),
            type_: desc.ty(),
            api_version: desc.api_version(),
            labels,
            default,
            is_optional,
        }
    }

    pub(crate) fn generate_value(&self) -> Option<String> {
        self.default.clone()
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
