// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::datastore::expr;
use crate::deno::current_api_version;
use crate::deno::make_field_policies;
use crate::policies::FieldPolicies;

use crate::runtime;
use crate::types::{Field, ObjectType, Type, TypeSystemError, OAUTHUSER_TYPE_NAME};
use crate::JsonObject;

use anyhow::{anyhow, Result};
use enum_as_inner::EnumAsInner;
use serde_json::value::Value;
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
pub(crate) enum QueryField {
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
    /// Entity that is being queried. Contains information necessary to reconstruct
    /// the JSON response.
    pub(crate) entity: QueriedEntity,
    /// Entity fields selected by the user. This field is used to post-filter fields that
    /// shall be returned to the user in JSON.
    /// FIXME: The post-filtering is suboptimal solution and selection should happen when
    /// we build the raw_sql query.
    pub(crate) allowed_fields: Option<HashSet<String>>,
}

/// QueriedEntity represents queried Entity of type `ty` which is to be aliased as
/// `table_alias` in the SQL query and joined with nested Entities using `joins`.
#[derive(Debug, Clone)]
pub(crate) struct QueriedEntity {
    /// Entity fields to be returned in JSON response
    pub(crate) fields: Vec<QueryField>,
    /// Type of the entity.
    ty: Arc<ObjectType>,
    /// Alias name of this entity to be used in SQL query.
    table_alias: String,
    /// Map from Entity field name to joined Entities which correspond to the entities
    /// stored under the field name.
    joins: HashMap<String, Join>,
}

impl QueriedEntity {
    pub(crate) fn get_child_entity<'a>(&'a self, child_name: &str) -> Option<&'a QueriedEntity> {
        self.joins.get(child_name).map(|c| &c.entity)
    }
}

/// Represents JOIN operator joining `entity` to a previous QueriedEntity which holds the
/// join. The join is done using equality on `lkey` of the previous QueriedEntity and `rkey`
/// on the current `entity`.
#[derive(Debug, Clone)]
struct Join {
    entity: QueriedEntity,
    lkey: String,
    rkey: String,
}

/// Class used to build `Query` from either JSON query or `ObjectType`.
/// The json part recursively descends through selected fields and captures all
/// joins necessary for nested types retrieval.
/// When we are done with that, `build` is called which creates a `Query`
/// structure that contains raw SQL query string and additional data necessary for
/// JSON response reconstruction and filtering.
struct QueryBuilder {
    /// Vector of SQL column aliases that will be selected from the database and corresponding
    /// scalar fields.
    columns: Vec<(String, String, Field)>,
    /// Entity object representing entity that is being retrieved along with necessary joins
    /// and nested entities
    entity: QueriedEntity,
    restrictions: Vec<Restriction>,
    /// Expression used to filter the entities that are to be returned.
    filter_expr: Option<expr::Expr>,
    /// List of fields to be returned to the user.
    allowed_fields: Option<HashSet<String>>,
    /// Limits how many rows/entries will be returned to the user.
    limit: Option<u64>,
    /// Counts the total number of joins the builder encountered. It's used to
    /// uniquely identify joined tables.
    join_counter: usize,
}

impl QueryBuilder {
    fn new(base_type: Arc<ObjectType>) -> Self {
        Self {
            columns: vec![],
            entity: QueriedEntity {
                ty: base_type.clone(),
                fields: vec![],
                table_alias: base_type.backing_table().to_owned(),
                joins: HashMap::default(),
            },
            restrictions: vec![],
            filter_expr: None,
            allowed_fields: None,
            limit: None,
            join_counter: 0,
        }
    }

    fn base_type(&self) -> &Arc<ObjectType> {
        &self.entity.ty
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
            builder.entity.fields.push(field)
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
        builder.entity = builder.load_entity(&runtime, &ty, ty.backing_table());
        Ok(builder)
    }

    fn make_scalar_field(
        &mut self,
        field: &Field,
        table_name: &str,
        column_name: &str,
        transform: Option<fn(Value) -> Value>,
    ) -> QueryField {
        let select_field = QueryField::Scalar {
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

    /// QueriedEntity for a given type `ty` to be retrieved from the
    /// database. For fields that represent a nested Entity a join is
    /// generated and we attempt to retrieve them recursively as well.
    fn load_entity(
        &mut self,
        runtime: &runtime::Runtime,
        ty: &Arc<ObjectType>,
        current_table: &str,
    ) -> QueriedEntity {
        let policies = make_field_policies(runtime, ty);
        self.add_login_restrictions(ty, current_table, &policies);

        let mut fields = vec![];
        let mut joins = HashMap::default();
        for field in ty.all_fields() {
            let field_policy = policies.transforms.get(&field.name).cloned();

            let query_field = if let Type::Object(nested_ty) = &field.type_ {
                let nested_table = format!(
                    "{}_JOIN{}_{}",
                    current_table,
                    self.join_counter,
                    nested_ty.backing_table()
                );
                self.join_counter += 1;

                joins.insert(
                    field.name.to_owned(),
                    Join {
                        entity: self.load_entity(runtime, nested_ty, &nested_table),
                        lkey: field.name.to_owned(),
                        rkey: "id".to_owned(),
                    },
                );
                QueryField::Entity {
                    name: field.name.clone(),
                    is_optional: field.is_optional,
                    transform: field_policy,
                }
            } else {
                self.make_scalar_field(field, current_table, &field.name, field_policy)
            };
            fields.push(query_field);
        }
        QueriedEntity {
            ty: ty.clone(),
            fields,
            table_alias: current_table.to_owned(),
            joins,
        }
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

    fn add_expression_filter(&mut self, expr: expr::Expr) {
        if let Some(filter_expr) = &self.filter_expr {
            let new_expr = expr::BinaryExpr {
                left: Box::new(expr),
                op: expr::BinaryOp::And,
                right: Box::new(filter_expr.clone()),
            };
            self.filter_expr = Some(new_expr.into());
        } else {
            self.filter_expr = Some(expr);
        }
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
        fn gather_joins(entity: &QueriedEntity) -> String {
            let mut join_string = String::new();
            for join in entity.joins.values() {
                join_string += &format!(
                    "LEFT JOIN \"{}\" AS \"{}\" ON \"{}\".\"{}\"=\"{}\".\"{}\"\n",
                    join.entity.ty.backing_table(),
                    join.entity.table_alias,
                    entity.table_alias,
                    join.lkey,
                    join.entity.table_alias,
                    join.rkey
                );
                join_string += gather_joins(&join.entity).as_str();
            }
            join_string
        }
        gather_joins(&self.entity)
    }

    fn make_filter_string(&self) -> Result<String> {
        let mut rest_filter = make_restriction_string(&self.restrictions);
        if let Some(expr) = &self.filter_expr {
            let condition = self.filter_expr_to_string(expr)?;
            if rest_filter.is_empty() {
                rest_filter = format!("WHERE {}", condition);
            } else {
                rest_filter = format!("{} AND ({})", rest_filter, condition);
            }
        }
        Ok(rest_filter)
    }

    fn filter_expr_to_string(&self, expr: &expr::Expr) -> Result<String> {
        let expr_str = match &expr {
            expr::Expr::Literal { value } => {
                let lit_str = match &value {
                    expr::Literal::Bool(lit) => (if *lit { "1" } else { "0" }).to_string(),
                    expr::Literal::U64(lit) => lit.to_string(),
                    expr::Literal::I64(lit) => lit.to_string(),
                    expr::Literal::F64(lit) => lit.to_string(),
                    expr::Literal::String(lit) => lit.to_string(),
                    expr::Literal::Null => "NULL".to_owned(),
                };
                escape_string(lit_str.as_str())
            }
            expr::Expr::Binary(binary_exp) => {
                format!(
                    "({} {} {})",
                    self.filter_expr_to_string(&binary_exp.left)?,
                    binary_exp.op.to_sql_string(),
                    self.filter_expr_to_string(&binary_exp.right)?,
                )
            }
            expr::Expr::Property(property) => self.property_expr_to_string(property)?,
            expr::Expr::Parameter { .. } => anyhow::bail!("unexpected standalone parameter usage"),
        };
        Ok(expr_str)
    }

    fn property_expr_to_string(&self, prop_access: &expr::PropertyAccess) -> Result<String> {
        fn get_property_chain(prop_access: &expr::PropertyAccess) -> Result<Vec<String>> {
            match &*prop_access.object {
                expr::Expr::Property(obj) => {
                    let mut properties = get_property_chain(obj)?;
                    properties.push(prop_access.property.to_owned());
                    Ok(properties)
                }
                expr::Expr::Parameter { .. } => Ok(vec![prop_access.property.to_owned()]),
                _ => anyhow::bail!("unexpected expression in property chain!"),
            }
        }
        let properties = get_property_chain(prop_access)?;
        assert!(!properties.is_empty());

        let mut field = &properties[0];
        let mut entity = &self.entity;
        for next_field in &properties[1..] {
            entity = &entity
                .joins
                .get(field)
                .ok_or_else(|| {
                    anyhow!(
                        "expression error: unable to locate joined entity on field {}",
                        field
                    )
                })?
                .entity;
            field = next_field;
        }
        Ok(format!("\"{}\".\"{}\"", entity.table_alias, field))
    }

    fn make_raw_query(&self) -> Result<String> {
        let column_string = self.make_column_string();
        let join_string = self.make_join_string();
        let filter_string = self.make_filter_string()?;

        let mut raw_sql = format!(
            "SELECT {} FROM \"{}\" {} {}",
            column_string,
            self.base_type().backing_table(),
            join_string,
            filter_string
        );
        if let Some(limit) = self.limit {
            raw_sql += format!(" LIMIT {}", limit).as_str();
        }
        Ok(raw_sql)
    }

    fn build(&self) -> Result<Query> {
        Ok(Query {
            raw_sql: self.make_raw_query()?,
            entity: self.entity.clone(),
            allowed_fields: self.allowed_fields.clone(),
        })
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
        "ExpressionFilter" => {
            let expr = expr::from_json(val["expression"].clone())?;
            builder.add_expression_filter(expr);
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

pub(crate) fn type_to_query(ty: &Arc<ObjectType>) -> Result<Query> {
    let builder = QueryBuilder::from_type(ty);
    builder.build()
}

pub(crate) fn json_to_query(val: &serde_json::Value) -> Result<Query> {
    let builder = convert_to_query_builder(val)?;
    builder.build()
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
