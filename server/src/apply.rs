// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use petgraph::graphmap::GraphMap;
use petgraph::Directed;
use url::Url;

use crate::datastore::{MetaService, QueryEngine};
use crate::feat_typescript_policies;
use crate::policies::PolicySystem;
use crate::proto::type_msg::TypeEnum;
use crate::proto::{
    AddTypeRequest, ApplyRequest, ContainerType, FieldDefinition, IndexCandidate,
    PolicyUpdateRequest, TypeMsg,
};
use crate::server::Server;
use crate::types::{
    DbIndex, Entity, Field, NewField, NewObject, ObjectType, Type, TypeSystem, TypeSystemError,
};
use crate::version::VersionInfo;

pub struct ApplyResult {
    pub type_system: TypeSystem,
    pub policy_system: PolicySystem,
    pub type_names_user_order: Vec<String>,
    pub labels: Vec<String>,
    pub policy_sources: Arc<HashMap<String, Box<[u8]>>>,
}

pub struct ParsedPolicies {
    policy_system: (PolicySystem, String),
    policy_sources: Arc<HashMap<String, Box<[u8]>>>,
}

impl ParsedPolicies {
    fn parse(request: &[PolicyUpdateRequest]) -> Result<Self> {
        let mut policy_system = None;
        let mut policy_sources = HashMap::new();

        for p in request {
            let path = PathBuf::from(&p.path);
            match path.extension().and_then(|s| s.to_str()) {
                Some("ts") if feat_typescript_policies() => {
                    let entity_name = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .and_then(|s| s.strip_suffix(".ts"))
                        .context("invalid policy path")?
                        .to_owned();
                    let policy_code = p.policy_config.as_bytes().to_vec().into_boxed_slice();
                    // Check that the policy code is valid
                    chiselc::policies::Policies::check(
                        &policy_code,
                        Url::from_file_path(path).unwrap(),
                    )?;
                    policy_sources.insert(entity_name, policy_code);
                }
                _ => {
                    if policy_system.is_none() {
                        policy_system.replace((
                            PolicySystem::from_yaml(&p.policy_config)?,
                            p.policy_config.clone(),
                        ));
                    } else {
                        anyhow::bail!("Currently only one policy file is supported");
                    }
                }
            }
        }

        Ok(Self {
            policy_system: policy_system.unwrap_or_default(),
            policy_sources: Arc::new(policy_sources),
        })
    }
}

pub async fn apply(
    server: Arc<Server>,
    apply_request: &ApplyRequest,
    type_system: &mut TypeSystem,
    version_id: String,
    version_info: &VersionInfo,
    modules: &HashMap<String, String>,
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

    let meta = &server.meta_service;
    let mut transaction = meta.begin_transaction().await?;

    for (existing, removed) in type_system.custom_types.iter() {
        if !type_names.contains(existing) {
            match meta.count_rows(&mut transaction, removed).await? {
                0 => to_remove.push(removed.clone()),
                cnt => to_remove_has_data.push((removed.clone(), cnt)),
            }
        }
    }

    if !to_remove_has_data.is_empty() && !apply_request.allow_type_deletion {
        let s = to_remove_has_data
            .iter()
            .map(|x| format!("{} ({} elements)", x.0.name(), x.1))
            .fold("\t".to_owned(), |acc, x| format!("{}\n\t{}", acc, x));
        bail!(
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
            bail!("custom type expected, got `{name}` instead");
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
                    None => {
                        bail!("field type `{entity_name}` is neither a built-in nor a custom type",)
                    }
                }
            } else if let TypeEnum::EntityId(entity_name) = field_ty {
                if !type_names.contains(entity_name) {
                    bail!(
                        "field `{}` of entity `{name}` is of type `Id<{entity_name}>`, but entity `{entity_name}` is undefined",
                        field.name
                    );
                }
                Type::EntityId(entity_name.to_owned())
            } else {
                bail!("field type must either be entity, entity id or be a builtin");
            };

            fields.push(Field::new(
                &NewField::new(&field.name, field_ty, &version_id)?,
                field.labels,
                field.default_value,
                field.is_optional,
                field.is_unique,
            ));
        }
        let ty_indexes = indexes.get(&name).cloned().unwrap_or_default();

        let ty = Arc::new(ObjectType::new(
            &NewObject::new(&name, &version_id),
            fields,
            ty_indexes,
        )?);

        new_types.insert(name.to_owned(), Entity::Custom(ty.clone()));

        match type_system.lookup_custom_type(&name) {
            Ok(old_type) => {
                let is_empty = meta.count_rows(&mut transaction, &old_type).await? == 0;
                let delta = type_system.generate_type_delta(&old_type, ty, is_empty)?;
                to_update.push((old_type.clone(), delta));
            }
            Err(TypeSystemError::NoSuchType(_)) => {
                to_insert.push(ty.clone());
            }
            Err(e) => bail!(e),
        }
    }

    let ParsedPolicies {
        policy_system: (policy_system, policy_system_str),
        policy_sources,
    } = ParsedPolicies::parse(&apply_request.policies)?;

    meta.persist_policy_sources(&mut transaction, &version_id, &policy_sources)
        .await?;
    meta.persist_policy_version(&mut transaction, &version_id, &policy_system_str)
        .await?;
    meta.persist_version_info(&mut transaction, &version_id, version_info)
        .await?;
    meta.persist_modules(&mut transaction, &version_id, modules)
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

    let labels: Vec<String> = policy_system.labels.keys().map(|x| x.to_owned()).collect();

    // Reload the type system so that we have new ids
    *type_system = meta
        .load_type_systems(&server.builtin_types)
        .await?
        .remove(&version_id)
        .unwrap_or_else(|| TypeSystem::new(server.builtin_types.clone(), version_id.clone()));

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
        .map(|ty| type_system.lookup_custom_type(ty.name()))
        .collect::<Result<Vec<_>, _>>()?;

    let to_update = to_update
        .into_iter()
        .map(|(ty, delta)| {
            let updated_ty = type_system.lookup_custom_type(ty.name());
            updated_ty.map(|ty| (ty, delta))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let query_engine = &server.query_engine;
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
        type_system: type_system.clone(),
        type_names_user_order,
        labels,
        policy_system,
        policy_sources,
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
) -> Result<Vec<AddTypeRequest>> {
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
        .map_err(|_| anyhow!("cycle detected in models"))?
        .iter()
        .map(|ty| {
            ty_pos
                .get(ty)
                .copied()
                // this error should be caught earlier, the check is just an extra safety
                .ok_or_else(|| anyhow!("unknown type {ty}"))
        })
        .collect::<Result<Vec<_>>>()?;

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
            TypeEnum::String(_)
            | TypeEnum::Number(_)
            | TypeEnum::Bool(_)
            | TypeEnum::JsDate(_)
            | TypeEnum::ArrayBuffer(_) => true,
            TypeEnum::Entity(name) | TypeEnum::EntityId(name) => {
                ts.lookup_builtin_type(name).is_ok()
            }
            TypeEnum::Array(inner) => inner.value_type()?.is_builtin(ts)?,
        };
        Ok(is_builtin)
    }

    fn get_builtin(&self, ts: &TypeSystem) -> Result<Type> {
        let ty = match self {
            TypeEnum::String(_) => Type::String,
            TypeEnum::Number(_) => Type::Float,
            TypeEnum::Bool(_) => Type::Boolean,
            TypeEnum::JsDate(_) => Type::JsDate,
            TypeEnum::ArrayBuffer(_) => Type::ArrayBuffer,
            TypeEnum::Entity(name) => ts.lookup_builtin_type(name)?,
            TypeEnum::EntityId(entity_name) => Type::EntityId(entity_name.to_owned()),
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
            Type::JsDate => TypeEnum::JsDate(true),
            Type::ArrayBuffer => TypeEnum::ArrayBuffer(true),
            Type::Entity(entity) => TypeEnum::Entity(entity.name().to_owned()),
            Type::EntityId(entity_name) => TypeEnum::EntityId(entity_name),
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
