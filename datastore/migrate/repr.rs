use anyhow::{Result, Context, bail};
use std::sync::Arc;
use chisel_snapshot::{schema, typecheck};
use crate::layout;

/// Gets the best representation for a new id column.
pub fn new_id_repr(id_type: schema::IdType) -> layout::IdRepr {
    match id_type {
        schema::IdType::Uuid => layout::IdRepr::UuidAsText,
        schema::IdType::String => layout::IdRepr::StringAsText,
    }
}

/// Computes the new representation of an id column when the id has changed its type.
pub fn update_id_repr(
    old_repr: layout::IdRepr,
    old_type: schema::IdType,
    new_type: schema::IdType,
) -> Result<layout::IdRepr> {
    Ok(match (old_repr, new_type) {
        (layout::IdRepr::UuidAsText, schema::IdType::Uuid) => layout::IdRepr::UuidAsText,
        (layout::IdRepr::UuidAsText, schema::IdType::String) => layout::IdRepr::StringAsText,
        (layout::IdRepr::StringAsText, schema::IdType::String) => layout::IdRepr::StringAsText,
        _ => bail!("cannot migrate id from {:?} to {:?}", old_type, new_type),
    })
}

/// Gets the best representation for a new field column of given type.
pub fn new_field_repr(schema: &schema::Schema, type_: &schema::Type) -> layout::FieldRepr {
    match type_ {
        schema::Type::Typedef(type_name) =>
            new_field_repr(schema, &schema.typedefs[type_name]),
        schema::Type::Ref(entity_name, _) =>
            new_primitive_repr(schema.entities[entity_name].id_type.as_primitive_type()),
        schema::Type::Primitive(type_) =>
            new_primitive_repr(*type_),
        _ => layout::FieldRepr::AsJsonText,
    }
}

fn new_primitive_repr(type_: schema::PrimitiveType) -> layout::FieldRepr {
    match type_ {
        schema::PrimitiveType::String => layout::FieldRepr::StringAsText,
        schema::PrimitiveType::Number => layout::FieldRepr::NumberAsDouble,
        schema::PrimitiveType::Boolean => layout::FieldRepr::BooleanAsInt,
        schema::PrimitiveType::Uuid => layout::FieldRepr::UuidAsText,
        schema::PrimitiveType::JsDate => layout::FieldRepr::JsDateAsDouble,
    }
}

/// Computes the new representation of a column for a field that has changed its type, using its
/// old representation. This must keep the SQL type intact.
pub fn update_field_repr(
    old_schema: &schema::Schema,
    new_schema: &schema::Schema,
    old_repr: layout::FieldRepr,
    old_type: &Arc<schema::Type>,
    new_type: &Arc<schema::Type>,
) -> Result<layout::FieldRepr> {
    let is_new_subtype_of = |src_type: &Arc<schema::Type>| -> bool {
        typecheck::check_subtype(
            new_schema, new_type, new_schema, src_type,
            typecheck::TypeVariant::Plain
        ).is_ok()
    };
    let check_new_supertype_of = |src_type: &Arc<schema::Type>| -> Result<()> {
        typecheck::check_subtype(
            new_schema, src_type, new_schema, new_type,
            typecheck::TypeVariant::Plain
        )
    };

    Ok(match old_repr {
        layout::FieldRepr::StringAsText => {
            check_new_supertype_of(&schema::TYPE_STRING)
                .context("field must keep compatibility with string")?;
            layout::FieldRepr::StringAsText
        },
        layout::FieldRepr::NumberAsDouble => {
            check_new_supertype_of(&schema::TYPE_NUMBER)
                .context("field must keep compatibility with number")?;
            layout::FieldRepr::NumberAsDouble
        },
        layout::FieldRepr::BooleanAsInt => {
            check_new_supertype_of(&schema::TYPE_BOOLEAN)
                .context("field must keep compatibility with boolean")?;
            layout::FieldRepr::BooleanAsInt
        },
        layout::FieldRepr::UuidAsText => {
            if is_new_subtype_of(&schema::TYPE_UUID) {
                check_new_supertype_of(&schema::TYPE_STRING)
                    .context("field must keep compatibility with Uuid")?;
                layout::FieldRepr::UuidAsText
            } else {
                check_new_supertype_of(&schema::TYPE_STRING)
                    .context("field must keep compatibility with Uuid or string")?;
                layout::FieldRepr::StringAsText
            }
        },
        layout::FieldRepr::JsDateAsDouble => {
            if is_new_subtype_of(&schema::TYPE_JS_DATE) {
                check_new_supertype_of(&schema::TYPE_NUMBER)
                    .context("field must keep compatibility with Date")?;
                layout::FieldRepr::JsDateAsDouble
            } else {
                check_new_supertype_of(&schema::TYPE_NUMBER)
                    .context("field must keep compatibility with Date or number")?;
                layout::FieldRepr::NumberAsDouble
            }
        },
        layout::FieldRepr::AsJsonText => {
            typecheck::check_subtype(old_schema, old_type, new_schema, new_type, typecheck::TypeVariant::Plain)
                .context("field must keep compatibility with previous type")?;
            layout::FieldRepr::AsJsonText
        },
    })
}
