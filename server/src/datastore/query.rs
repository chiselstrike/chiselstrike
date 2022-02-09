// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::deno::current_api_version;
use crate::deno::make_field_policies;
use crate::policies::FieldPolicies;

use crate::runtime;
use crate::types::{Field, ObjectType, Type, TypeSystemError, OAUTHUSER_TYPE_NAME};

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
    k: String,
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
    },
    Entity {
        /// Name of the original Type field
        name: String,
        is_optional: bool,
        /// Nested fields of the Entity object.
        children: Vec<SelectField>,
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
    /// sql response row. Each element (Vec<SelectField>) represents a recipe
    /// used to reconstruct one Entity object. Each Entity object corresponds to either
    /// base Entity (type) or one of the joined entities (once we support joins).
    /// Each element of Vec<SelectField> represents a selected field (potentially nested)
    /// of given Entity (type).
    pub(crate) fields: Vec<Vec<SelectField>>,
    /// Entity fields selected by the user. This field is used to post-filter fields that
    /// shall be returned to the user in JSON.
    /// FIXME: The post-filtering is suboptimal solution and selection should happen when
    /// we build the raw_sql query.
    pub(crate) allowed_fields: Option<HashSet<String>>,
    /// Field policies to be applied on the resulting response.
    pub(crate) policies: FieldPolicies,
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

/// Query builder is used to construct a `Query` object.
///
/// The query builder works either from a JSON representation or an `ObjectType`.
/// The JSON builder recursively descends through selected fields and captures all
/// joins necessary for nested types retrieval.
/// When we are done with that, `build` is called which creates a `Query`
/// structure that contains raw SQL query string and additional data necessary for
/// JSON response reconstruction and filtering.
struct QueryBuilder {
    /// Recursive vector used to reconstruct nested entities based on flat vector of columns
    /// returned by the database. Each element (Vec<SelectField>) represents a recipe
    /// used to reconstruct one Entity object. Each Entity object corresponds to either
    /// base Entity (type) or one of the joined entities (once we support joins).
    fields: Vec<Vec<SelectField>>,
    /// Vector of SQL column aliases that will be selected from the database and corresponding
    /// scalar fields.
    columns: Vec<(String, Field)>,
    base_type: Arc<ObjectType>,
    joins: Vec<Join>,
    restrictions: Vec<Restriction>,
    /// List of fields to be returned to the user.
    allowed_fields: Option<HashSet<String>>,
    policies: FieldPolicies,
    /// Limits how many rows/entries will be returned to the user.
    limit: Option<u64>,
}

impl QueryBuilder {
    fn new(base_type: Arc<ObjectType>, policies: FieldPolicies, limit: Option<u64>) -> Self {
        Self {
            fields: vec![],
            columns: vec![],
            base_type,
            joins: vec![],
            restrictions: vec![],
            allowed_fields: None,
            policies,
            limit,
        }
    }
    /// Constructs a query builder ready to build an expression querying all fields of a
    /// given type `ty`. This is done in a shallow manner. Columns representing foreign
    /// key are returned as string, not as the related Entity.
    fn from_type(ty: &Arc<ObjectType>) -> Self {
        let mut builder = Self::new(ty.clone(), FieldPolicies::default(), None);
        let mut fields = vec![];
        for field in ty.all_fields() {
            let mut field = field.clone();
            field.type_ = match field.type_ {
                Type::Object(_) => Type::String, // This is actually a foreign key.
                ty => ty,
            };
            let field = builder.make_scalar_field(&field, field.name.as_str());
            fields.push(field)
        }
        builder.fields = vec![fields];
        builder
    }

    /// Constructs a builder from the query expression JSON object.
    fn parse_from_json_v2(val: &serde_json::Value) -> Result<Self> {
        let entity_name = val["base_entity"].as_str().ok_or_else(|| {
            anyhow!(
                "internal error: `base_entity` field is either missing or not a string: {}",
                val
            )
        })?;
        let runtime = runtime::get();
        let ts = &runtime.type_system;
        let api_version = current_api_version();
        let ty = match ts.lookup_builtin_type(entity_name) {
            Ok(Type::Object(ty)) => ty,
            Err(TypeSystemError::NotABuiltinType(_)) => {
                ts.lookup_custom_type(entity_name, &api_version)?
            }
            _ => anyhow::bail!("Unexpected type name as query base type: {}", entity_name),
        };
        let policies = make_field_policies(&runtime, &ty);

        let mut builder = Self::new(ty.clone(), policies, None);
        builder.fields = vec![builder.parse_fields_v2(&ty, ty.backing_table())];

        let ops = val["operations"].as_array().ok_or_else(|| {
            anyhow!(
                "internal error: `operations` field is either missing or not an array: {}",
                val
            )
        })?;
        builder.parse_operations_v2(ops)?;
        Ok(builder)
    }

    fn parse_fields_v2(&mut self, ty: &Arc<ObjectType>, current_table: &str) -> Vec<SelectField> {
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

                fields.push(SelectField::Entity {
                    name: field.name.clone(),
                    is_optional: field.is_optional,
                    children: self.parse_fields_v2(ty, &nested_table),
                });
            } else {
                let column_name = format!("{}.{}", current_table, field.name);
                let field = self.make_scalar_field(field, &column_name);
                fields.push(field)
            }
        }
        fields
    }

    fn parse_operations_v2(&mut self, ops: &[serde_json::Value]) -> Result<()> {
        for op in ops {
            macro_rules! get_key {
                ($key:expr) => {{
                    get_key!($key, as_str)
                }};
                ($key:expr, $as_type:ident) => {{
                    op[$key].$as_type().ok_or_else(|| {
                        anyhow!(
                            "internal error: `{}` field is either missing or has invalid type.",
                            $key
                        )
                    })
                }};
            }

            let op_type = get_key!("type")?;
            if op_type == "Take" {
                let limit = get_key!("count", as_u64)?;
                self.limit = Some(std::cmp::min(limit, self.limit.unwrap_or(limit)));
            } else {
                anyhow::bail!("unexpected operation type `{}`", op_type);
            }
        }
        Ok(())
    }

    /// Constructs a builder from the `BackingStore` JSON object.
    fn parse_from_json_v1(val: &serde_json::Value) -> Result<Self> {
        anyhow::ensure!(
            val["kind"] == "BackingStore",
            "unexpected object kind. Expected `BackingStore`, got {:?}",
            val["kind"]
        );
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
        let policies = make_field_policies(&runtime, &ty);

        let mut builder = Self::new(ty.clone(), policies, val["limit"].as_u64());
        builder.fields = vec![builder.parse_fields_v1(&ty, ty.backing_table(), &val["columns"])?];
        Ok(builder)
    }

    fn make_scalar_field(&mut self, field: &Field, column_name: &str) -> SelectField {
        let select_field = SelectField::Scalar {
            name: field.name.clone(),
            type_: field.type_.clone(),
            is_optional: field.is_optional,
            column_idx: self.columns.len(),
        };
        self.columns.push((column_name.to_owned(), field.clone()));
        select_field
    }

    /// Recursively loads Fields to be retrieved from the database, as specified
    /// by the JSON object's array `columns`. For fields that represent a nested
    /// Entity a join is generated and we attempt to retrieve it as well.
    fn parse_fields_v1(
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
                    let field = self.make_scalar_field(field, column_name.as_str());
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
                        self.joins.push(Join {
                            rtype: nested_ty.clone(),
                            lkey: field_name.to_owned(),
                            rkey: "id".to_owned(),
                            lalias: current_table.to_owned(),
                            ralias: nested_table.to_owned(),
                        });

                        let nested_fields = self.parse_fields_v1(
                            nested_ty,
                            &nested_table,
                            &nested_fields["columns"],
                        )?;
                        fields.push(SelectField::Entity {
                            name: field.name.clone(),
                            is_optional: field.is_optional,
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

    fn update_allowed_fields(&mut self, columns_json: &serde_json::Value) -> Result<()> {
        if let Some(columns) = columns_json.as_array() {
            let mut allowed_fields = HashSet::<String>::default();
            for c in columns {
                let c = &c.as_array().ok_or_else(|| {
                    // FIXME: This is ugly and the internal part is not necessary.
                    anyhow!(
                        "internal error: column object must be an array, got `{}`",
                        c
                    )
                })?[0];
                if let Value::String(field_name) = c {
                    allowed_fields.insert(field_name.to_owned());
                }
            }
            self.allowed_fields = Some(allowed_fields);
        }
        Ok(())
    }

    fn parse_restrictions_v1(&mut self, rest_json: &serde_json::Value) -> Result<()> {
        if let Some(limit) = rest_json["limit"].as_u64() {
            self.limit = Some(limit);
        }
        self.update_allowed_fields(&rest_json["columns"])?;
        let restrictions = rest_json["restrictions"]
            .as_object()
            .ok_or_else(|| anyhow!("Missing restrictions in filter"))?;
        let restrictions = convert_restrictions(restrictions)?;
        for r in restrictions {
            anyhow::ensure!(
                self.base_type.field_by_name(&r.k).is_some(),
                "trying to filter by non-existent field `{}`",
                r.k
            );
            self.restrictions.push(r);
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
                "LEFT JOIN ({}) AS {} ON {}.{}={}.{}\n",
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

    // FIXME: This needs to be done for nested objects as well.
    fn make_login_restrictions(&self) -> Vec<Restriction> {
        let current_userid = match &self.policies.current_userid {
            None => "NULL".to_owned(),
            Some(id) => id.to_owned(),
        };
        let mut restrictions = vec![];
        for field_name in &self.policies.match_login {
            match self.base_type.field_by_name(field_name) {
                Some(Field {
                    type_: Type::Object(obj),
                    ..
                }) if obj.name() == OAUTHUSER_TYPE_NAME => {
                    restrictions.push(Restriction {
                        k: field_name.to_owned(),
                        v: SqlValue::String(current_userid.clone()),
                    });
                }
                _ => {}
            }
        }
        restrictions
    }

    fn make_raw_query(&self) -> String {
        let column_string = self.make_column_string();
        let join_string = self.make_join_string();
        let raw_restrictions = {
            let mut restrictions = self.make_login_restrictions();
            restrictions.extend(self.restrictions.clone());
            make_restriction_string(&restrictions)
        };

        let mut raw_sql = format!(
            "SELECT {} FROM {} {} {}",
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
            policies: self.policies.clone(),
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
            SqlValue::Bool(v) => format!("{}", v),
            SqlValue::U64(v) => format!("{}", v),
            SqlValue::I64(v) => format!("{}", v),
            SqlValue::F64(v) => format!("{}", v),
            SqlValue::String(v) => escape_string(v),
        };
        if acc.is_empty() {
            format!("WHERE {}={}", rest.k, str_v)
        } else {
            format!("{} AND {}={}", acc, rest.k, str_v)
        }
    })
}

fn convert_to_query_builder(val: &serde_json::Value) -> Result<QueryBuilder> {
    let kind = val["kind"].as_str().ok_or_else(|| {
        anyhow!(
            "internal error: `kind` field is either missing or not a string: {}",
            val
        )
    })?;

    match kind {
        "BackingStore" => QueryBuilder::parse_from_json_v1(val),
        "Join" => anyhow::bail!("join is not supported"),
        "Filter" => {
            let mut builder = convert_to_query_builder(&val["inner"])?;
            builder.parse_restrictions_v1(val)?;
            Ok(builder)
        }
        _ => anyhow::bail!("unexpected relation kind `{}`", kind),
    }
}

/// Convert JSON restrictions into vector of `Restriction` objects.
pub(crate) fn convert_restrictions(
    restrictions: &serde_json::Map<std::string::String, serde_json::Value>,
) -> Result<Vec<Restriction>> {
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
        sql_restrictions.push(Restriction { k: k.clone(), v });
    }
    Ok(sql_restrictions)
}

pub(crate) fn type_to_query(ty: &Arc<ObjectType>) -> Query {
    let builder = QueryBuilder::from_type(ty);
    builder.build()
}

pub(crate) fn json_to_query_v1(val: &serde_json::Value) -> Result<Query> {
    let builder = convert_to_query_builder(val)?;
    Ok(builder.build())
}

pub(crate) fn json_to_query_v2(val: &serde_json::Value) -> Result<Query> {
    let builder = QueryBuilder::parse_from_json_v2(val)?;
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
            "DELETE FROM {} {}",
            &ty.backing_table(),
            make_restriction_string(&restrictions)
        );
        Ok(Self { raw_sql: sql })
    }
}
