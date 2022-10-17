use anyhow::{Result, Context as _, anyhow, bail};
use chisel_snapshot::schema;
use deno_core::v8;
use std::borrow::Borrow;
use crate::{encode_v8, decode_v8};
use super::{Query, InputParam, InputExpr, OutputExpr};

pub fn eval_sql_args<'s, 'a>(
    query: &Query,
    scope: &mut v8::HandleScope<'s>,
    js_arg: v8::Local<'s, v8::Value>,
) -> Result<sqlx::any::AnyArguments<'a>> {
    let mut sql_args = sqlx::any::AnyArguments::default();
    for input_param in query.inputs.iter() {
        encode_input_arg(&query.schema, input_param, scope, js_arg, &mut sql_args)
            .context("could not encode SQL argument from JS input")?;
    }
    Ok(sql_args)
}

fn encode_input_arg<'s>(
    schema: &schema::Schema,
    param: &InputParam,
    scope: &mut v8::HandleScope<'s>,
    js_arg: v8::Local<'s, v8::Value>,
    out_args: &mut sqlx::any::AnyArguments,
) -> Result<()> {
    match param {
        InputParam::Id(repr, input_expr) => {
            let js_value = eval_input_expr(schema, input_expr, scope, js_arg)
                .context("could not evaluate id input")?;
            encode_v8::encode_id_to_sql(*repr, scope, js_value, out_args)
        },
        InputParam::Field(repr, type_, input_expr) => {
            let js_value = eval_input_expr(schema, input_expr, scope, js_arg)
                .context("could not evaluate field input")?;
            encode_v8::encode_field_to_sql(schema, *repr, type_, scope, js_value, out_args)
        },
    }
}

fn eval_input_expr<'s>(
    schema: &schema::Schema,
    expr: &InputExpr,
    scope: &mut v8::HandleScope<'s>,
    arg: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Value>> {
    match expr {
        InputExpr::Arg => Ok(arg),
        InputExpr::Get(obj_expr, key_global) => {
            let get_key_str = |scope: &mut v8::HandleScope<'_>| {
                let key: &v8::String = key_global.borrow();
                key.to_rust_string_lossy(scope)
            };
            let obj = eval_input_expr(schema, obj_expr, scope, arg)?;
            let obj = v8::Local::<v8::Object>::try_from(obj)
                .map_err(|_| anyhow!("expected a JS object (to get property {:?})", get_key_str(scope)))?;
            let key = v8::Local::new(scope, key_global);
            match obj.get(scope, key.into()) {
                Some(value) => Ok(value),
                None => bail!("missing property {:?} of a JS object", get_key_str(scope)),
            }
        },
    }
}

pub fn eval_output_expr<'s>(
    schema: &schema::Schema,
    expr: &OutputExpr,
    out_scope: &mut v8::HandleScope<'s>,
    row: &sqlx::any::AnyRow,
) -> Result<v8::Local<'s, v8::Value>> {
    match expr {
        OutputExpr::Object(properties) => {
            let scope = &mut v8::EscapableHandleScope::new(out_scope);
            let obj = v8::Object::new(scope);
            for (key_global, value_expr) in properties.iter() {
                let value = eval_output_expr(schema, value_expr, scope, row)?;
                let key = v8::Local::new(scope, key_global);
                obj.set(scope, key.into(), value).unwrap();
            }
            Ok(scope.escape(obj).into())
        },
        OutputExpr::Id(repr, row_idx) =>
            decode_v8::decode_id_from_sql(*repr, out_scope, row, *row_idx),
        OutputExpr::Field(repr, type_, row_idx) =>
            decode_v8::decode_field_from_sql(schema, *repr, type_, out_scope, row, *row_idx),
    }
}
