// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use super::{
    BuiltinTypes, DbIndex, Entity, FieldAttrDelta, FieldDelta, FieldMap, ObjectDelta, ObjectType,
    QueryEngine, QueryPlan, Type, TypeId, TypeSystemError,
};
use anyhow::Context;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TypeSystem {
    pub custom_types: HashMap<String, Entity>,
    pub builtin: Arc<BuiltinTypes>,
    pub version_id: String,
}

impl TypeSystem {
    pub fn new(builtin: Arc<BuiltinTypes>, version_id: String) -> Self {
        Self {
            custom_types: HashMap::new(),
            builtin,
            version_id,
        }
    }

    pub fn lookup_custom_type(&self, type_name: &str) -> Result<Entity, TypeSystemError> {
        match self.custom_types.get(type_name) {
            Some(ty) => Ok(ty.to_owned()),
            None => Err(TypeSystemError::NoSuchType(type_name.to_owned())),
        }
    }

    pub fn add_custom_type(&mut self, ty: Entity) -> Result<(), TypeSystemError> {
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

    /// Generate an [`ObjectDelta`] with the necessary information to evolve a specific type.
    pub fn generate_type_delta(
        &self,
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
                        return Err(TypeSystemError::UnsafeReplacement(new_type.name.clone(), format!("Trying to add a new non-optional field ({}) without a trivial default value. Consider adding a default value or making it optional to make the types compatible", field.name)));
                    }
                    added_fields.push(field.to_owned().clone());
                }
                Some(old) => {
                    // Important: we can only issue this get() here, after we know this model is
                    // pre-existing. Otherwise this breaks in the case where we're adding a
                    // property that is itself a custom model.
                    let field_ty = self.get(&field.type_id)?;

                    let old_ty = self.get(&old.type_id)?;
                    if field_ty != old_ty {
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

    /// Looks up a builtin type with name `type_name`.
    pub fn lookup_builtin_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
        if let Some(element_type_str) = type_name.strip_prefix("Array<") {
            if let Some(element_type_str) = element_type_str.strip_suffix('>') {
                let element_type = self.lookup_builtin_type(element_type_str)?;
                return Ok(Type::Array(Box::new(element_type)));
            }
        }
        self.builtin
            .types
            .get(type_name)
            .cloned()
            .ok_or_else(|| TypeSystemError::NotABuiltinType(type_name.to_string()))
    }

    /// Tries to look up a type. It tries to match built-ins first, custom types second.
    ///
    /// # Errors
    ///
    /// If the looked up type does not exists, the function returns a `NoSuchType`.
    pub fn lookup_type(&self, type_name: &str) -> Result<Type, TypeSystemError> {
        if let Ok(ty) = self.lookup_builtin_type(type_name) {
            Ok(ty)
        } else if let Ok(ty) = self.lookup_custom_type(type_name) {
            Ok(ty.into())
        } else {
            Err(TypeSystemError::NoSuchType(type_name.to_owned()))
        }
    }

    /// Tries to lookup a type of name `type_name` is an Entity. That means it's either a built-in
    /// Entity::Auth type like `AuthUser` or a Entity::Custom.
    ///
    /// # Errors
    ///
    /// If the looked up type does not exists, the function returns a `NoSuchType`.
    pub fn lookup_entity(&self, type_name: &str) -> Result<Entity, TypeSystemError> {
        match self.lookup_builtin_type(type_name) {
            Ok(Type::Entity(ty)) => Ok(ty),
            Err(TypeSystemError::NotABuiltinType(_)) => self.lookup_custom_type(type_name),
            _ => Err(TypeSystemError::NoSuchType(type_name.to_owned())),
        }
    }

    pub async fn populate_types(
        engine: &QueryEngine,
        to: &TypeSystem,
        from: &TypeSystem,
    ) -> anyhow::Result<()> {
        for (ty_name, ty_obj) in from.custom_types.iter() {
            if let Some(ty_obj_to) = to.custom_types.get(ty_name) {
                // Either the TO type is a safe replacement of FROM, of we need to have a lens
                ty_obj_to
                    .check_if_safe_to_populate(ty_obj)
                    .with_context(|| {
                        format!(
                            "Not possible to evolve type {} ({} -> {})",
                            ty_name, from.version_id, to.version_id,
                        )
                    })?;

                let tr = engine.begin_transaction_static().await?;
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

    pub fn get(&self, ty: &TypeId) -> Result<Type, TypeSystemError> {
        match ty {
            TypeId::String | TypeId::Float | TypeId::Boolean | TypeId::Id | TypeId::Array(_) => {
                self.lookup_builtin_type(&ty.name())
            }
            TypeId::Entity { name, version_id } => {
                if version_id == "__chiselstrike" {
                    self.lookup_builtin_type(name)
                } else {
                    assert_eq!(*version_id, self.version_id);
                    self.lookup_entity(name).map(Type::Entity)
                }
            }
        }
    }
}
