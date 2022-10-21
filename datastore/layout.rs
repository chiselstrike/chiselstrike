use chisel_snapshot::{schema, serde_map_as_vec};
use indexmap::IndexMap;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Concrete representation of a [schema::Schema] in the database.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    #[serde(with = "layout_entity_tables")] // serialize as Vec<EntityTable>
    pub entity_tables: HashMap<schema::EntityName, Arc<EntityTable>>,
    pub schema: Arc<schema::Schema>,
}

/// An SQL table that stores instances of a given entity.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTable {
    pub entity_name: schema::EntityName,
    pub table_name: Name,
    pub id_col: IdColumn,
    #[serde(with = "entity_table_field_cols")] // serialize as Vec<FieldColumn>
    pub field_cols: IndexMap<String, Arc<FieldColumn>>,
}

/// Description of the SQL column that stores the entity id. This column is the primary key of its
/// table.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdColumn {
    pub col_name: Name,
    pub repr: IdRepr,
}

/// Description of an SQL column that stores an entity field.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldColumn {
    pub field_name: String,
    pub col_name: Name,
    pub repr: FieldRepr,
    #[serde(default)]
    pub nullable: bool,
}

/// Representation of a JavaScript id in SQL.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IdRepr {
    /// An UUID stored as an SQL text.
    UuidAsText,
    /// A JS string stored as an SQL text.
    StringAsText,
}

/// Representation of a JavaScript field in SQL column.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldRepr {
    /// A JS string stored as an SQL text.
    StringAsText,
    /// A JS number stored as an SQL double.
    NumberAsDouble,
    /// A JS boolean stored as an SQL integer.
    BooleanAsInt,
    /// A JS UUID stored as an SQL text.
    UuidAsText,
    /// A JS `Date` stored as an SQL double.
    JsDateAsDouble,
    /// Any value encoded in JSON and stored as an SQL text.
    AsJsonText,
}

/// An SQL identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Name(pub String);

serde_map_as_vec!(mod layout_entity_tables, HashMap<schema::EntityName, Arc<EntityTable>>, entity_name);
serde_map_as_vec!(mod entity_table_field_cols, IndexMap<String, Arc<FieldColumn>>, field_name);
