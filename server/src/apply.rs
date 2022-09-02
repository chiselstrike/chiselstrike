// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::api::ApiInfo;
use crate::datastore::{MetaService, QueryEngine};
use crate::policies::{Policies, VersionPolicy};
use crate::proto::{
    type_msg::TypeEnum, ChiselApplyRequest, ContainerType, IndexCandidate, TypeMsg,
};
use crate::proto::{AddTypeRequest, FieldDefinition, PolicyUpdateRequest};
use crate::types::{
    DbIndex, Entity, Field, NewField, NewObject, ObjectType, Type, TypeSystem, TypeSystemError,
};
use crate::FEATURES;
use anyhow::{Context, Result};
use petgraph::graphmap::GraphMap;
use petgraph::Directed;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

pub struct ApplyResult {
    pub type_names_user_order: Vec<String>,
    pub labels: Vec<String>,
    pub version_policy: VersionPolicy,
}

#[allow(dead_code)]
pub struct ParsedPolicies {
    version_policy: (VersionPolicy, String),
}

impl ParsedPolicies {
    fn parse(request: &[PolicyUpdateRequest]) -> Result<Self> {
        let mut version_policy = None;

        for p in request {
            let path = PathBuf::from(&p.path);
            match path.extension().and_then(|s| s.to_str()) {
                Some("ts") if FEATURES.typescript_policies => {
                    // let entity_name = path
                    //     .file_name()
                    //     .and_then(|s| s.to_str())
                    //     .and_then(|s| s.strip_suffix(".ts"))
                    //     .context("invalid policy path")?
                    //     .to_owned();

                    todo!();
                    // let policy = EntityPolicy::from_policy_code(p.policy_config.clone())?;
                    // entity_policies.insert(entity_name, policy);
                }
                _ => {
                    if version_policy.is_none() {
                        version_policy.replace((
                            VersionPolicy::from_yaml(&p.policy_config)?,
                            p.policy_config.clone(),
                        ));
                    } else {
                        anyhow::bail!("Currently only one policy file supported");
                    }
                }
            }
        }

        Ok(Self {
            version_policy: version_policy.unwrap_or_default(),
        })
    }
}

pub async fn apply(
    query_engine: &QueryEngine,
    meta: &MetaService,
    type_system: &mut TypeSystem,
    policies: &mut Policies,
    apply_request: &ChiselApplyRequest,
    api_version: String,
    api_info: &ApiInfo,
) -> Result<ApplyResult> {
    let mut type_names = BTreeSet::new();
    let mut type_names_user_order = vec![];

    for tdef in apply_request.types.iter() {
        type_names.insert(tdef.name.clone());
        type_names_user_order.push(tdef.name.clone());
    }

    let mut to_remove = vec![];
    let mut to_remove_has_data = vec![];
    let mut to_insert = vec![];
    let mut to_update = vec![];

    type_system.get_version_mut(&api_version);
    let version_types = type_system.get_version(&api_version)?;

    let mut transaction = meta.begin_transaction().await?;

    for (existing, removed) in version_types.custom_types.iter() {
        if type_names.get(existing).is_none() {
            match meta.count_rows(&mut transaction, removed).await? {
                0 => to_remove.push(removed.clone()),
                cnt => to_remove_has_data.push((removed.clone(), cnt)),
            }
        }
    }

    let ParsedPolicies { version_policy, .. } = ParsedPolicies::parse(&apply_request.policies)?;

    if !to_remove_has_data.is_empty() && !apply_request.allow_type_deletion {
        let s = to_remove_has_data
            .iter()
            .map(|x| format!("{} ({} elements)", x.0.name(), x.1))
            .fold("\t".to_owned(), |acc, x| format!("{}\n\t{}", acc, x));
        anyhow::bail!(
            r"Trying to remove models from the models file, but the following models still have data:
{}

To proceed, try:

'npx chisel apply --allow-type-deletion' (if installed from npm)

or

'chisel apply --allow-type-deletion' (otherwise)",
            s
        );
    }
    // if we got here, either the slice is empty anyway, or the user is forcing the deletion.
    to_remove.extend(to_remove_has_data.iter().map(|x| x.0.clone()));

    let mut decorators = BTreeSet::default();
    let mut new_types = HashMap::<String, Entity>::default();
    let indexes = aggregate_indexes(&apply_request.index_candidates);

    // No changes are made to the type system in this loop. We re-read the database after we
    // apply the changes, and this way we don't have to deal with the case of succeding to
    // apply a type, but failing the next
    for type_def in sort_custom_types(type_system, apply_request.types.clone())? {
        let name = type_def.name;
        if type_system.lookup_builtin_type(&name).is_ok() {
            anyhow::bail!("custom type expected, got `{}` instead", name);
        }

        let mut fields = Vec::new();
        for field in type_def.field_defs {
            for label in &field.labels {
                decorators.insert(label.clone());
            }

            let field_ty = field.field_type()?;
            let field_ty = if field_ty.is_builtin(type_system)? {
                field_ty.get_builtin(type_system)?
            } else if let TypeEnum::Entity(entity_name) = field_ty {
                match new_types.get(entity_name) {
                    Some(ty) => Type::Entity(ty.clone()),
                    None => anyhow::bail!(
                        "field type `{entity_name}` is neither a built-in nor a custom type",
                    ),
                }
            } else {
                anyhow::bail!("field type must either contain an entity or be a builtin");
            };

            fields.push(Field::new(
                &NewField::new(&field.name, field_ty, &api_version)?,
                field.labels,
                field.default_value,
                field.is_optional,
                field.is_unique,
            ));
        }
        let ty_indexes = indexes.get(&name).cloned().unwrap_or_default();

        let ty = Arc::new(ObjectType::new(
            &NewObject::new(&name, &api_version),
            fields,
            ty_indexes,
        )?);

        new_types.insert(name.to_owned(), Entity::Custom(ty.clone()));

        match version_types.lookup_custom_type(&name) {
            Ok(old_type) => {
                let is_empty = meta.count_rows(&mut transaction, &old_type).await? == 0;
                let delta = TypeSystem::generate_type_delta(&old_type, ty, type_system, is_empty)?;
                to_update.push((old_type.clone(), delta));
            }
            Err(TypeSystemError::NoSuchType(_) | TypeSystemError::NoSuchVersion(_)) => {
                to_insert.push(ty.clone());
            }
            Err(e) => anyhow::bail!(e),
        }
    }

    meta.persist_policy_version(&mut transaction, &api_version, &version_policy.1)
        .await?;

    meta.persist_api_info(&mut transaction, &api_version, api_info)
        .await?;

    for ty in to_insert.iter() {
        // FIXME: Consistency between metadata and backing store updates.
        meta.insert_type(&mut transaction, ty).await?;
    }

    for (old, delta) in to_update.iter() {
        meta.update_type(&mut transaction, old, delta.clone())
            .await?;
    }

    for ty in to_remove.iter() {
        meta.remove_type(&mut transaction, ty).await?;
    }

    MetaService::commit_transaction(transaction).await?;

    let labels: Vec<String> = version_policy
        .0
        .labels
        .keys()
        .map(|x| x.to_owned())
        .collect();
    *type_system = meta.load_type_system().await?;

    policies
        .versions
        .insert(api_version.to_owned(), version_policy.0.clone());

    // FIXME: Now that we have --db-uri, this is the reason we still have to drop
    // the transaction on meta, and acquire on query_engine: we need to reload the
    // type system to get db-side IDs, and if we do that before we get the transactions
    // then we won't get them. Without ids, we can't build the to_insert and to_update
    // arrays.
    //
    // Refresh to_insert types so that they have fresh meta ids (e.g. new  DbIndexes
    // need their meta id to be created in the storage database).
    let to_insert = to_insert
        .iter()
        .map(|ty| type_system.lookup_custom_type(ty.name(), &api_version))
        .collect::<Result<Vec<_>, _>>()?;

    let to_update = to_update
        .into_iter()
        .map(|(ty, delta)| {
            let updated_ty = type_system.lookup_custom_type(ty.name(), &api_version);
            updated_ty.map(|ty| (ty, delta))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut transaction = query_engine.begin_transaction().await?;
    for ty in to_insert.into_iter() {
        query_engine.create_table(&mut transaction, &ty).await?;
    }

    for ty in to_remove.into_iter() {
        query_engine.drop_table(&mut transaction, &ty).await?;
    }

    for (old, delta) in to_update.into_iter() {
        query_engine
            .alter_table(&mut transaction, &old, delta)
            .await?;
    }
    QueryEngine::commit_transaction(transaction).await?;

    Ok(ApplyResult {
        type_names_user_order,
        labels,
        version_policy: version_policy.0,
    })
}

fn aggregate_indexes(indexes: &Vec<IndexCandidate>) -> HashMap<String, Vec<DbIndex>> {
    let mut index_map = HashMap::<String, Vec<DbIndex>>::new();
    for candidate in indexes {
        let idx = DbIndex::new_from_fields(candidate.properties.clone());
        if let Some(type_indexes) = index_map.get_mut(&candidate.entity_name) {
            type_indexes.push(idx);
        } else {
            index_map.insert(candidate.entity_name.clone(), vec![idx]);
        }
    }
    index_map
}

fn sort_custom_types(
    ts: &TypeSystem,
    mut types: Vec<AddTypeRequest>,
) -> anyhow::Result<Vec<AddTypeRequest>> {
    let mut graph: GraphMap<&str, (), Directed> = GraphMap::new();
    // map the type name to its position in the types array
    let mut ty_pos = HashMap::new();
    for (pos, ty) in types.iter().enumerate() {
        graph.add_node(ty.name.as_str());
        ty_pos.insert(ty.name.as_str(), pos);
        for field in &ty.field_defs {
            let field_type = field.field_type()?;
            match field_type {
                TypeEnum::Entity(name) if !field_type.is_builtin(ts)? => {
                    graph.add_node(name);
                    graph.add_edge(name, ty.name.as_str(), ());
                }
                _ => (),
            }
        }
    }

    let order = petgraph::algo::toposort(&graph, None)
        .map_err(|_| anyhow::anyhow!("cycle detected in models"))?
        .iter()
        .map(|ty| {
            ty_pos
                .get(ty)
                .copied()
                // this error should be caught earlier, the check is just an extra safety
                .ok_or_else(|| anyhow::anyhow!("unknown type {ty}"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut permutation = permutation::Permutation::oneline(order);

    permutation.apply_inv_slice_in_place(&mut types);

    Ok(types)
}

impl FieldDefinition {
    fn field_type(&self) -> Result<&TypeEnum> {
        self.field_type
            .as_ref()
            .with_context(|| format!("field_type of field '{}' is None", self.name))?
            .type_enum
            .as_ref()
            .with_context(|| format!("type_enum of field '{}' is None", self.name))
    }
}

impl ContainerType {
    fn value_type(&self) -> Result<&TypeEnum> {
        self.value_type
            .as_ref()
            .context("value_type of ContainerType is None")?
            .type_enum
            .as_ref()
            .context("type_enum of value_type of ContainerType is None")
    }
}

impl TypeEnum {
    fn is_builtin(&self, ts: &TypeSystem) -> Result<bool> {
        let is_builtin = match self {
            TypeEnum::String(_) | TypeEnum::Number(_) | TypeEnum::Bool(_) => true,
            TypeEnum::Entity(name) => ts.lookup_builtin_type(name).is_ok(),
            TypeEnum::Array(inner) => inner.value_type()?.is_builtin(ts)?,
        };
        Ok(is_builtin)
    }

    fn get_builtin(&self, ts: &TypeSystem) -> Result<Type> {
        let ty = match self {
            TypeEnum::String(_) => Type::String,
            TypeEnum::Number(_) => Type::Float,
            TypeEnum::Bool(_) => Type::Boolean,
            TypeEnum::Entity(name) => ts.lookup_builtin_type(name)?,
            TypeEnum::Array(inner) => Type::Array(Box::new(inner.value_type()?.get_builtin(ts)?)),
        };
        Ok(ty)
    }
}

impl From<Type> for TypeMsg {
    fn from(ty: Type) -> TypeMsg {
        let ty = match ty {
            Type::Float => TypeEnum::Number(true),
            Type::String => TypeEnum::String(true),
            Type::Boolean => TypeEnum::Bool(true),
            Type::Entity(entity) => TypeEnum::Entity(entity.name().to_owned()),
            Type::Array(elem_type) => {
                let inner_msg = (*elem_type).into();
                TypeEnum::Array(Box::new(ContainerType {
                    value_type: Some(Box::new(inner_msg)),
                }))
            }
        };
        TypeMsg {
            type_enum: Some(ty),
        }
    }
}
