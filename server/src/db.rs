// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::deno::current_api_version;
use crate::deno::get_policies;
use crate::policies::FieldPolicies;
use crate::query::engine::new_query_results;
use crate::query::engine::JsonObject;
use crate::query::engine::SqlStream;
use crate::runtime;
use crate::types::{Field, ObjectType, Type, TypeSystemError};
use anyhow::{anyhow, Result};
use enum_as_inner::EnumAsInner;
use futures::future;
use futures::StreamExt;
use serde_json::json;
use serde_json::value::Value;
use sqlx::any::{AnyPool, AnyRow};
use sqlx::Row;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, EnumAsInner)]
pub(crate) enum SqlValue {
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    String(String),
}

#[derive(Debug, Clone)]
pub(crate) enum SelectField {
    Builtin {
        name: String,
        type_: Type,
        column_idx: usize,
    },
    Nested {
        name: String,
        children: Vec<SelectField>,
    },
}

/// `SqlSelect` is a structure representing a query ready to be fired.
#[derive(Debug, Clone)]
pub(crate) struct SqlSelect {
    /// SQL query text
    raw_query: String,
    /// Nested structure representing a blueprint which is used to reconstruct
    /// multi-dimensional (=potentialy nested) JSON response from a linear
    /// sql response row.
    fields: Vec<SelectField>,
    /// Equality filters to be applied. Key is a name of field, value is the
    /// expected value. Currently doesn't support filtering based on nested
    /// fields.
    filters: HashMap<String, SqlValue>,
    /// JSON fields to be sent to user. This could be done in the builder directly,
    /// but many factors (including filters) need to be taken into account and it
    /// is better to do it later.
    allowed_columns: Option<HashSet<String>>,
    /// Field policies to be applied on the resulting response.
    policies: FieldPolicies,
}

#[derive(Debug, Clone)]
struct SqlJoin {
    rtype: Arc<ObjectType>,
    lkey: String,
    rkey: String,
    lalias: String,
    ralias: String,
}

struct SqlSelectBuilder {
    fields: Vec<SelectField>,
    columns: Vec<(String, Field)>,
    base_type: Arc<ObjectType>,
    joins: Vec<SqlJoin>,
    filters: HashMap<String, SqlValue>,
    allowed_columns: Option<HashSet<String>>,
    policies: FieldPolicies,
    limit: Option<u64>,
}

/// Class used to build `SqlSelect` from either JSON query or `ObjectType`.
/// The json part recursively descends through selected fields and captures all
/// joins necessary for nested types retrieval.
/// Once constructed, it can be further restricted by calling load_restrictions method.
/// When we are done with that, `build_sql_select` is called which creates a `SqlSelect`
/// structure that contains raw SQL query string and additional data necessary for
/// JSON response reconstruction and filtering.
impl SqlSelectBuilder {
    fn new_from_json(val: &serde_json::Value) -> Result<Self> {
        let name = val["name"].as_str().ok_or_else(|| {
            anyhow!(
                "internal error: `name` field is either missing or not a string: {}",
                val
            )
        })?;
        let runtime = runtime::get();
        let ts = &runtime.type_system;
        let api_version = current_api_version();
        let ty = match ts.lookup_builtin_type(name) {
            Ok(Type::Object(ty)) => ty,
            Err(TypeSystemError::NotABuiltinType(_)) => {
                ts.lookup_custom_type(name, &api_version)?
            }
            _ => anyhow::bail!("Unexpected type name as select base type: {}", name),
        };
        let policies = get_policies(&runtime, &ty)?;

        let mut builder = Self {
            fields: vec![],
            columns: vec![],
            base_type: ty.clone(),
            joins: vec![],
            filters: HashMap::default(),
            allowed_columns: None,
            policies,
            limit: val["limit"].as_u64(),
        };
        builder.fields = builder.load_fields(&ty, ty.backing_table(), &val["columns"])?;
        Ok(builder)
    }
    fn new_from_type(ty: &Arc<ObjectType>) -> Result<Self> {
        let mut builder = Self {
            fields: vec![],
            columns: vec![],
            base_type: ty.clone(),
            joins: vec![],
            filters: HashMap::default(),
            allowed_columns: None,
            policies: FieldPolicies::default(),
            limit: None,
        };

        for field in ty.all_fields() {
            let mut field = field.clone();
            field.type_ = match field.type_ {
                Type::Object(_) => Type::String, // This is actually a foreign key.
                ty => ty,
            };
            let field = builder.make_builtin_field(&field, field.name.as_str());
            builder.fields.push(field)
        }
        Ok(builder)
    }

    fn make_builtin_field(&mut self, field: &Field, column_name: &str) -> SelectField {
        let select_field = SelectField::Builtin {
            name: field.name.clone(),
            type_: field.type_.clone(),
            column_idx: self.columns.len(),
        };
        self.columns.push((column_name.to_owned(), field.clone()));
        select_field
    }

    fn load_fields(
        &mut self,
        ty: &Arc<ObjectType>,
        current_table: &str,
        columns: &serde_json::Value,
    ) -> Result<Vec<SelectField>> {
        let columns = columns.as_array().ok_or_else(|| {
            anyhow!(
                "internal error: `columns` object must be an array, got `{}`",
                columns
            )
        })?;
        let mut fields: Vec<SelectField> = vec![];
        for c in columns {
            let c = &c.as_array().ok_or_else(|| {
                // FIXME: This is ugly and the internal part is not necessary.
                anyhow!(
                    "internal error: column object must be an array, got `{}`",
                    c
                )
            })?[0];
            match c {
                Value::String(field_name) => {
                    let field = ty.field_by_name(field_name).ok_or_else(|| {
                        anyhow!(
                            "unknown field name `{}` in type `{}`",
                            field_name,
                            ty.name()
                        )
                    })?;
                    let column_name = format!("{}.{}", current_table, field_name);
                    let field = self.make_builtin_field(field, column_name.as_str());
                    fields.push(field);
                }
                Value::Object(nested_fields) => {
                    let field_name = nested_fields["field_name"]
                        .as_str()
                        .ok_or_else(|| anyhow!("name should be a string"))?;
                    let field = ty.field_by_name(field_name).ok_or_else(|| {
                        anyhow!(
                            "unknown field name `{}` in type `{}`",
                            field_name,
                            ty.name()
                        )
                    })?;
                    if let Type::Object(nested_ty) = &field.type_ {
                        let nested_table = format!(
                            "{}_JOIN{}_{}",
                            current_table,
                            self.joins.len(),
                            nested_ty.backing_table()
                        );
                        self.joins.push(SqlJoin {
                            rtype: nested_ty.clone(),
                            lkey: field_name.to_owned(),
                            rkey: "id".to_owned(),
                            lalias: current_table.to_owned(),
                            ralias: nested_table.to_owned(),
                        });

                        let nested_fields =
                            self.load_fields(nested_ty, &nested_table, &nested_fields["columns"])?;
                        fields.push(SelectField::Nested {
                            name: field.name.clone(),
                            children: nested_fields,
                        });
                    } else {
                        anyhow::bail!(
                            "found nested column selection on field that is not an object"
                        )
                    }
                }
                _ => anyhow::bail!("expected String or Object, got `{}`", c),
            }
        }
        Ok(fields)
    }

    fn update_allowed_columns(&mut self, columns_json: &serde_json::Value) -> Result<()> {
        if let Some(columns) = columns_json.as_array() {
            let mut allowed_columns = HashSet::<String>::default();
            for c in columns {
                let c = &c.as_array().ok_or_else(|| {
                    // FIXME: This is ugly and the internal part is not necessary.
                    anyhow!(
                        "internal error: column object must be an array, got `{}`",
                        c
                    )
                })?[0];
                if let Value::String(field_name) = c {
                    allowed_columns.insert(field_name.to_owned());
                }
            }
            self.allowed_columns = Some(allowed_columns);
        }
        Ok(())
    }

    fn load_restrictions(&mut self, rest_json: &serde_json::Value) -> Result<()> {
        if let Some(limit) = rest_json["limit"].as_u64() {
            self.limit = Some(limit);
        }
        self.update_allowed_columns(&rest_json["columns"])?;
        let restrictions = rest_json["restrictions"]
            .as_object()
            .ok_or_else(|| anyhow!("Missing restrictions in filter"))?;
        for (k, v) in restrictions.iter() {
            anyhow::ensure!(
                self.base_type.field_by_name(k).is_some(),
                "trying to filter by non-existent field `{}`",
                k
            );
            let v = match v {
                Value::Null => anyhow::bail!("Null restriction"),
                Value::Bool(v) => SqlValue::Bool(*v),
                Value::Number(v) => {
                    if let Some(v) = v.as_u64() {
                        SqlValue::U64(v)
                    } else if let Some(v) = v.as_i64() {
                        SqlValue::I64(v)
                    } else {
                        SqlValue::F64(v.as_f64().unwrap())
                    }
                }
                Value::String(v) => SqlValue::String(v.clone()),
                Value::Array(v) => anyhow::bail!("Array restriction {:?}", v),
                Value::Object(v) => anyhow::bail!("Object restriction {:?}", v),
            };
            self.filters.insert(k.clone(), v);
        }
        Ok(())
    }

    fn make_column_string(&self) -> String {
        let mut column_string = String::new();
        for (column_name, field) in &self.columns {
            let col = match field.default_value() {
                Some(dfl) => format!(
                    "coalesce({},\"{}\") AS {},",
                    column_name,
                    dfl,
                    column_name.replace(".", "_")
                ),
                None => format!("{},", column_name),
            };
            column_string += &col;
        }
        column_string.pop();
        column_string
    }

    fn make_join_string(&self) -> String {
        let mut join_string = String::new();
        for join in &self.joins {
            join_string += &format!(
                "JOIN ({}) AS {} ON {}.{}={}.{}\n",
                join.rtype.backing_table(),
                join.ralias,
                join.lalias,
                join.lkey,
                join.ralias,
                join.rkey
            );
        }
        join_string
    }

    fn make_raw_query(&self) -> String {
        let column_string = self.make_column_string();
        let join_string = self.make_join_string();

        let mut raw_query = format!(
            "SELECT {} FROM {} {}",
            column_string,
            self.base_type.backing_table(),
            join_string
        );
        if let Some(limit) = self.limit {
            raw_query += format!(" LIMIT {}", limit).as_str();
        }
        raw_query
    }

    fn build_sql_select(&self) -> SqlSelect {
        SqlSelect {
            raw_query: self.make_raw_query(),
            fields: self.fields.clone(),
            filters: self.filters.clone(),
            allowed_columns: self.allowed_columns.clone(),
            policies: self.policies.clone(),
        }
    }
}

pub(crate) fn select_from_type(ty: &Arc<ObjectType>) -> Result<SqlSelect> {
    let builder = SqlSelectBuilder::new_from_type(ty)?;
    Ok(builder.build_sql_select())
}

pub(crate) fn convert_to_select(val: &serde_json::Value) -> Result<SqlSelect> {
    let builder = convert_to_select_builder(val)?;
    Ok(builder.build_sql_select())
}

fn convert_to_select_builder(val: &serde_json::Value) -> Result<SqlSelectBuilder> {
    let kind = val["kind"].as_str().ok_or_else(|| {
        anyhow!(
            "internal error: `kind` field is either missing or not a string: {}",
            val
        )
    })?;

    match kind {
        "BackingStore" => SqlSelectBuilder::new_from_json(val),
        "Join" => anyhow::bail!("join is not supported"),
        "Filter" => {
            let mut select = convert_to_select_builder(&val["inner"])?;
            select.load_restrictions(val)?;
            Ok(select)
        }
        _ => anyhow::bail!("unexpected relation kind `{}`", kind),
    }
}

fn row_to_json(fields: &[SelectField], row: &AnyRow) -> anyhow::Result<JsonObject> {
    let mut ret = JsonObject::default();
    for s_field in fields {
        match s_field {
            SelectField::Builtin {
                name,
                type_,
                column_idx,
            } => {
                let i = column_idx;
                // FIXME: consider result_column.type_info().is_null() too
                macro_rules! to_json {
                    ($value_type:ty) => {{
                        let val = row.get::<$value_type, _>(i);
                        json!(val)
                    }};
                }
                let val = match type_ {
                    Type::Float => {
                        // https://github.com/launchbadge/sqlx/issues/1596
                        // sqlx gets confused if the float doesn't have decimal points.
                        let val: &str = row.get_unchecked(i);
                        json!(val.parse::<f64>()?)
                    }
                    Type::String => to_json!(&str),
                    Type::Id => to_json!(&str),
                    Type::Boolean => {
                        // Similarly to the float issue, type information is not filled in
                        // *if* this value was put in as a result of coalesce() (default).
                        //
                        // Also the database has integers, and we need to map it back to a
                        // boolean type on json.
                        let val: &str = row.get_unchecked(i);
                        let x: bool = val.parse::<usize>()? == 1;
                        json!(x)
                    }
                    Type::Object(_) => anyhow::bail!("object is not a builtin"),
                };
                ret.insert(name.clone(), val);
            }
            SelectField::Nested { name, children } => {
                let val = json!(row_to_json(children, row)?);
                ret.insert(name.clone(), val);
            }
        }
    }
    Ok(ret)
}

fn filter_stream_item(
    o: &anyhow::Result<JsonObject>,
    restrictions: &HashMap<String, SqlValue>,
) -> bool {
    let o = match o {
        Ok(o) => o,
        Err(_) => return true,
    };
    for (k, v) in o.iter() {
        if let Some(v2) = restrictions.get(k) {
            let eq = match v2 {
                SqlValue::Bool(v2) => v == v2,
                // FIXME: This is very unfortunate consequence of
                // storing all ints as floats.
                SqlValue::U64(v2) => v == *v2 as f64,
                SqlValue::I64(v2) => v == *v2 as f64,
                SqlValue::F64(v2) => v == v2,
                SqlValue::String(v2) => v == v2,
            };
            if !eq {
                return false;
            }
        }
    }
    true
}
fn filter_columns(
    o: anyhow::Result<JsonObject>,
    allowed_columns: &Option<HashSet<String>>,
) -> anyhow::Result<JsonObject> {
    let mut o = o?;
    if let Some(allowed_columns) = &allowed_columns {
        let removed_keys = o
            .iter()
            .map(|(k, _)| k.to_owned())
            .filter(|k| !allowed_columns.contains(k))
            .collect::<Vec<String>>();
        for k in &removed_keys {
            o.remove(k);
        }
    }
    Ok(o)
}

/// FIXME: This function should perform recursive descend into nested fields.
fn apply_policies(
    o: anyhow::Result<JsonObject>,
    policies: &FieldPolicies,
) -> anyhow::Result<JsonObject> {
    let mut o = o?;
    for (k, v) in o.iter_mut() {
        if let Some(xform) = policies.get(k) {
            *v = xform(v.take());
        }
    }
    Ok(o)
}

pub(crate) fn run_select(pool: &AnyPool, select: SqlSelect) -> Result<SqlStream> {
    let policies = select.policies;
    let filters = select.filters;
    let allowed_columns = select.allowed_columns;

    let stream = new_query_results(select.raw_query, pool);
    let stream = stream.map(move |row| row_to_json(&select.fields, &row?));
    let stream = stream.filter(move |o| future::ready(filter_stream_item(o, &filters)));
    let stream = Box::pin(stream.map(move |o| {
        let o = filter_columns(o, &allowed_columns);
        apply_policies(o, &policies)
    }));
    Ok(stream)
}
