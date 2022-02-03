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
        /// Index of a column containing this field in the resulting row we get from
        /// the database.
        column_idx: usize,
    },
    Entity {
        /// Name of the original Type field
        name: String,
        /// Nested fields of the Entity object.
        children: Vec<SelectField>,
    },
}

/// `QueryExpression` is a structure representing a query ready to be fired.
#[derive(Debug, Clone)]
pub(crate) struct QueryExpression {
    /// SQL query text
    pub(crate) raw_sql: String,
    /// Nested structure representing a blueprint which is used to reconstruct
    /// multi-dimensional (=potentialy nested) JSON response from a linear
    /// sql response row. Each element represents a selected field (potentially nested)
    /// of the selected base type.
    pub(crate) fields: Vec<SelectField>,
    /// JSON fields to be sent to user. It's used to filter columns that come out of the database.
    /// This filtering could be done when generating the expression, but it's complicated
    /// and we will do it later.
    pub(crate) allowed_columns: Option<HashSet<String>>,
    /// Field policies to be applied on the resulting response.
    pub(crate) policies: FieldPolicies,
}

#[derive(Debug, Clone)]
struct Join {
    rtype: Arc<ObjectType>,
    lkey: String,
    rkey: String,
    lalias: String,
    ralias: String,
}

/// Class used to build `QueryExpression` from either JSON query or `ObjectType`.
/// The json part recursively descends through selected fields and captures all
/// joins necessary for nested types retrieval.
/// Once constructed, it can be further restricted by calling load_restrictions method.
/// When we are done with that, `build_query_expression` is called which creates a `QueryExpression`
/// structure that contains raw SQL query string and additional data necessary for
/// JSON response reconstruction and filtering.
struct QueryExpressionBuilder {
    fields: Vec<SelectField>,
    columns: Vec<(String, Field)>,
    base_type: Arc<ObjectType>,
    joins: Vec<Join>,
    restrictions: Vec<Restriction>,
    allowed_columns: Option<HashSet<String>>,
    policies: FieldPolicies,
    limit: Option<u64>,
}

impl QueryExpressionBuilder {
    /// Constructs a query builder ready to build an expression querying all fields of a
    /// given type `ty`. This is done in a shallow manner. Columns representing foreign
    /// key are returned as string, not as the related Entity.
    fn new_from_type(ty: &Arc<ObjectType>) -> Result<Self> {
        let mut builder = Self {
            fields: vec![],
            columns: vec![],
            base_type: ty.clone(),
            joins: vec![],
            restrictions: vec![],
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

    /// Constructs a builder from the `BackingStore` JSON object.
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
        let policies = make_field_policies(&runtime, &ty);

        let mut builder = Self {
            fields: vec![],
            columns: vec![],
            base_type: ty.clone(),
            joins: vec![],
            restrictions: vec![],
            allowed_columns: None,
            policies,
            limit: val["limit"].as_u64(),
        };
        builder.fields = builder.load_fields(&ty, ty.backing_table(), &val["columns"])?;
        Ok(builder)
    }

    fn make_builtin_field(&mut self, field: &Field, column_name: &str) -> SelectField {
        let select_field = SelectField::Scalar {
            name: field.name.clone(),
            type_: field.type_.clone(),
            column_idx: self.columns.len(),
        };
        self.columns.push((column_name.to_owned(), field.clone()));
        select_field
    }

    /// Recursively loads Fields to be retrieved from the database, as specified
    /// by the JSON object's array `columns`. For fields that represent a nested
    /// Entity a join is generated and we attempt to retrieve it as well.
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
                        self.joins.push(Join {
                            rtype: nested_ty.clone(),
                            lkey: field_name.to_owned(),
                            rkey: "id".to_owned(),
                            lalias: current_table.to_owned(),
                            ralias: nested_table.to_owned(),
                        });

                        let nested_fields =
                            self.load_fields(nested_ty, &nested_table, &nested_fields["columns"])?;
                        fields.push(SelectField::Entity {
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

    fn build_query_expression(&self) -> QueryExpression {
        QueryExpression {
            raw_sql: self.make_raw_query(),
            fields: self.fields.clone(),
            allowed_columns: self.allowed_columns.clone(),
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

fn convert_to_expression_builder(val: &serde_json::Value) -> Result<QueryExpressionBuilder> {
    let kind = val["kind"].as_str().ok_or_else(|| {
        anyhow!(
            "internal error: `kind` field is either missing or not a string: {}",
            val
        )
    })?;

    match kind {
        "BackingStore" => QueryExpressionBuilder::new_from_json(val),
        "Join" => anyhow::bail!("join is not supported"),
        "Filter" => {
            let mut builder = convert_to_expression_builder(&val["inner"])?;
            builder.load_restrictions(val)?;
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

pub(crate) fn type_to_expression(ty: &Arc<ObjectType>) -> Result<QueryExpression> {
    let builder = QueryExpressionBuilder::new_from_type(ty)?;
    Ok(builder.build_query_expression())
}

pub(crate) fn json_to_expression(val: &serde_json::Value) -> Result<QueryExpression> {
    let builder = convert_to_expression_builder(val)?;
    Ok(builder.build_query_expression())
}

/// `DeleteExpr` is a structure representing a delete expression ready to be fired.
#[derive(Debug, Clone)]
pub(crate) struct DeleteExpr {
    /// SQL query text
    pub(crate) raw_sql: String,
}

impl DeleteExpr {
    pub(crate) fn new_from_json(val: &serde_json::Value) -> Result<Self> {
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
