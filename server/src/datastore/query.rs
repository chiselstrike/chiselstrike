// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, Literal, PropertyAccess};
use crate::deno::make_field_policies;
use crate::policies::Policies;
use crate::runtime;
use crate::types::TypeSystem;
use crate::types::{Field, ObjectType, Type, TypeSystemError, OAUTHUSER_TYPE_NAME};
use crate::JsonObject;

use anyhow::{anyhow, Result};
use enum_as_inner::EnumAsInner;
use serde_derive::{Deserialize, Serialize};
use serde_json::value::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
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

    fn has_field(&self, field_name: &str) -> bool {
        self.ty.all_fields().any(|field| field.name == field_name)
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

/// Sorts elements by `field_name` in `ascending` order if true, descending otherwise.
#[derive(Debug, Clone)]
struct SortBy {
    field_name: String,
    ascending: bool,
}

/// Operators used to mutate the result set.
#[derive(Debug, Clone, EnumAsInner)]
enum QueryOp {
    /// Filters elements by `expression`.
    Filter {
        expression: Expr,
    },
    /// Projects QueryEntity to a projected Entity containing only `fields`.
    Projection {
        fields: Vec<String>,
    },
    /// Limits the number of output rows by taking the first `count` rows.
    Take {
        count: u64,
    },
    /// Skips the first `count` rows.
    Skip {
        count: u64,
    },
    SortBy(SortBy),
}

struct Column {
    /// Column name which is coincidentally also the name of the Entity field
    /// this column corresponds to.
    name: String,
    /// Name of the table storing this column.
    table_name: String,
    /// Entity field corresponding to this column.
    field: Field,
}

impl Column {
    /// Column alias used to uniquely address the column within SQL query.
    fn alias(&self) -> ColumnAlias {
        ColumnAlias {
            field_name: self.name.to_owned(),
            table_name: self.table_name.to_owned(),
        }
    }
}

/// ColumnAlias is used to uniquely identify a `Column` that is to be retrieved
/// from the database. It's string representation is used in the SELECT statement
/// to identify the column which is then utilized by filtering and sorting statements.
struct ColumnAlias {
    /// Name of the entity field that corresponds to this retrieved column.
    field_name: String,
    /// Name of the table where the field resides. This name can be an alias of the
    /// original database table name, but it must be the name that is addressable within
    /// the SQL statement in which the corresponding column is retrieved/used.
    table_name: String,
}

impl fmt::Display for ColumnAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", self.table_name, self.field_name)
    }
}

#[derive(Debug, Clone, EnumAsInner)]
pub(crate) enum TargetDatabase {
    Postgres,
    Sqlite,
}

/// Class used to build `Query` from either QueryOpChain or `ObjectType`.
/// For the op chain, it recursively descends through selected fields and captures all
/// joins necessary for nested types retrieval.
/// When we are done with that, `build_query` can be called which creates a `Query`
/// structure that contains raw SQL query string and additional data necessary for
/// JSON response reconstruction and filtering.
pub(crate) struct QueryPlan {
    /// Columns that will be retrieved from the database in order defined by this vector.
    columns: Vec<Column>,
    /// Entity object representing entity that is being retrieved along with necessary joins
    /// and nested entities
    entity: QueriedEntity,
    /// List of fields to be returned to the user.
    allowed_fields: Option<HashSet<String>>,
    /// Counts the total number of joins the builder encountered. It's used to
    /// uniquely identify joined tables.
    join_counter: usize,
    /// Operators used to mutate the result set.
    operators: Vec<QueryOp>,
}

impl QueryPlan {
    fn new(base_type: Arc<ObjectType>) -> Self {
        Self {
            columns: vec![],
            entity: QueriedEntity {
                ty: base_type.clone(),
                fields: vec![],
                table_alias: base_type.backing_table().to_owned(),
                joins: HashMap::default(),
            },
            allowed_fields: None,
            join_counter: 0,
            operators: vec![],
        }
    }

    fn base_type(&self) -> &Arc<ObjectType> {
        &self.entity.ty
    }

    /// Constructs a query builder ready to build an expression querying all fields of a
    /// given type `ty`. This is done in a shallow manner. Columns representing foreign
    /// key are returned as string, not as the related Entity.
    pub(crate) fn from_type(ty: &Arc<ObjectType>) -> Self {
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

    /// Constructs a query builder from a query `op_chain` and additional
    /// helper data like `api_version`, `userid` and `path` (url path used
    /// for policy evaluation).
    pub(crate) fn from_op_chain(
        ts: &TypeSystem,
        api_version: &str,
        userid: &Option<String>,
        path: &str,
        op_chain: QueryOpChain,
    ) -> Result<Self> {
        let (entity_name, operators) = convert_ops(op_chain)?;

        let runtime = runtime::get();
        let ty = match ts.lookup_builtin_type(&entity_name) {
            Ok(Type::Object(ty)) => ty,
            Err(TypeSystemError::NotABuiltinType(_)) => {
                ts.lookup_custom_type(&entity_name, api_version)?
            }
            _ => anyhow::bail!("Unexpected type name as select base type: {}", entity_name),
        };

        let mut builder = Self::new(ty.clone());
        builder.entity = builder.load_entity(&runtime.policies, userid, path, &ty);

        let operators = builder.process_projections(operators);
        builder.operators.extend(operators);
        Ok(builder)
    }

    /// Processes Projection Operators, returns the remaining unused operators.
    fn process_projections(&mut self, mut ops: Vec<QueryOp>) -> Vec<QueryOp> {
        // FIXME: Replace this with .drain_filter() when it's moved to stable.
        for op in &ops {
            if let QueryOp::Projection { fields } = op {
                self.allowed_fields = Some(HashSet::from_iter(fields.iter().cloned()));
            }
        }
        ops.retain(|op| !matches!(op, QueryOp::Projection { .. }));
        ops
    }

    fn make_scalar_field(
        &mut self,
        field: &Field,
        table_name: &str,
        transform: Option<fn(Value) -> Value>,
    ) -> QueryField {
        let column_idx = self.columns.len();
        let select_field = QueryField::Scalar {
            name: field.name.clone(),
            type_: field.type_.clone(),
            is_optional: field.is_optional,
            column_idx,
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
    fn load_entity(
        &mut self,
        policies: &Policies,
        userid: &Option<String>,
        path: &str,
        ty: &Arc<ObjectType>,
    ) -> QueriedEntity {
        self.add_login_filters_recursive(
            policies,
            userid,
            path,
            ty,
            Expr::Parameter { position: 0 },
        );
        self.load_entity_recursive(policies, userid, path, ty, ty.backing_table())
    }

    /// Loads QueriedEntity for a given type `ty` to be retrieved from the
    /// database. For fields that represent a nested Entity a join is
    /// generated and we attempt to retrieve them recursively as well.
    fn load_entity_recursive(
        &mut self,
        policies: &Policies,
        userid: &Option<String>,
        path: &str,
        ty: &Arc<ObjectType>,
        current_table: &str,
    ) -> QueriedEntity {
        let field_policies = make_field_policies(policies, userid, path, ty);

        let mut fields = vec![];
        let mut joins = HashMap::default();
        for field in ty.all_fields() {
            let field_policy = field_policies.transforms.get(&field.name).cloned();

            let query_field = if let Type::Object(nested_ty) = &field.type_ {
                let nested_table = format!(
                    "JOIN{}_{}_TO_{}",
                    self.join_counter,
                    ty.name(),
                    nested_ty.name()
                );
                // PostgreSQL has a limit on identifiers to be at most 63 bytes long.
                let nested_table = max_prefix(nested_table.as_str(), 63).to_owned();
                self.join_counter += 1;

                self.make_scalar_field(field, current_table, field_policy);
                joins.insert(
                    field.name.to_owned(),
                    Join {
                        entity: self.load_entity_recursive(
                            policies,
                            userid,
                            path,
                            nested_ty,
                            &nested_table,
                        ),
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
        policies: &Policies,
        userid: &Option<String>,
        path: &str,
        ty: &Arc<ObjectType>,
        property_chain: Expr,
    ) {
        let field_policies = make_field_policies(policies, userid, path, ty);
        let user_id: Literal = match &field_policies.current_userid {
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
                    if field_policies.match_login.contains(&field.name) {
                        let expr = BinaryExpr {
                            left: Box::new(property_access.into()),
                            op: BinaryOp::Eq,
                            right: Box::new(user_id.clone().into()),
                        };
                        self.operators.push(QueryOp::Filter {
                            expression: expr.into(),
                        });
                    }
                } else {
                    self.add_login_filters_recursive(
                        policies,
                        userid,
                        path,
                        nested_ty,
                        property_access.into(),
                    );
                }
            }
        }
    }

    fn make_column_string(&self) -> String {
        let mut column_string = String::new();
        for c in &self.columns {
            let col = match c.field.default_value() {
                Some(dfl) => format!(
                    "coalesce(\"{}\".\"{}\",'{}') AS \"{}\",",
                    c.table_name,
                    c.name,
                    dfl,
                    c.alias()
                ),
                None => format!("\"{}\".\"{}\" AS \"{}\",", c.table_name, c.name, c.alias()),
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

    fn make_filter_string(&self, expr: &Option<Expr>) -> Result<String> {
        let where_cond = if let Some(expr) = expr {
            let condition = self.filter_expr_to_string(expr)?;
            format!("WHERE {}", condition)
        } else {
            "".to_owned()
        };
        Ok(where_cond)
    }

    fn filter_expr_to_string(&self, expr: &Expr) -> Result<String> {
        let expr_str = match &expr {
            Expr::Literal { value } => match &value {
                Literal::Bool(lit) => (if *lit { "1" } else { "0" }).to_string(),
                Literal::U64(lit) => lit.to_string(),
                Literal::I64(lit) => lit.to_string(),
                Literal::F64(lit) => lit.to_string(),
                Literal::String(lit) => escape_string(lit),
                Literal::Null => "NULL".to_string(),
            },
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

        let check_field = |entity: &QueriedEntity, field| {
            anyhow::ensure!(
                entity.has_field(field),
                "expression error: entity '{}' doesn't have field '{}'",
                entity.ty.name(),
                field
            );
            Ok(())
        };

        let mut field = &properties[0];
        let mut entity = &self.entity;
        check_field(entity, field)?;

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
            check_field(entity, field)?;
        }
        let c_alias = ColumnAlias {
            field_name: field.to_owned(),
            table_name: entity.table_alias.to_owned(),
        };

        Ok(format!("\"{}\"", c_alias))
    }

    fn make_sort_string(&self, sort: Option<&SortBy>) -> Result<String> {
        let sort_str = if let Some(sort) = sort {
            if !self.base_type().has_field(&sort.field_name) {
                anyhow::bail!(
                    "entity '{}' has no field named '{}'",
                    self.base_type().name(),
                    sort.field_name
                );
            }
            let order = if sort.ascending { "ASC" } else { "DESC" };
            let c_alias = ColumnAlias {
                field_name: sort.field_name.to_owned(),
                table_name: self.base_type().backing_table().to_owned(),
            };
            format!("ORDER BY \"{}\" {}", c_alias, order)
        } else {
            "".into()
        };
        Ok(sort_str)
    }

    fn make_limit_and_offset_string(
        &self,
        target: &TargetDatabase,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> String {
        if target.as_sqlite().is_some() && limit.is_none() && offset.is_some() {
            // Covers Sqlite not supporting standalone OFFSET statement without LIMIT.
            format!("LIMIT {},-1", offset.unwrap())
        } else {
            let limit_str = limit.map_or("".into(), |l| format!("LIMIT {}", l));
            let offset_str = offset.map_or("".into(), |o| format!("OFFSET {}", o));
            format!("{} {}", limit_str, offset_str)
        }
    }

    fn make_core_select(&self) -> String {
        let column_string = self.make_column_string();
        let join_string = self.make_join_string();
        format!(
            "SELECT {} FROM \"{}\" {}",
            column_string,
            self.base_type().backing_table(),
            join_string,
        )
    }

    /// Splits the operators' slice at a first occurrence of Take or Skip (break) operator into two slices
    /// first containing everything up to the Take|Skip (inclusive) and the second containing the
    /// remainder. Idiomatically ops = [..., Take|Skip] + [...].
    fn split_on_first_take<'a>(&self, ops: &'a [QueryOp]) -> (&'a [QueryOp], &'a [QueryOp]) {
        for (i, op) in ops.iter().enumerate() {
            match op {
                QueryOp::Take { .. } | QueryOp::Skip { .. } => {
                    return (&ops[..i + 1], &ops[i + 1..]);
                }
                _ => (),
            }
        }
        (ops, &[])
    }

    fn gather_filters(&self, ops: &[QueryOp]) -> Option<Expr> {
        let mut expr = None;
        for op in ops {
            if let QueryOp::Filter { expression } = op {
                if let Some(filter_expr) = expr {
                    let new_expr = BinaryExpr {
                        left: Box::new(filter_expr),
                        op: BinaryOp::And,
                        right: Box::new(expression.clone()),
                    };
                    expr = Some(new_expr.into());
                } else {
                    expr = Some(expression.clone());
                }
            }
        }
        expr
    }

    fn find_last_sort_by<'a>(&self, ops: &'a [QueryOp]) -> Option<&'a SortBy> {
        ops.iter()
            .rfind(|op| op.as_sort_by().is_some())
            .map(|op| op.as_sort_by().unwrap())
    }

    fn find_take_count(&self, ops: &[QueryOp]) -> Option<u64> {
        assert!(ops.iter().filter(|op| op.as_take().is_some()).count() <= 1);
        ops.iter()
            .rfind(|op| op.as_take().is_some())
            .map(|op| *op.as_take().unwrap())
    }

    fn find_skip_count(&self, ops: &[QueryOp]) -> Option<u64> {
        assert!(ops.iter().filter(|op| op.as_skip().is_some()).count() <= 1);
        ops.iter()
            .rfind(|op| op.as_skip().is_some())
            .map(|op| *op.as_skip().unwrap())
    }

    fn make_raw_query(&self, target: TargetDatabase) -> Result<String> {
        let mut sql_query = self.make_core_select();
        let mut remaining_ops: &[QueryOp] = &self.operators[..];
        while !remaining_ops.is_empty() {
            let (ops, remainder) = self.split_on_first_take(remaining_ops);
            remaining_ops = remainder;

            let filter_expr = self.gather_filters(ops);
            let filter_string = self.make_filter_string(&filter_expr)?;

            let sort = self.find_last_sort_by(ops);
            let sort_string = self.make_sort_string(sort)?;

            let limit = self.find_take_count(ops);
            let offset = self.find_skip_count(ops);
            let lo_string = self.make_limit_and_offset_string(&target, limit, offset);

            // The "AS subquery" part is necessary to make Postgres happy.
            sql_query = format!(
                "SELECT * FROM ({}) AS subquery {} {} {}",
                sql_query, filter_string, sort_string, lo_string
            );
        }
        Ok(sql_query)
    }

    pub(crate) fn build_query(&self, target: TargetDatabase) -> Result<Query> {
        Ok(Query {
            raw_sql: self.make_raw_query(target)?,
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
pub(crate) enum QueryOpChain {
    BaseEntity {
        name: String,
    },
    #[serde(rename = "ExpressionFilter")]
    Filter {
        expression: Expr,
        inner: Box<QueryOpChain>,
    },
    #[serde(rename = "ColumnsSelect")]
    Projection {
        #[serde(rename = "columns")]
        fields: Vec<String>,
        inner: Box<QueryOpChain>,
    },
    Take {
        count: u64,
        inner: Box<QueryOpChain>,
    },
    Skip {
        count: u64,
        inner: Box<QueryOpChain>,
    },
    SortBy {
        #[serde(rename = "key")]
        field_name: String,
        ascending: bool,
        inner: Box<QueryOpChain>,
    },
}

/// Converts operator chain into a tuple `(entity_name, ops)`, where
/// `entity_name` is the name taken from the BaseEntity which corresponds to
/// Entity which is to be queried. `ops` are a Vector of Operators that
/// are to be applied on the resulting entity elements in order that
/// is defined by the vector.
fn convert_ops(op: QueryOpChain) -> Result<(String, Vec<QueryOp>)> {
    use QueryOpChain::*;
    let (query_op, inner) = match op {
        BaseEntity { name } => {
            return Ok((name, vec![]));
        }
        Filter { expression, inner } => (QueryOp::Filter { expression }, inner),
        Projection { fields, inner } => (QueryOp::Projection { fields }, inner),
        Take { count, inner } => (QueryOp::Take { count }, inner),
        Skip { count, inner } => (QueryOp::Skip { count }, inner),
        SortBy {
            field_name,
            ascending,
            inner,
        } => (
            QueryOp::SortBy(super::query::SortBy {
                field_name,
                ascending,
            }),
            inner,
        ),
    };
    let (entity_name, mut ops) = convert_ops(*inner)?;
    ops.push(query_op);
    Ok((entity_name, ops))
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
    pub(crate) fn parse_delete(
        type_system: &TypeSystem,
        api_version: &str,
        type_name: &str,
        restrictions: &JsonObject,
    ) -> Result<Self> {
        let (ty, restrictions) = {
            let ty = match type_system.lookup_custom_type(type_name, api_version) {
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
