// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, Literal, PropertyAccess};
use crate::deno::current_api_version;
use crate::deno::make_field_policies;

use crate::runtime;
use crate::types::{Field, ObjectType, Type, TypeSystemError, OAUTHUSER_TYPE_NAME};
use crate::JsonObject;

use anyhow::{anyhow, Context, Result};
use enum_as_inner::EnumAsInner;
use serde_derive::{Deserialize, Serialize};
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

#[derive(Debug, Clone)]
struct SortBy {
    field_name: String,
    ascending: bool,
}

struct Column {
    /// Column name.
    name: String,
    /// Name of the table storing this column.
    table_name: String,
    /// Entity field corresponding to this column.
    field: Field,
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
    columns: Vec<Column>,
    /// Entity object representing entity that is being retrieved along with necessary joins
    /// and nested entities
    entity: QueriedEntity,
    /// Expression used to filter the entities that are to be returned.
    filter_expr: Option<Expr>,
    /// List of fields to be returned to the user.
    allowed_fields: Option<HashSet<String>>,
    /// If Some, it will be used to sort the queried results.
    sort: Option<SortBy>,
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
            filter_expr: None,
            allowed_fields: None,
            sort: None,
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
            let field = builder.make_scalar_field(&field, ty.backing_table(), None);
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
        builder.entity = builder.load_entity(&runtime, &ty);
        Ok(builder)
    }

    fn make_scalar_field(
        &mut self,
        field: &Field,
        table_name: &str,
        transform: Option<fn(Value) -> Value>,
    ) -> QueryField {
        let select_field = QueryField::Scalar {
            name: field.name.clone(),
            type_: field.type_.clone(),
            is_optional: field.is_optional,
            column_idx: self.columns.len(),
            transform,
        };
        self.columns.push(Column {
            name: field.name.to_owned(),
            table_name: table_name.to_owned(),
            field: field.clone(),
        });
        select_field
    }

    /// Prepares the retrieval of Entity of type `ty` from the database and
    /// ensures login restrictions are respected.
    fn load_entity(&mut self, runtime: &runtime::Runtime, ty: &Arc<ObjectType>) -> QueriedEntity {
        self.add_login_filters_recursive(runtime, ty, Expr::Parameter { position: 0 });
        self.load_entity_recursive(runtime, ty, ty.backing_table())
    }

    /// Loads QueriedEntity for a given type `ty` to be retrieved from the
    /// database. For fields that represent a nested Entity a join is
    /// generated and we attempt to retrieve them recursively as well.
    fn load_entity_recursive(
        &mut self,
        runtime: &runtime::Runtime,
        ty: &Arc<ObjectType>,
        current_table: &str,
    ) -> QueriedEntity {
        let policies = make_field_policies(runtime, ty);

        let mut fields = vec![];
        let mut joins = HashMap::default();
        for field in ty.all_fields() {
            let field_policy = policies.transforms.get(&field.name).cloned();

            let query_field = if let Type::Object(nested_ty) = &field.type_ {
                let nested_table = format!(
                    "JOIN{}_{}_TO_{}",
                    self.join_counter,
                    current_table,
                    nested_ty.backing_table()
                );
                // PostgreSQL has a limit on identifiers to be at most 63 bytes long.
                let nested_table = max_prefix(nested_table.as_str(), 63).to_owned();
                self.join_counter += 1;

                joins.insert(
                    field.name.to_owned(),
                    Join {
                        entity: self.load_entity_recursive(runtime, nested_ty, &nested_table),
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
                self.make_scalar_field(field, current_table, field_policy)
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

    /// Adds filters that ensure login constrains are satisfied for a type
    /// `ty` that is to be retrieved from the database.
    fn add_login_filters_recursive(
        &mut self,
        runtime: &runtime::Runtime,
        ty: &Arc<ObjectType>,
        property_chain: Expr,
    ) {
        let policies = make_field_policies(runtime, ty);
        let user_id: Literal = match &policies.current_userid {
            None => "NULL",
            Some(id) => id.as_str(),
        }
        .into();

        for field in ty.all_fields() {
            if let Type::Object(nested_ty) = &field.type_ {
                let property_access = PropertyAccess {
                    property: field.name.to_owned(),
                    object: property_chain.clone().into(),
                };
                if nested_ty.name() == OAUTHUSER_TYPE_NAME {
                    if policies.match_login.contains(&field.name) {
                        self.add_expression_filter(
                            BinaryExpr {
                                left: Box::new(property_access.into()),
                                op: BinaryOp::Eq,
                                right: Box::new(user_id.clone().into()),
                            }
                            .into(),
                        )
                    }
                } else {
                    self.add_login_filters_recursive(runtime, nested_ty, property_access.into());
                }
            }
        }
    }

    fn update_limit(&mut self, limit: u64) {
        self.limit = Some(std::cmp::min(limit, self.limit.unwrap_or(limit)));
    }

    fn update_allowed_fields(&mut self, columns: Vec<String>) -> Result<()> {
        self.allowed_fields = Some(HashSet::from_iter(columns));
        Ok(())
    }

    fn add_expression_filter(&mut self, expr: Expr) {
        if let Some(filter_expr) = &self.filter_expr {
            let new_expr = BinaryExpr {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(filter_expr.clone()),
            };
            self.filter_expr = Some(new_expr.into());
        } else {
            self.filter_expr = Some(expr);
        }
    }

    fn set_sort(&mut self, field_name: &str, ascending: bool) {
        self.sort = Some(SortBy {
            field_name: field_name.to_owned(),
            ascending,
        });
    }

    fn make_column_string(&self) -> String {
        let mut column_string = String::new();
        for c in &self.columns {
            let col = match c.field.default_value() {
                Some(dfl) => format!(
                    "coalesce(\"{}\".\"{}\",'{}') AS \"{}_{}\",",
                    c.table_name, c.name, dfl, c.table_name, c.name
                ),
                None => format!("\"{}\".\"{}\",", c.table_name, c.name),
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
        let where_cond = if let Some(expr) = &self.filter_expr {
            let condition = self.filter_expr_to_string(expr)?;
            format!("WHERE {}", condition)
        } else {
            "".to_owned()
        };
        Ok(where_cond)
    }

    fn filter_expr_to_string(&self, expr: &Expr) -> Result<String> {
        let expr_str = match &expr {
            Expr::Literal { value } => {
                let lit_str = match &value {
                    Literal::Bool(lit) => (if *lit { "1" } else { "0" }).to_string(),
                    Literal::U64(lit) => lit.to_string(),
                    Literal::I64(lit) => lit.to_string(),
                    Literal::F64(lit) => lit.to_string(),
                    Literal::String(lit) => lit.to_string(),
                    Literal::Null => "NULL".to_owned(),
                };
                escape_string(lit_str.as_str())
            }
            Expr::Binary(binary_exp) => {
                format!(
                    "({} {} {})",
                    self.filter_expr_to_string(&binary_exp.left)?,
                    binary_exp.op.to_sql_string(),
                    self.filter_expr_to_string(&binary_exp.right)?,
                )
            }
            Expr::Property(property) => self.property_expr_to_string(property)?,
            Expr::Parameter { .. } => anyhow::bail!("unexpected standalone parameter usage"),
        };
        Ok(expr_str)
    }

    fn property_expr_to_string(&self, prop_access: &PropertyAccess) -> Result<String> {
        fn get_property_chain(prop_access: &PropertyAccess) -> Result<Vec<String>> {
            match &*prop_access.object {
                Expr::Property(obj) => {
                    let mut properties = get_property_chain(obj)?;
                    properties.push(prop_access.property.to_owned());
                    Ok(properties)
                }
                Expr::Parameter { .. } => Ok(vec![prop_access.property.to_owned()]),
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

    fn make_sort_string(&self) -> String {
        if let Some(sort) = &self.sort {
            let order = if sort.ascending { "ASC" } else { "DESC" };
            format!("ORDER BY \"{}\" {}", sort.field_name, order)
        } else {
            "".into()
        }
    }

    fn make_raw_query(&self) -> Result<String> {
        let column_string = self.make_column_string();
        let join_string = self.make_join_string();
        let filter_string = self.make_filter_string()?;
        let sort_string = self.make_sort_string();

        let mut raw_sql = format!(
            "SELECT {} FROM \"{}\" {} {} {}",
            column_string,
            self.base_type().backing_table(),
            join_string,
            filter_string,
            sort_string
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

/// Returns the longest possible prefix of `s` that is at most `max_len`
/// bytes long and ends at a character boundary so that we don't break
/// multi-byte characters.
fn max_prefix(s: &str, max_len: usize) -> &str {
    if max_len >= s.len() {
        return s;
    }
    let mut idx = max_len;
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum QueryOperator {
    BaseEntity {
        name: String,
    },
    #[serde(rename = "ExpressionFilter")]
    Filter {
        expression: Expr,
        inner: Box<QueryOperator>,
    },
    #[serde(rename = "ColumnsSelect")]
    Projection {
        #[serde(rename = "columns")]
        fields: Vec<String>,
        inner: Box<QueryOperator>,
    },
    Take {
        count: u64,
        inner: Box<QueryOperator>,
    },
    SortBy {
        key: String,
        ascending: bool,
        inner: Box<QueryOperator>,
    },
}

fn convert_to_query_builder(op: QueryOperator) -> Result<QueryBuilder> {
    use QueryOperator::*;
    let builder = match op {
        BaseEntity { name } => QueryBuilder::new_from_type_name(&name)?,
        Filter { expression, inner } => {
            let mut builder = convert_to_query_builder(*inner)?;
            builder.add_expression_filter(expression);
            builder
        }
        Projection { fields, inner } => {
            let mut builder = convert_to_query_builder(*inner)?;
            builder.update_allowed_fields(fields)?;
            builder
        }
        Take { count, inner } => {
            let mut builder = convert_to_query_builder(*inner)?;
            builder.update_limit(count);
            builder
        }
        SortBy {
            key,
            ascending,
            inner,
        } => {
            let mut builder = convert_to_query_builder(*inner)?;
            builder.set_sort(&key, ascending);
            builder
        }
    };
    Ok(builder)
}

pub(crate) fn type_to_query(ty: &Arc<ObjectType>) -> Result<Query> {
    let builder = QueryBuilder::from_type(ty);
    builder.build()
}

pub(crate) fn json_to_query(val: serde_json::Value) -> Result<Query> {
    let op_chain: QueryOperator =
        serde_json::from_value(val).context("failed to deserialize QueryOperator from JSON")?;
    let builder = convert_to_query_builder(op_chain)?;
    builder.build()
}

#[derive(Debug, Clone)]
struct Restriction {
    /// Table in which `column` can be found. If unspecified, base select table will be used.
    table: Option<String>,
    /// Database column name used for equality restriction.
    column: String,
    /// The value used to restrict the results.
    v: SqlValue,
}

/// Convert JSON restrictions into vector of `Restriction` objects.
fn convert_restrictions(restrictions: &JsonObject) -> Result<Vec<Restriction>> {
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

/// Convert a vector of `Restriction` objects into a SQL `WHERE` clause.
fn make_restriction_string(restrictions: &[Restriction]) -> String {
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
