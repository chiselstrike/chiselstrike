use anyhow::{Result, Context, bail, ensure};
use chisel_snapshot::{schema, typecheck};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;
use crate::layout;
use super::repr;

#[derive(Debug)]
pub struct PlanOpts {
    pub table_prefix: String,
}

#[derive(Debug)]
pub struct Plan {
    pub new_layout: layout::Layout,
    pub steps: Vec<Step>,
}

#[derive(Debug)]
pub enum Step {
    AddTable(AddTable),
    RemoveTable(RemoveTable),
    AddColumn(AddColumn),
    RemoveColumn(RemoveColumn),
    UpdateColumn(UpdateColumn),
}

#[derive(Debug)]
pub struct AddTable {
    pub new_table: Arc<layout::EntityTable>,
}

#[derive(Debug)]
pub struct RemoveTable {
    pub old_table_name: layout::Name,
}

#[derive(Debug)]
pub struct AddColumn {
    pub table_name: layout::Name,
    pub new_col: Arc<layout::FieldColumn>,
    pub value: schema::Value,
}

#[derive(Debug)]
pub struct RemoveColumn {
    pub table_name: layout::Name,
    pub old_col_name: layout::Name,
}

#[derive(Debug)]
pub struct UpdateColumn {
    pub table_name: layout::Name,
    pub col_name: layout::Name,
    pub new_col: Arc<layout::FieldColumn>,
    pub new_nullable: Option<bool>,
}

pub fn plan_migration(
    old_layout: &layout::Layout,
    new_schema: Arc<schema::Schema>,
    opts: &PlanOpts,
) -> Result<Plan> {
    let mut steps = Vec::new();
    let mut new_entity_tables = HashMap::new();

    for new_entity in new_schema.entities.values() {
        let new_table = match old_layout.entity_tables.get(&new_entity.name) {
            Some(old_table) => plan_update_table(
                    &old_layout.schema, &new_schema, old_table, new_entity, &mut steps)
                .with_context(|| format!("could not migrate table for entity {:?}", new_entity.name))?,
            None => plan_add_table(opts, &new_schema, new_entity, &mut steps),
        };
        new_entity_tables.insert(new_table.entity_name.clone(), new_table);
    }

    for old_table in old_layout.entity_tables.values() {
        if !new_schema.entities.contains_key(&old_table.entity_name) {
            plan_remove_table(old_table, &mut steps);
        }
    }

    let new_layout = layout::Layout {
        entity_tables: new_entity_tables,
        schema: new_schema,
    };
    Ok(Plan { new_layout, steps })
}

//
// tables
//

fn plan_add_table(
    opts: &PlanOpts,
    new_schema: &schema::Schema,
    new_entity: &schema::Entity,
    out_steps: &mut Vec<Step>,
) -> Arc<layout::EntityTable> {
    let id_col = plan_id_col(new_entity.id_type);
    let field_cols = new_entity.fields.values()
        .map(|field| {
            (field.name.clone(), plan_field_col(new_schema, field))
        })
        .collect();

    let entity_name = new_entity.name.clone();
    let table_name = gen_table_name(opts, &new_entity.name);
    let table = Arc::new(layout::EntityTable { entity_name, table_name, id_col, field_cols });
    out_steps.push(Step::AddTable(AddTable { new_table: table.clone() }));
    table
}

fn gen_table_name(opts: &PlanOpts, entity_name: &schema::EntityName) -> layout::Name {
    let (name_prefix, name) = match entity_name {
        schema::EntityName::User(name) => ('u', name),
        schema::EntityName::Builtin(name) => ('b', name),
    };
    layout::Name(format!("{}{}_{}", opts.table_prefix, name_prefix, name))
}

fn plan_update_table(
    old_schema: &schema::Schema,
    new_schema: &schema::Schema,
    old_table: &layout::EntityTable,
    new_entity: &schema::Entity,
    out_steps: &mut Vec<Step>,
) -> Result<Arc<layout::EntityTable>> {
    let old_entity = &old_schema.entities[&old_table.entity_name];

    let new_id_col = plan_update_id_col(&old_table.id_col, old_entity.id_type, new_entity.id_type)
        .context("could not migrate entity id")?;

    let mut new_field_cols = IndexMap::new();
    for new_field in new_entity.fields.values() {
        let new_field_col = match old_table.field_cols.get(&new_field.name) {
            Some(old_field_col) => plan_update_field_col(
                    old_schema, new_schema, old_table, old_field_col, new_field, out_steps)
                .with_context(|| format!("could not migrate column for field {:?}", new_field.name))?,
            None => plan_add_field_col(new_schema, old_table, new_field, out_steps)
                .with_context(|| format!("could not add column for field {:?}", new_field.name))?,
        };
        new_field_cols.insert(new_field.name.clone(), new_field_col);
    }

    for old_field_col in old_table.field_cols.values() {
        if !new_entity.fields.contains_key(&old_field_col.field_name) {
            plan_remove_column(old_table, old_field_col, out_steps);
        }
    }

    let new_table = layout::EntityTable {
        entity_name: new_entity.name.clone(),
        table_name: old_table.table_name.clone(),
        id_col: new_id_col,
        field_cols: new_field_cols,
    };
    Ok(Arc::new(new_table))
}

fn plan_remove_table(old_table: &layout::EntityTable, out_steps: &mut Vec<Step>) {
    out_steps.push(Step::RemoveTable(RemoveTable { old_table_name: old_table.table_name.clone() }));
}

//
// id column
//

fn plan_id_col(id_type: schema::IdType) -> layout::IdColumn {
    let repr = repr::new_id_repr(id_type);
    let col_name = layout::Name("id".into());
    layout::IdColumn { col_name, repr }
}

fn plan_update_id_col(
    old_col: &layout::IdColumn,
    old_type: schema::IdType,
    new_type: schema::IdType,
) -> Result<layout::IdColumn> {
    let new_repr = repr::update_id_repr(old_col.repr, old_type, new_type)?;
    Ok(layout::IdColumn {
        col_name: old_col.col_name.clone(),
        repr: new_repr,
    })
}

//
// field columns
//

fn plan_field_col(schema: &schema::Schema, field: &schema::EntityField) -> Arc<layout::FieldColumn> {
    let (repr, nullable) = match typecheck::is_optional(schema, &field.type_) {
        Some(type_) => (repr::new_field_repr(schema, &type_), true),
        None => (repr::new_field_repr(schema, &field.type_), false),
    };

    let field_name = field.name.clone();
    let col_name = layout::Name(field_name.clone());
    Arc::new(layout::FieldColumn { field_name, col_name, repr, nullable })
}

fn plan_add_field_col(
    new_schema: &schema::Schema,
    old_table: &layout::EntityTable,
    new_field: &schema::EntityField,
    out_steps: &mut Vec<Step>,
) -> Result<Arc<layout::FieldColumn>> {
    let value = new_field.default.clone()
        .context("the field does not have a default value")?;
    let new_col = plan_field_col(new_schema, new_field);
    out_steps.push(Step::AddColumn(AddColumn {
        table_name: old_table.table_name.clone(),
        new_col: new_col.clone(),
        value: value.clone(),
    }));
    Ok(new_col)
}

fn plan_update_field_col(
    old_schema: &schema::Schema,
    new_schema: &schema::Schema,
    old_table: &layout::EntityTable,
    old_col: &layout::FieldColumn,
    new_field: &schema::EntityField,
    out_steps: &mut Vec<Step>,
) -> Result<Arc<layout::FieldColumn>> {
    let old_entity = &old_schema.entities[&old_table.entity_name];
    let old_field = &old_entity.fields[&new_field.name];

    match (&old_field.default, &new_field.default) {
        (None, None) => (),
        (Some(_), None) => bail!("cannot remove the default value from an existing field"),
        (None, Some(_)) => bail!("cannot add a default value to an existing field"),
        (Some(old), Some(new)) => ensure!(old == new, "cannot change the default value of an existing field"),
    };

    let (old_type, new_type) = (&old_field.type_, &new_field.type_);
    let old_type_opt = typecheck::is_optional(old_schema, old_type);
    let new_type_opt = typecheck::is_optional(new_schema, new_type);
    let (old_type, new_type, new_nullable) = match (old_type_opt, new_type_opt) {
        (Some(old_type), Some(new_type)) => (old_type, new_type, true),
        (None, None) => (old_type, new_type, false),
        (None, Some(new_type)) => (old_type, new_type, true),
        (Some(_old_type), None) => bail!("cannot change field from optional to required"),
    };

    let new_repr = repr::update_field_repr(old_schema, new_schema, old_col.repr, old_type, new_type)?;

    let new_col = Arc::new(layout::FieldColumn {
        field_name: new_field.name.clone(),
        col_name: old_col.col_name.clone(),
        repr: new_repr,
        nullable: new_nullable,
    });

    if new_col.nullable != old_col.nullable {
        out_steps.push(Step::UpdateColumn(UpdateColumn {
            table_name: old_table.table_name.clone(),
            col_name: old_col.col_name.clone(),
            new_col: new_col.clone(),
            new_nullable: Some(new_col.nullable),
        }));
    }

    Ok(new_col)
}

fn plan_remove_column(
    old_table: &layout::EntityTable,
    old_col: &layout::FieldColumn,
    out_steps: &mut Vec<Step>,
) {
    out_steps.push(Step::RemoveColumn(RemoveColumn {
        table_name: old_table.table_name.clone(),
        old_col_name: old_col.col_name.clone(),
    }));
}
