// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::deno::current_api_version;
use crate::deno::make_field_policies;
use crate::policies::FieldPolicies;

use crate::runtime;
use crate::types::{Field, ObjectType, Type, TypeSystemError, OAUTHUSER_TYPE_NAME};
use crate::JsonObject;

use anyhow::{anyhow, Result};
use enum_as_inner::EnumAsInner;
use serde_json::value::Value;
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

impl From<&str> for SqlValue {
    fn from(f: &str) -> Self {
        Self::String(f.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct Restriction {
    /// Table in which `column` can be found. If unspecified, base select table will be used.
    table: Option<String>,
    /// Database column name used for equality restriction.
    column: String,
    /// The value used to restrict the results.
    v: SqlValue,
}

#[derive(Debug, Clone)]
pub(crate) enum SelectField {
    Scalar {
        /// Name of the original Type field
        name: String,
        /// Type of the field
        type_: Type,
        is_optional: bool,
        /// Index of a column containing this field in the resulting row we get from
        /// the database.
        column_idx: usize,
        /// Policy transformation to be applied on the resulting JSON value.
        transform: Option<fn(Value) -> Value>,
    },
    Entity {
        /// Name of the original Type field
        name: String,
        is_optional: bool,
        /// Nested fields of the Entity object.
        children: Vec<SelectField>,
        /// Policy transformation to be applied on the resulting JSON value.
        transform: Option<fn(Value) -> Value>,
    },
}

/// `Query` is a structure that represents an executable query.
///
/// A query represents a full query including filtering, projection, joins,
/// and so on. The `execute` method of `QueryEngine` executes these queries
/// using SQL and the policy engine.
#[derive(Debug, Clone)]
pub(crate) struct Query {
    /// SQL query text
    pub(crate) raw_sql: String,
    /// Nested structure representing a blueprint which is used to reconstruct
    /// multi-dimensional (=potentialy nested) JSON response from a linear
    /// sql response row. Each element represents a selected field (potentially nested)
    /// of the selected base type.
    pub(crate) fields: Vec<SelectField>,
    /// Entity fields selected by the user. This field is used to post-filter fields that
    /// shall be returned to the user in JSON.
    /// FIXME: The post-filtering is suboptimal solution and selection should happen when
    /// we build the raw_sql query.
    pub(crate) allowed_fields: Option<HashSet<String>>,
}

/// Represents JOIN operator joining `lalias` table using `lkey` with Entity of `rtype`
/// whose data are stored in `ralias` table and joined using `rkey`.
#[derive(Debug, Clone)]
struct Join {
    rtype: Arc<ObjectType>,
    lkey: String,
    rkey: String,
    lalias: String,
    ralias: String,
}

/// Class used to build `Query` from either JSON query or `ObjectType`.
/// The json part recursively descends through selected fields and captures all
/// joins necessary for nested types retrieval.
/// Once constructed, it can be further restricted by calling load_restrictions method.
/// When we are done with that, `build` is called which creates a `Query`
/// structure that contains raw SQL query string and additional data necessary for
/// JSON response reconstruction and filtering.
struct QueryBuilder {
    /// Recursive vector used to reconstruct nested entities based on flat vector of columns
    /// returned by the database.
    fields: Vec<SelectField>,
    /// Vector of SQL column aliases that will be selected from the database and corresponding
    /// scalar fields.
    columns: Vec<(String, String, Field)>,
    base_type: Arc<ObjectType>,
    joins: Vec<Join>,
    restrictions: Vec<Restriction>,
    /// List of fields to be returned to the user.
    allowed_fields: Option<HashSet<String>>,
    /// Limits how many rows/entries will be returned to the user.
    limit: Option<u64>,
}

impl QueryBuilder {
    fn new(base_type: Arc<ObjectType>) -> Self {
        Self {
            fields: vec![],
            columns: vec![],
            base_type,
            joins: vec![],
            restrictions: vec![],
            allowed_fields: None,
            limit: None,
        }
    }

    /// Constructs a query builder ready to build an expression querying all fields of a
    /// given type `ty`. This is done in a shallow manner. Columns representing foreign
    /// key are returned as string, not as the related Entity.
    fn from_type(ty: &Arc<ObjectType>) -> Self {
        let mut builder = Self::new(ty.clone());
        for field in ty.all_fields() {
            let mut field = field.clone();
            field.type_ = match field.type_ {
                Type::Object(_) => Type::String, // This is actually a foreign key.
                ty => ty,
            };
            let field =
                builder.make_scalar_field(&field, ty.backing_table(), field.name.as_str(), None);
            builder.fields.push(field)
        }
        builder
    }

    /// Constructs a builder from a type name `ty_name`.
    fn new_from_type_name(ty_name: &str) -> Result<Self> {
        let runtime = runtime::get();
        let ts = &runtime.type_system;
        let api_version = current_api_version();
        let ty = match ts.lookup_builtin_type(ty_name) {
            Ok(Type::Object(ty)) => ty,
            Err(TypeSystemError::NotABuiltinType(_)) => {
                ts.lookup_custom_type(ty_name, &api_version)?
            }
            _ => anyhow::bail!("Unexpected type name as select base type: {}", ty_name),
        };

        let mut builder = Self::new(ty.clone());
        builder.fields = builder.load_fields(&runtime, &ty, ty.backing_table());
        Ok(builder)
    }

    fn make_scalar_field(
        &mut self,
        field: &Field,
        table_name: &str,
        column_name: &str,
        transform: Option<fn(Value) -> Value>,
    ) -> SelectField {
        let select_field = SelectField::Scalar {
            name: field.name.clone(),
            type_: field.type_.clone(),
            is_optional: field.is_optional,
            column_idx: self.columns.len(),
            transform,
        };
        self.columns
            .push((table_name.to_owned(), column_name.to_owned(), field.clone()));
        select_field
    }

    /// Loads all Fields of a given type `ty` to be retrieved from the
    /// database. For fields that represent a nested Entity a join is
    /// generated and we attempt to retrieve them recursivelly as well.
    fn load_fields(
        &mut self,
        runtime: &runtime::Runtime,
        ty: &Arc<ObjectType>,
        current_table: &str,
    ) -> Vec<SelectField> {
        let policies = make_field_policies(runtime, ty);
        self.add_login_restrictions(ty, current_table, &policies);

        let mut fields = vec![];
        for field in ty.all_fields() {
            if let Type::Object(nested_ty) = &field.type_ {
                let nested_table = format!(
                    "{}_JOIN{}_{}",
                    current_table,
                    self.joins.len(),
                    nested_ty.backing_table()
                );
                self.joins.push(Join {
                    rtype: nested_ty.clone(),
                    lkey: field.name.to_owned(),
                    rkey: "id".to_owned(),
                    lalias: current_table.to_owned(),
                    ralias: nested_table.to_owned(),
                });
                let field_policy = policies.transforms.get(&field.name).cloned();

                fields.push(SelectField::Entity {
                    name: field.name.clone(),
                    is_optional: field.is_optional,
                    children: self.load_fields(runtime, nested_ty, &nested_table),
                    transform: field_policy,
                });
            } else {
                let field_policy = policies.transforms.get(&field.name).cloned();
                let field = self.make_scalar_field(field, current_table, &field.name, field_policy);
                fields.push(field)
            }
        }
        fields
    }

    fn add_login_restrictions(
        &mut self,
        ty: &Arc<ObjectType>,
        current_table: &str,
        policies: &FieldPolicies,
    ) {
        let current_userid = match &policies.current_userid {
            None => "NULL".to_owned(),
            Some(id) => id.to_owned(),
        };
        for field_name in &policies.match_login {
            match ty.field_by_name(field_name) {
                Some(Field {
                    type_: Type::Object(obj),
                    ..
                }) if obj.name() == OAUTHUSER_TYPE_NAME => {
                    self.restrictions.push(Restriction {
                        table: Some(current_table.to_owned()),
                        column: field_name.to_owned(),
                        v: SqlValue::String(current_userid.clone()),
                    });
                }
                _ => {}
            }
        }
    }

    fn update_limit(&mut self, limit: u64) {
        self.limit = Some(std::cmp::min(limit, self.limit.unwrap_or(limit)));
    }

    fn update_allowed_fields(&mut self, columns: &[serde_json::Value]) -> Result<()> {
        let mut allowed_fields = HashSet::<String>::default();
        for c in columns {
            let field_name = c
                .as_str()
                .ok_or_else(|| anyhow!("internal error: got column unexpected type: `{}`", c))?;
            allowed_fields.insert(field_name.to_owned());
        }
        self.allowed_fields = Some(allowed_fields);
        Ok(())
    }

    fn load_restrictions(&mut self, restrictions: &JsonObject) -> Result<()> {
        let restrictions = convert_restrictions(restrictions)?;
        for r in restrictions {
            anyhow::ensure!(
                self.base_type.field_by_name(&r.column).is_some(),
                "trying to filter by non-existent field `{}`",
                r.column
            );
            self.restrictions.push(r);
        }
        Ok(())
    }

    fn make_column_string(&self) -> String {
        let mut column_string = String::new();
        for (table_name, column_name, field) in &self.columns {
            let col = match field.default_value() {
                Some(dfl) => format!(
                    "coalesce(\"{}\".\"{}\",'{}') AS \"{}_{}\",",
                    table_name, column_name, dfl, table_name, column_name
                ),
                None => format!("\"{}\".\"{}\",", table_name, column_name),
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
                "LEFT JOIN \"{}\" AS \"{}\" ON \"{}\".\"{}\"=\"{}\".\"{}\"\n",
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
        let raw_restrictions = make_restriction_string(&self.restrictions);

        let mut raw_sql = format!(
            "SELECT {} FROM \"{}\" {} {}",
            column_string,
            self.base_type.backing_table(),
            join_string,
            raw_restrictions
        );
        if let Some(limit) = self.limit {
            raw_sql += format!(" LIMIT {}", limit).as_str();
        }
        raw_sql
    }

    fn build(&self) -> Query {
        Query {
            raw_sql: self.make_raw_query(),
            fields: self.fields.clone(),
            allowed_fields: self.allowed_fields.clone(),
        }
    }
}

// FIXME: We should use prepared statements instead
fn escape_string(s: &str) -> String {
    format!("{}", format_sql_query::QuotedData(s))
}

/// Convert a vector of `Restriction` objects into a SQL `WHERE` clause.
pub(crate) fn make_restriction_string(restrictions: &[Restriction]) -> String {
    restrictions.iter().fold(String::new(), |acc, rest| {
        let str_v = match &rest.v {
            SqlValue::Bool(v) => (if *v { "1" } else { "0" }).to_string(),
            SqlValue::U64(v) => format!("{}", v),
            SqlValue::I64(v) => format!("{}", v),
            SqlValue::F64(v) => format!("{}", v),
            SqlValue::String(v) => escape_string(v),
        };
        let equality = if let Some(table) = &rest.table {
            format!("\"{}\".\"{}\"={}", table, rest.column, str_v)
        } else {
            format!("\"{}\"={}", rest.column, str_v)
        };
        if acc.is_empty() {
            format!("WHERE {}", equality)
        } else {
            format!("{} AND {}", acc, equality)
        }
    })
}

fn convert_to_query_builder(val: &serde_json::Value) -> Result<QueryBuilder> {
    macro_rules! get_key {
        ($key:expr, $as_type:ident) => {{
            val[$key].$as_type().ok_or_else(|| {
                anyhow!(
                    "internal error: `{}` field is either missing or has invalid type.",
                    $key
                )
            })
        }};
    }
    let op_type = get_key!("type", as_str)?;
    if op_type == "BaseEntity" {
        let type_name = get_key!("name", as_str)?;
        return QueryBuilder::new_from_type_name(type_name);
    }

    let mut builder = convert_to_query_builder(&val["inner"])?;
    match op_type {
        "RestrictionFilter" => {
            builder.load_restrictions(get_key!("restrictions", as_object)?)?;
        }
        "ColumnsSelect" => {
            builder.update_allowed_fields(get_key!("columns", as_array)?)?;
        }
        "Take" => {
            let count = get_key!("count", as_u64)?;
            builder.update_limit(count);
        }
        _ => anyhow::bail!("unexpected relation type `{}`", op_type),
    }
    Ok(builder)
}

/// Convert JSON restrictions into vector of `Restriction` objects.
pub(crate) fn convert_restrictions(restrictions: &JsonObject) -> Result<Vec<Restriction>> {
    let mut sql_restrictions = vec![];
    for (k, v) in restrictions.iter() {
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
        sql_restrictions.push(Restriction {
            table: None,
            column: k.clone(),
            v,
        });
    }
    Ok(sql_restrictions)
}

pub(crate) fn type_to_query(ty: &Arc<ObjectType>) -> Query {
    let builder = QueryBuilder::from_type(ty);
    builder.build()
}

pub(crate) fn json_to_query(val: &serde_json::Value) -> Result<Query> {
    let builder = convert_to_query_builder(val)?;
    Ok(builder.build())
}

/// `Mutation` represents a statement that mutates the database state.
#[derive(Debug, Clone)]
pub(crate) struct Mutation {
    /// SQL statement text
    pub(crate) raw_sql: String,
}

impl Mutation {
    /// Parses a delete statement from JSON.
    pub(crate) fn parse_delete(val: &serde_json::Value) -> Result<Self> {
        let type_name = val["type_name"]
            .as_str()
            .ok_or_else(|| anyhow!("The `type_name` field is not a JSON string."))?;
        let restrictions = val["restrictions"]
            .as_object()
            .ok_or_else(|| anyhow!("The `restrictions` passed is not a JSON object."))?;
        let (ty, restrictions) = {
            let runtime = runtime::get();
            let api_version = current_api_version();
            let ty = match runtime
                .type_system
                .lookup_custom_type(type_name, &api_version)
            {
                Err(_) => anyhow::bail!("Cannot delete from type `{}`", type_name),
                Ok(ty) => ty,
            };
            let restrictions = convert_restrictions(restrictions)?;
            (ty, restrictions)
        };
        let sql = format!(
            "DELETE FROM \"{}\" {}",
            &ty.backing_table(),
            make_restriction_string(&restrictions)
        );
        Ok(Self { raw_sql: sql })
    }
}
