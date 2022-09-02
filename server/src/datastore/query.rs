// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

use crate::auth::AUTH_USER_NAME;
use crate::datastore::expr::{BinaryExpr, Expr, PropertyAccess, Value as ExprValue};
use crate::deno::ChiselRequestContext;
use crate::policies::{FieldPolicies, Policies};
use crate::types::{Entity, Field, ObjectType, Type, TypeId, TypeSystem};

use anyhow::{anyhow, Context, Result};
use enum_as_inner::EnumAsInner;
use serde_derive::{Deserialize, Serialize};
use serde_json::value::Value as JsonValue;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Write;
use std::ops::Deref;

#[derive(Debug, Clone, EnumAsInner)]
pub enum SqlValue {
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
pub struct RequestContext<'a> {
    pub inner: ChiselRequestContext,
    /// Policies to be applied on the query.
    pub policies: &'a Policies,
    /// Type system to be used of version `api_version`
    pub ts: &'a TypeSystem,
}

impl Deref for RequestContext<'_> {
    type Target = ChiselRequestContext;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl RequestContext<'_> {
    /// Calculates field policies for the request being processed.
    fn make_field_policies(&self, ty: &ObjectType) -> FieldPolicies {
        self.policies
            .make_field_policies(&self.user_id, &self.path, ty)
    }
}

/// Whether a field should be included in or omitted from query result.
#[derive(Debug, Clone)]
pub enum KeepOrOmitField {
    Keep,
    Omit,
}

#[derive(Debug, Clone)]
pub enum QueryField {
    Scalar {
        /// Name of the original Type field
        name: String,
        /// Type of the field
        type_id: TypeId,
        is_optional: bool,
        /// Index of a column containing this field in the resulting row we get from
        /// the database.
        column_idx: usize,
        /// Policy transformation to be applied on the resulting JSON value.
        transform: Option<fn(JsonValue) -> JsonValue>,
        /// Do not include field in return json
        keep_or_omit: KeepOrOmitField,
    },
    Entity {
        /// Name of the original Type field
        name: String,
        is_optional: bool,
        /// Policy transformation to be applied on the resulting JSON value.
        transform: Option<fn(JsonValue) -> JsonValue>,
        /// Do not include field in return json
        keep_or_omit: KeepOrOmitField,
    },
}

/// `Query` is a structure that represents an executable query.
///
/// A query represents a full query including filtering, projection, joins,
/// and so on. The `execute` method of `QueryEngine` executes these queries
/// using SQL and the policy engine.
#[derive(Debug, Clone)]
pub struct Query {
    /// SQL query text
    pub raw_sql: String,
    /// Entity that is being queried. Contains information necessary to reconstruct
    /// the JSON response.
    pub entity: QueriedEntity,
    /// Entity fields selected by the user. This field is used to post-filter fields that
    /// shall be returned to the user in JSON.
    /// FIXME: The post-filtering is suboptimal solution and selection should happen when
    /// we build the raw_sql query.
    pub allowed_fields: Option<HashSet<String>>,
}

/// QueriedEntity represents queried Entity of type `ty` which is to be aliased as
/// `table_alias` in the SQL query and joined with nested Entities using `joins`.
#[derive(Debug, Clone)]
pub struct QueriedEntity {
    /// Entity fields to be returned in JSON response
    pub fields: Vec<QueryField>,
    /// Type of the entity.
    ty: Entity,
    /// Alias name of this entity to be used in SQL query.
    table_alias: String,
    /// Map from Entity field name to joined Entities which correspond to the entities
    /// stored under the field name.
    joins: HashMap<String, Join>,
}

impl QueriedEntity {
    pub fn get_child_entity<'a>(&'a self, child_name: &str) -> Option<&'a QueriedEntity> {
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

/// SortKey specifies a `field_name` and ordering in which sorting should be done.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SortKey {
    #[serde(rename = "fieldName")]
    pub field_name: String,
    pub ascending: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SortBy {
    pub keys: Vec<SortKey>,
}

/// Operators used to mutate the result set.
#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Clone, EnumAsInner)]
pub enum QueryOp {
    /// Filters elements by `expression`.
    Filter { expression: Expr },
    /// Projects QueryEntity to a projected Entity containing only `fields`.
    Projection { fields: Vec<String> },
    /// Limits the number of output rows by taking the first `count` rows.
    Take { count: u64 },
    /// Skips the first `count` rows.
    Skip { count: u64 },
    /// Lexicographically sorts elements using `SortKey`s.
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
pub enum TargetDatabase {
    Postgres,
    Sqlite,
}

/// Class used to build `Query` from either QueryOpChain or `Entity`.
/// For the op chain, it recursively descends through selected fields and captures all
/// joins necessary for nested types retrieval.
/// When we are done with that, `build_query` can be called which creates a `Query`
/// structure that contains raw SQL query string and additional data necessary for
/// JSON response reconstruction and filtering.
pub struct QueryPlan {
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
    fn new(base_type: Entity) -> Self {
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

    fn base_type(&self) -> &Entity {
        &self.entity.ty
    }

    /// Constructs a query builder ready to build an expression querying all fields of a
    /// given type `ty`. This is done in a shallow manner. Columns representing foreign
    /// key are returned as string, not as the related Entity.
    pub fn from_type(ty: &Entity) -> Self {
        let mut builder = Self::new(ty.clone());
        for field in ty.all_fields() {
            let mut field = field.clone();
            field.type_id = match field.type_id {
                TypeId::Entity { .. } => TypeId::String, // This is actually a foreign key.
                ty => ty,
            };
            let field =
                builder.make_scalar_field(&field, ty.backing_table(), None, &KeepOrOmitField::Keep);
            builder.entity.fields.push(field)
        }
        builder
    }

    fn from_entity_name(c: &RequestContext, entity_name: &str) -> Result<Self> {
        let ty = c
            .ts
            .lookup_entity(entity_name, &c.api_version)
            .with_context(|| {
                format!("unable to construct QueryPlan from an unknown entity name `{entity_name}`")
            })?;

        let mut builder = Self::new(ty.clone());
        builder.entity = builder.load_entity(c, &ty)?;
        Ok(builder)
    }

    /// Constructs QueryPlan from type `ty` and application of given
    /// `operators.
    pub fn from_ops(c: &RequestContext, ty: &Entity, operators: Vec<QueryOp>) -> Result<Self> {
        let mut query_plan = Self::new(ty.clone());
        query_plan.entity = query_plan.load_entity(c, ty)?;
        query_plan.extend_operators(operators);
        Ok(query_plan)
    }

    /// Constructs a query plan from a query `op_chain` and
    /// additional helper data like `policies`, `api_version`,
    /// `userid` and `path` (url path used for policy evaluation).
    pub fn from_op_chain(context: &RequestContext, op_chain: QueryOpChain) -> Result<Self> {
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
        transform: Option<fn(JsonValue) -> JsonValue>,
        keep_or_omit: &KeepOrOmitField,
    ) -> QueryField {
        let column_idx = self.columns.len();
        let select_field = QueryField::Scalar {
            name: field.name.clone(),
            type_id: field.type_id.clone(),
            is_optional: field.is_optional,
            column_idx,
            transform,
            keep_or_omit: keep_or_omit.clone(),
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
        context: &RequestContext,
        ty: &Entity,
    ) -> anyhow::Result<QueriedEntity> {
        self.add_login_filters_recursive(context, ty, Expr::Parameter { position: 0 })?;
        self.load_entity_recursive(context, ty, ty.backing_table())
    }

    /// Loads QueriedEntity for a given type `ty` to be retrieved from the
    /// database. For fields that represent a nested Entity a join is
    /// generated and we attempt to retrieve them recursively as well.
    fn load_entity_recursive(
        &mut self,
        context: &RequestContext,
        ty: &Entity,
        current_table: &str,
    ) -> anyhow::Result<QueriedEntity> {
        let field_policies = context.make_field_policies(ty);

        let mut fields = vec![];
        let mut joins = HashMap::default();
        for field in ty.all_fields() {
            let field_policy = field_policies.transforms.get(&field.name).cloned();
            let keep_or_omit = match field_policies.omit.contains(&field.name) {
                true => KeepOrOmitField::Omit,
                _ => KeepOrOmitField::Keep,
            };

            let ty = context.ts.get(&field.type_id)?;

            let query_field = if let Type::Entity(nested_ty) = &ty {
                let nested_table = format!(
                    "JOIN{}_{}_TO_{}",
                    self.join_counter,
                    ty.name(),
                    nested_ty.name()
                );
                // PostgreSQL has a limit on identifiers to be at most 63 bytes long.
                let nested_table = truncate_identifier(nested_table.as_str()).to_owned();
                self.join_counter += 1;

                self.make_scalar_field(field, current_table, field_policy, &keep_or_omit);
                joins.insert(
                    field.name.to_owned(),
                    Join {
                        entity: self.load_entity_recursive(context, nested_ty, &nested_table)?,
                        lkey: field.name.to_owned(),
                        rkey: "id".to_owned(),
                    },
                );
                QueryField::Entity {
                    name: field.name.clone(),
                    is_optional: field.is_optional,
                    transform: field_policy,
                    keep_or_omit,
                }
            } else {
                self.make_scalar_field(field, current_table, field_policy, &keep_or_omit)
            };
            fields.push(query_field);
        }

        Ok(QueriedEntity {
            ty: ty.clone(),
            fields,
            table_alias: current_table.to_owned(),
            joins,
        })
    }

    /// Adds filters that ensure login constrains are satisfied for a type
    /// `ty` that is to be retrieved from the database.
    fn add_login_filters_recursive(
        &mut self,
        context: &RequestContext,
        ty: &Entity,
        property_chain: Expr,
    ) -> anyhow::Result<()> {
        let field_policies = context.make_field_policies(ty);
        let user_id: ExprValue = match &field_policies.current_userid {
            None => "NULL",
            Some(id) => id.as_str(),
        }
        .into();

        for field in ty.all_fields() {
            let ty = context.ts.get(&field.type_id)?;
            if let Type::Entity(nested_ty) = &ty {
                let property_access = PropertyAccess {
                    property: field.name.to_owned(),
                    object: property_chain.clone().into(),
                };
                if nested_ty.name() == AUTH_USER_NAME {
                    if field_policies.match_login.contains(&field.name) {
                        let expr = BinaryExpr::eq(property_access.into(), user_id.clone().into());
                        self.operators.push(QueryOp::Filter { expression: expr });
                    }
                } else {
                    self.add_login_filters_recursive(context, nested_ty, property_access.into())?;
                }
            }
        }

        Ok(())
    }

    fn make_column_string(&self) -> String {
        let mut column_string = String::new();
        for c in &self.columns {
            let col = match c.field.default_value() {
                Some(dfl) => {
                    let sql_default = match c.field.type_id {
                        TypeId::String => format!("'{}'", dfl),
                        _ => dfl.to_string(),
                    };
                    format!(
                        "coalesce(\"{}\".\"{}\",{}) AS \"{}\",",
                        c.table_name,
                        c.name,
                        sql_default,
                        c.alias()
                    )
                }
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
                writeln!(
                    join_string,
                    "LEFT JOIN \"{}\" AS \"{}\" ON \"{}\".\"{}\"=\"{}\".\"{}\"",
                    join.entity.ty.backing_table(),
                    join.entity.table_alias,
                    entity.table_alias,
                    join.lkey,
                    join.entity.table_alias,
                    join.rkey
                )
                .unwrap();
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
            Expr::Value { value } => match &value {
                ExprValue::Bool(value) => (if *value { "true" } else { "false" }).to_string(),
                ExprValue::U64(value) => value.to_string(),
                ExprValue::I64(value) => value.to_string(),
                ExprValue::F64(value) => value.to_string(),
                ExprValue::String(value) => escape_string(value),
                ExprValue::Null => "NULL".to_string(),
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
            let mut order_tokens = vec![];
            for sort_key in &sort.keys {
                if !self.base_type().has_field(&sort_key.field_name) {
                    anyhow::bail!(
                        "entity '{}' has no field named '{}'",
                        self.base_type().name(),
                        sort_key.field_name
                    );
                }
                let order = if sort_key.ascending { "ASC" } else { "DESC" };
                let c_alias = ColumnAlias {
                    field_name: sort_key.field_name.to_owned(),
                    table_name: self.base_type().backing_table().to_owned(),
                };
                order_tokens.push(format!("\"{c_alias}\" {order}"));
            }
            format!("ORDER BY {}", order_tokens.join(", "))
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
                    let new_expr = BinaryExpr::and(filter_expr, expression.clone());
                    expr = Some(new_expr);
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
            let lo_string = self.make_limit_and_offset_string(target, limit, offset);

            // The "AS subquery" part is necessary to make Postgres happy.
            sql_query = format!(
                "SELECT * FROM ({}) AS subquery {} {} {}",
                sql_query, filter_string, sort_string, lo_string
            );
        }
        Ok(sql_query)
    }

    pub fn build_query(&self, target: &TargetDatabase) -> Result<Query> {
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

/// Truncates Database identifier (column/table name) to 63 bytes to make it
/// Postgres-compatible.
pub fn truncate_identifier(s: &str) -> &str {
    max_prefix(s, 63)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum QueryOpChain {
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
        keys: Vec<SortKey>,
        inner: Box<QueryOpChain>,
    },
}

/// Converts operator chain into a tuple `(entity_name, ops)`, where
/// `entity_name` is the name taken from the BaseEntity which corresponds to
/// Entity which is to be queried. `ops` are a Vector of Operators that
/// are to be applied on the resulting entity elements in order that
/// is defined by the vector.
fn convert_ops(op: QueryOpChain) -> Result<(String, Vec<QueryOp>)> {
    use QueryOpChain as Op;
    let (query_op, inner): (QueryOp, _) = match op {
        Op::BaseEntity { name } => {
            return Ok((name, vec![]));
        }
        Op::Filter { expression, inner } => (QueryOp::Filter { expression }, inner),
        Op::Projection { fields, inner } => (QueryOp::Projection { fields }, inner),
        Op::Take { count, inner } => (QueryOp::Take { count }, inner),
        Op::Skip { count, inner } => (QueryOp::Skip { count }, inner),
        Op::SortBy { keys, inner } => (QueryOp::SortBy(SortBy { keys }), inner),
    };
    let (entity_name, mut ops) = convert_ops(*inner)?;
    ops.push(query_op);
    Ok((entity_name, ops))
}

/// `Mutation` represents a statement that mutates the database state.
pub struct Mutation {
    base_entity: Entity,
    /// Query plan used to build mutation condition.
    filter_query_plan: QueryPlan,
}

impl Mutation {
    /// Constructs delete from filter expression.
    pub fn delete_from_expr(
        c: &RequestContext,
        type_name: &str,
        filter_expr: &Option<Expr>,
    ) -> Result<Self> {
        let base_entity = match c.ts.lookup_type(type_name, &c.api_version) {
            Ok(Type::Entity(ty)) => ty,
            Ok(ty) => anyhow::bail!("Cannot delete scalar type {type_name} ({})", ty.name()),
            Err(_) => anyhow::bail!("Cannot delete from type `{type_name}`, type not found"),
        };

        let mut query_plan = QueryPlan::from_entity_name(c, type_name)?;
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

    pub fn build_sql(&self, target: TargetDatabase) -> Result<String> {
        let select_sql = self.filter_query_plan.build_query(&target)?.raw_sql;
        let id_column = ColumnAlias {
            field_name: "id".to_owned(),
            table_name: self.base_entity.backing_table().to_owned(),
        };
        let raw_sql = format!(
            r#"DELETE FROM "{base_table}"
                WHERE "id" IN (
                    SELECT "{id_column}" FROM ({select_sql}) as subquery
                )"#,
            base_table = &self.base_entity.backing_table(),
        );
        Ok(raw_sql)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use deno_core::futures;
    use futures::StreamExt;
    use once_cell::sync::Lazy;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    use crate::datastore::expr::BinaryOp;
    use crate::datastore::{DbConnection, QueryEngine};
    use crate::types;
    use crate::JsonObject;

    pub const VERSION: &str = "version_1";

    pub fn binary(fields: &[&'static str], op: BinaryOp, value: ExprValue) -> Expr {
        assert!(!fields.len() > 0);
        let mut field_chain = Expr::Parameter { position: 0 };
        for field_name in fields {
            field_chain = PropertyAccess {
                property: field_name.to_string(),
                object: field_chain.into(),
            }
            .into();
        }
        BinaryExpr::new(op, field_chain, value.into()).into()
    }

    pub fn make_type_system(entities: &[Entity]) -> TypeSystem {
        let mut ts = TypeSystem::default();
        for ty in entities {
            ts.add_custom_type(ty.clone()).unwrap();
        }
        ts
    }

    pub fn make_entity(name: &str, fields: Vec<Field>) -> Entity {
        let desc = types::NewObject::new(name, VERSION);
        Entity::Custom(Arc::new(ObjectType::new(&desc, fields, vec![]).unwrap()))
    }

    pub fn make_field(name: &str, ty: Type) -> Field {
        let desc = types::NewField::new(name, ty, VERSION).unwrap();
        Field::new(&desc, vec![], None, false, false)
    }

    async fn init_query_engine(db_file: &NamedTempFile) -> QueryEngine {
        let db_uri = format!("sqlite://{}?mode=rwc", db_file.path().to_string_lossy());
        let data_db = DbConnection::connect(&db_uri, 1).await.unwrap();
        QueryEngine::local_connection(&data_db, 1).await.unwrap()
    }

    async fn init_database(query_engine: &QueryEngine, entities: &[Entity]) {
        let mut tr = query_engine.begin_transaction().await.unwrap();
        for entity in entities {
            query_engine.create_table(&mut tr, entity).await.unwrap();
        }
        QueryEngine::commit_transaction(tr).await.unwrap();
    }

    pub async fn setup_clear_db(entities: &[Entity]) -> (QueryEngine, NamedTempFile) {
        let db_file = NamedTempFile::new().unwrap();
        let qe = init_query_engine(&db_file).await;
        init_database(&qe, entities).await;
        (qe, db_file)
    }

    pub async fn add_row(
        query_engine: &QueryEngine,
        entity: &Entity,
        values: &serde_json::Value,
        ts: &TypeSystem,
    ) {
        let ins_row = values.as_object().unwrap();
        query_engine
            .add_row(entity, ins_row, None, ts)
            .await
            .unwrap();
        let rows = fetch_rows(query_engine, entity).await;
        assert!(rows.iter().any(|row| {
            ins_row.iter().all(|(key, value)| {
                if let TypeId::Entity { .. } = entity.get_field(key).unwrap().type_id {
                    true
                } else {
                    row[key] == *value
                }
            })
        }));
    }

    pub async fn fetch_rows(qe: &QueryEngine, entity: &Entity) -> Vec<JsonObject> {
        let query_plan = QueryPlan::from_type(entity);
        fetch_rows_with_plan(qe, query_plan).await
    }

    async fn fetch_rows_with_plan(qe: &QueryEngine, query_plan: QueryPlan) -> Vec<JsonObject> {
        let qe = Arc::new(qe.clone());
        let tr = qe.clone().begin_transaction_static().await.unwrap();
        let row_streams = qe.query(tr, query_plan).unwrap();

        row_streams
            .map(|row| row.unwrap())
            .collect::<Vec<JsonObject>>()
            .await
    }

    static PERSON_TY: Lazy<Entity> = Lazy::new(|| {
        make_entity(
            "Person",
            vec![
                make_field("name", Type::String),
                make_field("age", Type::Float),
            ],
        )
    });

    static COMPANY_TY: Lazy<Entity> = Lazy::new(|| {
        make_entity(
            "Company",
            vec![
                make_field("name", Type::String),
                make_field("ceo", PERSON_TY.clone().into()),
            ],
        )
    });
    static ENTITIES: Lazy<[Entity; 2]> = Lazy::new(|| [PERSON_TY.clone(), COMPANY_TY.clone()]);
    static TYPE_SYSTEM: Lazy<TypeSystem> = Lazy::new(|| make_type_system(&*ENTITIES));

    #[tokio::test]
    async fn test_query_plan() {
        let fetch_names = |qe: QueryEngine, op_chain: QueryOpChain| async move {
            let inner = ChiselRequestContext {
                api_version: VERSION.to_owned(),
                user_id: None,
                path: "".to_string(),
                headers: Default::default(),
                _method: "GET".into(),
            };
            let query_plan = QueryPlan::from_op_chain(
                &RequestContext {
                    policies: &Policies::default(),
                    ts: &make_type_system(&*ENTITIES),
                    inner,
                },
                op_chain,
            )
            .unwrap();
            let rows = fetch_rows_with_plan(&qe, query_plan).await;
            let names: Vec<_> = rows
                .iter()
                .map(|r| r["name"].as_str().unwrap().to_owned())
                .collect();
            names
        };

        let ppl = [
            json!({"name": "John", "age": json!(20f32)}),
            json!({"name": "Alan", "age": json!(30f32)}),
            json!({"name": "Max", "age": json!(40f32)}),
            json!({"name": "Kek", "age": json!(40f32)}),
        ];
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            for person in ppl {
                add_row(&qe, &PERSON_TY, &person, &TYPE_SYSTEM).await;
            }
            let make_sort_op = |keys: &[(&str, bool)]| {
                let keys = keys
                    .iter()
                    .map(|(name, asc)| SortKey {
                        field_name: name.to_string(),
                        ascending: *asc,
                    })
                    .collect();
                QueryOpChain::SortBy {
                    keys,
                    inner: QueryOpChain::BaseEntity {
                        name: "Person".to_owned(),
                    }
                    .into(),
                }
            };
            let ops = make_sort_op(&[("name", true)]);
            let names = fetch_names(qe.clone(), ops.clone()).await;
            assert_eq!(names, vec!["Alan", "John", "Kek", "Max"]);

            let ops = make_sort_op(&[("name", false)]);
            let names = fetch_names(qe.clone(), ops.clone()).await;
            assert_eq!(names, vec!["Max", "Kek", "John", "Alan"]);

            let ops = make_sort_op(&[("age", true), ("name", false)]);
            let names = fetch_names(qe.clone(), ops.clone()).await;
            assert_eq!(names, vec!["John", "Alan", "Max", "Kek"]);

            let ops = make_sort_op(&[("age", true), ("name", true)]);
            let names = fetch_names(qe.clone(), ops.clone()).await;
            assert_eq!(names, vec!["John", "Alan", "Kek", "Max"]);
        }
    }

    #[tokio::test]
    async fn test_delete_with_expr() {
        let delete_with_expr = |entity_name: &str, expr: Expr| {
            let inner = ChiselRequestContext {
                api_version: VERSION.to_owned(),
                user_id: None,
                path: "".to_string(),
                headers: Default::default(),
                _method: "GET".into(),
            };
            Mutation::delete_from_expr(
                &RequestContext {
                    policies: &Policies::default(),
                    ts: &make_type_system(&*ENTITIES),
                    inner,
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
            add_row(&qe, &PERSON_TY, &john, &TYPE_SYSTEM).await;

            let expr = binary(&["name"], BinaryOp::Eq, "John".into());
            let mutation = delete_with_expr("Person", expr);
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &PERSON_TY).await.len(), 0);
        }
        {
            let (qe, _db_file) = setup_clear_db(&*ENTITIES).await;
            add_row(&qe, &PERSON_TY, &john, &TYPE_SYSTEM).await;
            add_row(&qe, &PERSON_TY, &alan, &TYPE_SYSTEM).await;

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
            add_row(&qe, &COMPANY_TY, &chiselstrike, &TYPE_SYSTEM).await;

            let expr = binary(&["ceo", "name"], BinaryOp::Eq, "John".into());
            let mutation = delete_with_expr("Company", expr);
            qe.mutate(mutation).await.unwrap();

            assert_eq!(fetch_rows(&qe, &COMPANY_TY).await.len(), 0);
        }
    }
}
