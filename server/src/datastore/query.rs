// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::datastore::crud;
use crate::datastore::expr::{BinaryExpr, BinaryOp, Expr, Literal, PropertyAccess};
use crate::policies::{FieldPolicies, Policies};
use crate::types::TypeSystem;
use crate::types::{Field, ObjectType, Type, TypeSystemError, OAUTHUSER_TYPE_NAME};

use anyhow::{anyhow, Context, Result};
use enum_as_inner::EnumAsInner;
use serde_derive::{Deserialize, Serialize};
use serde_json::value::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Clone, EnumAsInner)]
pub(crate) enum SqlValue {
    Bool(bool),
    F64(f64),
    String(String),
}

impl From<&str> for SqlValue {
    fn from(f: &str) -> Self {
        Self::String(f.to_string())
    }
}

/// RequestContext bears a mix of contextual variables used by QueryPlan
/// and Mutations.
pub(crate) struct RequestContext<'a> {
    /// Policies to be applied on the query.
    pub policies: &'a Policies,
    /// Type system to be used of version `api_version`
    pub ts: &'a TypeSystem,
    /// Schema version to be used.
    pub api_version: String,
    /// Id of user making the request.
    pub user_id: Option<String>,
    /// Current URL path from which this request originated.
    pub path: String,
}

impl RequestContext<'_> {
    /// Calculates field policies for the request being processed.
    fn make_field_policies(&self, ty: &ObjectType) -> FieldPolicies {
        self.policies
            .make_field_policies(&self.user_id, &self.path, ty)
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
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone)]
pub(crate) struct SortBy {
    pub field_name: String,
    pub ascending: bool,
}

/// Operators used to mutate the result set.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, EnumAsInner)]
pub(crate) enum QueryOp {
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

    fn from_entity_name(c: &RequestContext, entity_name: &str) -> Result<Self> {
        let ty = match c.ts.lookup_builtin_type(entity_name) {
            Ok(Type::Object(ty)) => ty,
            Err(TypeSystemError::NotABuiltinType(_)) => {
                c.ts.lookup_custom_type(entity_name, &c.api_version)?
            }
            _ => anyhow::bail!("Unexpected type name as select base type: {}", entity_name),
        };

        let mut builder = Self::new(ty.clone());
        builder.entity = builder.load_entity(c, &ty);
        Ok(builder)
    }

    /// Constructs query plan from CRUD URL query parameters. It parses the query
    /// string contain within given `url` and loads provided querying parameters
    /// into the query plan.
    pub(crate) fn from_crud_url(
        context: &RequestContext,
        entity_name: &str,
        url: &str,
    ) -> Result<Self> {
        let mut builder = Self::from_entity_name(context, entity_name)?;
        let operators = crud::query_str_to_ops(builder.base_type(), url)?;
        builder.extend_operators(operators);

        Ok(builder)
    }

    /// Constructs a query plan from a query `op_chain` and
    /// additional helper data like `policies`, `api_version`,
    /// `userid` and `path` (url path used for policy evaluation).
    pub(crate) fn from_op_chain(context: &RequestContext, op_chain: QueryOpChain) -> Result<Self> {
        let (entity_name, operators) = convert_ops(op_chain)?;
        let mut builder = Self::from_entity_name(context, &entity_name)?;

        builder.extend_operators(operators);
        Ok(builder)
    }

    fn extend_operators(&mut self, ops: Vec<QueryOp>) {
        let ops = self.process_projections(ops);
        self.operators.extend(ops);
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
    fn load_entity(&mut self, context: &RequestContext, ty: &Arc<ObjectType>) -> QueriedEntity {
        self.add_login_filters_recursive(context, ty, Expr::Parameter { position: 0 });
        self.load_entity_recursive(context, ty, ty.backing_table())
    }

    /// Loads QueriedEntity for a given type `ty` to be retrieved from the
    /// database. For fields that represent a nested Entity a join is
    /// generated and we attempt to retrieve them recursively as well.
    fn load_entity_recursive(
        &mut self,
        context: &RequestContext,
        ty: &Arc<ObjectType>,
        current_table: &str,
    ) -> QueriedEntity {
        let field_policies = context.make_field_policies(ty);

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
                        entity: self.load_entity_recursive(context, nested_ty, &nested_table),
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
        context: &RequestContext,
        ty: &Arc<ObjectType>,
        property_chain: Expr,
    ) {
        let field_policies = context.make_field_policies(ty);
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
                    self.add_login_filters_recursive(context, nested_ty, property_access.into());
                }
            }
        }
    }

    fn make_column_string(&self, target: &TargetDatabase) -> String {
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
        // Adds row number selection for DELETE.
        let row_id = target.as_sqlite().map_or("ctid", |_| "rowid");
        column_string += &format!(
            "\"{}\".\"{row_id}\" AS \"{row_id}\",",
            self.base_type().backing_table(),
            row_id = row_id
        );
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

    fn make_core_select(&self, target: &TargetDatabase) -> String {
        let column_string = self.make_column_string(target);
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

    fn make_raw_query(&self, target: &TargetDatabase) -> Result<String> {
        let mut sql_query = self.make_core_select(target);
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
            let lo_string = self.make_limit_and_offset_string(target, limit, offset);

            // The "AS subquery" part is necessary to make Postgres happy.
            sql_query = format!(
                "SELECT * FROM ({}) AS subquery {} {} {}",
                sql_query, filter_string, sort_string, lo_string
            );
        }
        Ok(sql_query)
    }

    pub(crate) fn build_query(&self, target: &TargetDatabase) -> Result<Query> {
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

/// `Mutation` represents a statement that mutates the database state.
pub(crate) struct Mutation {
    base_entity: Arc<ObjectType>,
    /// Query plan used to build mutation condition.
    filter_query_plan: QueryPlan,
}

impl Mutation {
    /// Constructs delete from filter expression.
    pub(crate) fn delete_with_expr(
        c: &RequestContext,
        entity_name: &str,
        filter_expr: &Option<Expr>,
    ) -> Result<Self> {
        let base_entity = match c.ts.lookup_type(entity_name, &c.api_version) {
            Ok(Type::Object(ty)) => ty,
            Ok(ty) => anyhow::bail!(
                "Cannot delete builtin-in type {} ({})",
                entity_name,
                ty.name()
            ),
            Err(_) => anyhow::bail!(
                "Cannot delete from entity `{}`, entity not found",
                entity_name
            ),
        };

        let mut query_plan = QueryPlan::from_entity_name(c, entity_name)?;
        if let Some(expr) = filter_expr {
            query_plan.extend_operators(vec![QueryOp::Filter {
                expression: expr.clone(),
            }]);
        }
        Ok(Self {
            base_entity,
            filter_query_plan: query_plan,
        })
    }

    pub(crate) fn delete_from_crud_url(
        c: &RequestContext,
        type_name: &str,
        url: &str,
    ) -> Result<Self> {
        let base_entity = match c.ts.lookup_type(type_name, &c.api_version) {
            Ok(Type::Object(ty)) => ty,
            Ok(ty) => anyhow::bail!(
                "Cannot delete builtin-in type {} ({})",
                type_name,
                ty.name()
            ),
            Err(_) => anyhow::bail!("Cannot delete from type `{}`, type not found", type_name),
        };
        let filter_expr = crud::url_to_filter(&base_entity, url)
            .context("failed to convert crud URL to filter expression")?;
        if filter_expr.is_none() {
            let q = Url::parse(url)
                .with_context(|| format!("failed to parse query string '{}'", url))?;
            let delete_all = q
                .query_pairs()
                .any(|(key, value)| key == "all" && value == "true");
            if !delete_all {
                anyhow::bail!("crud delete requires a filter to be set or `all=true` parameter.")
            }
        }
        Self::delete_with_expr(c, type_name, &filter_expr)
    }

    pub(crate) fn build_sql(&self, target: TargetDatabase) -> Result<String> {
        let select_sql = self.filter_query_plan.build_query(&target)?.raw_sql;
        let row_id = target.as_sqlite().map_or("ctid", |_| "rowid");
        let raw_sql = format!(
            r#"DELETE FROM "{base_table}"
                WHERE {row_id} IN (
                    SELECT {row_id} FROM ({select_sql}) as subquery
                )"#,
            select_sql = select_sql,
            row_id = row_id,
            base_table = &self.base_entity.backing_table(),
        );
        Ok(raw_sql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use serde_json::json;
    use tempfile::NamedTempFile;

    use crate::datastore::{DbConnection, QueryEngine};
    use crate::types;
    use crate::JsonObject;

    const VERSION: &str = "version_1";

    fn binary(fields: &[&'static str], op: BinaryOp, literal: Literal) -> Expr {
        assert!(!fields.len() > 0);
        let mut field_chain = Expr::Parameter { position: 0 };
        for field_name in fields {
            field_chain = PropertyAccess {
                property: field_name.to_string(),
                object: field_chain.into(),
            }
            .into();
        }
        BinaryExpr {
            left: Box::new(field_chain),
            op,
            right: Box::new(literal.into()),
        }
        .into()
    }

    fn make_type_system(entities: &[&Arc<ObjectType>]) -> TypeSystem {
        let mut ts = TypeSystem::default();
        for &ty in entities {
            ts.add_type(ty.clone()).unwrap();
        }
        ts
    }

    fn make_object(name: &str, fields: Vec<Field>) -> Arc<ObjectType> {
        let desc = types::NewObject::new(name, VERSION);
        Arc::new(ObjectType::new(desc, fields).unwrap())
    }

    fn make_field(name: &str, ty: Type) -> Field {
        let desc = types::NewField::new(name, ty, VERSION).unwrap();
        Field::new(desc, vec![], None, false, false)
    }

    async fn init_query_engine(db_file: &NamedTempFile) -> QueryEngine {
        let db_uri = format!("sqlite://{}?mode=rwc", db_file.path().to_string_lossy());
        let data_db = DbConnection::connect(&db_uri, 1).await.unwrap();
        let query_engine = QueryEngine::local_connection(&data_db, 1).await.unwrap();
        query_engine
    }

    async fn init_database(query_engine: &QueryEngine, entities: &[&Arc<ObjectType>]) {
        let mut tr = query_engine.start_transaction().await.unwrap();
        for entity in entities {
            query_engine.create_table(&mut tr, entity).await.unwrap();
        }
        QueryEngine::commit_transaction(tr).await.unwrap();
    }

    async fn setup_clear_db(entities: &[&Arc<ObjectType>]) -> (QueryEngine, NamedTempFile) {
        let db_file = NamedTempFile::new().unwrap();
        let qe = init_query_engine(&db_file).await;
        init_database(&qe, entities).await;
        (qe, db_file)
    }

    async fn add_row(
        query_engine: &QueryEngine,
        entity: &Arc<ObjectType>,
        values: &serde_json::Value,
    ) {
        let ins_row = values.as_object().unwrap();
        query_engine.add_row(entity, ins_row, None).await.unwrap();
        let rows = fetch_rows(query_engine, entity).await;
        assert!(rows.iter().any(|row| {
            ins_row.iter().all(|(key, value)| {
                if let Type::Object(_) = entity.get_field(key).unwrap().type_ {
                    true
                } else {
                    row[key] == *value
                }
            })
        }));
    }

    async fn fetch_rows(qe: &QueryEngine, entity: &Arc<ObjectType>) -> Vec<JsonObject> {
        let qe = Arc::new(qe.clone());
        let query_plan = QueryPlan::from_type(entity);
        let tr = qe.clone().start_transaction_static().await.unwrap();
        let row_streams = qe.query(tr, query_plan).unwrap();

        row_streams
            .map(|row| row.unwrap())
            .collect::<Vec<JsonObject>>()
            .await
    }

    lazy_static! {
        static ref PERSON_TY: Arc<ObjectType> = make_object(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        );
        static ref COMPANY_TY: Arc<ObjectType> = make_object(
            "Company",
            vec![
                make_field("name", Type::String),
                make_field("ceo", Type::Object(PERSON_TY.clone())),
            ],
        );
        static ref ENTITIES: [&'static Arc<ObjectType>; 2] = [&*PERSON_TY, &*COMPANY_TY];
        static ref TS: TypeSystem = make_type_system(&*ENTITIES);
    }

    #[tokio::test]
    async fn test_delete_with_expr() {
        let delete_with_expr = |entity_name: &str, expr: Expr| {
            Mutation::delete_with_expr(
                &RequestContext {
                    policies: &Policies::default(),
                    ts: &make_type_system(&*ENTITIES),
                    api_version: VERSION.to_owned(),
                    user_id: None,
                    path: "".to_string(),
                },
                entity_name,
                &Some(expr),
            )
            .unwrap()
        };

        let john = json!({"name": "John", "age": json!(20f32)});
        let alan = json!({"name": "Alan", "age": json!(30f32)});
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &PERSON_TY, &john).await;

            let expr = binary(&["name"], BinaryOp::Eq, "John".into());
            let mutation = delete_with_expr("Person", expr);
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &PERSON_TY).await.len(), 0);
        }
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &PERSON_TY, &john).await;
            add_row(&qe, &PERSON_TY, &alan).await;

            let expr = binary(&["age"], BinaryOp::Eq, (30.).into());
            let mutation = delete_with_expr("Person", expr);
            qe.mutate(mutation).await.unwrap();

            let rows = fetch_rows(&qe, &PERSON_TY).await;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["name"], "John");
        }

        let chiselstrike = json!({"name": "ChiselStrike", "ceo": john});
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &COMPANY_TY, &chiselstrike).await;

            let expr = binary(&["ceo", "name"], BinaryOp::Eq, "John".into());
            let mutation = delete_with_expr("Company", expr);
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &COMPANY_TY).await.len(), 0);
        }
    }

    #[tokio::test]
    async fn test_delete_from_crud_url() {
        fn url(query_string: &str) -> String {
            format!("http://wtf?{}", query_string)
        }

        let delete_from_url = |entity_name: &str, url: &str| {
            Mutation::delete_from_crud_url(
                &RequestContext {
                    policies: &Policies::default(),
                    ts: &make_type_system(&*ENTITIES),
                    api_version: VERSION.to_owned(),
                    user_id: None,
                    path: "".to_string(),
                },
                entity_name,
                url,
            )
            .unwrap()
        };

        let john = json!({"name": "John", "age": json!(20f32)});
        let alan = json!({"name": "Alan", "age": json!(30f32)});
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &PERSON_TY, &john).await;

            let mutation = delete_from_url("Person", &url(".name=John"));
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &PERSON_TY).await.len(), 0);
        }
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &PERSON_TY, &john).await;
            add_row(&qe, &PERSON_TY, &alan).await;

            let mutation = delete_from_url("Person", &url(".age=30"));
            qe.mutate(mutation).await.unwrap();

            let rows = fetch_rows(&qe, &PERSON_TY).await;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["name"], "John");
        }

        let chiselstrike = json!({"name": "ChiselStrike", "ceo": john});
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &COMPANY_TY, &chiselstrike).await;

            let mutation = delete_from_url("Company", &url(".ceo.name=John"));
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &COMPANY_TY).await.len(), 0);
        }
    }
}
