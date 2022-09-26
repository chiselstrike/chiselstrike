use crate::types::{ObjectType, Type, TypeId};
use crate::worker::WorkerState;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

#[deno_core::op]
fn op_chisel_get_type_system(state: &mut deno_core::OpState) -> SimpleTypeSystem {
    let ts = state.borrow::<WorkerState>().version.type_system.clone();
    let mut entities: HashMap<_, _> = ts
        .custom_types
        .iter()
        .map(|(name, ty)| (name.to_owned(), simplify_object_type(ty.object_type())))
        .collect();
    for (name, ty) in &ts.builtin.types {
        if let Type::Entity(entity) = ty {
            entities.insert(name.to_owned(), simplify_object_type(entity.object_type()));
        }
    }
    SimpleTypeSystem { entities }
}

/// Simplified version of our type system dedicated for internal use in our TypeScript
/// API. Hopefully, this structure is just temporary and will go away with the new
/// data model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimpleTypeSystem {
    entities: HashMap<String, SimpleEntity>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimpleEntity {
    name: String,
    fields: Vec<SimpleField>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimpleField {
    name: String,
    #[serde(rename = "type")]
    field_type: SimpleTypeId,
    is_optional: bool,
    is_unique: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "name")]
enum SimpleTypeId {
    String,
    Number,
    Boolean,
    JsDate,
    Entity {
        #[serde(rename = "entityName")]
        entity_name: String,
    },
    EntityId {
        #[serde(rename = "entityName")]
        entity_name: String,
    },
    Array {
        #[serde(rename = "elementType")]
        element_type: Box<SimpleTypeId>,
    },
}

fn simplify_object_type(obj: &Arc<ObjectType>) -> SimpleEntity {
    let fields = obj
        .all_fields()
        .map(|f| SimpleField {
            name: f.name.to_owned(),
            field_type: simplify_type_id(&f.type_id),
            is_optional: f.is_optional,
            is_unique: f.is_unique,
        })
        .collect();
    SimpleEntity {
        name: obj.name().to_owned(),
        fields,
    }
}

fn simplify_type_id(ty: &TypeId) -> SimpleTypeId {
    match ty {
        TypeId::String => SimpleTypeId::String,
        TypeId::Float => SimpleTypeId::Number,
        TypeId::Boolean => SimpleTypeId::Boolean,
        TypeId::JsDate => SimpleTypeId::JsDate,
        TypeId::Id => SimpleTypeId::String,
        TypeId::Array(element_ty) => SimpleTypeId::Array {
            element_type: simplify_type_id(element_ty).into(),
        },
        TypeId::Entity { name, .. } => SimpleTypeId::Entity {
            entity_name: name.to_owned(),
        },
        TypeId::EntityId(entity_name) => SimpleTypeId::EntityId {
            entity_name: entity_name.to_owned(),
        },
    }
}
