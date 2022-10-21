use anyhow::{Result, Context, ensure, bail};
use deno_core::v8;
use guard::guard;
use sqlx::prelude::*;
use std::rc::Rc;
use crate::ctx::DataCtx;
use crate::util::reduce_args_lifetime;
use super::{Query, eval};

pub struct FetchStream {
    query: Rc<Query>,
    sql_args: Option<sqlx::any::AnyArguments<'static>>,
    rows: Option<Vec<sqlx::any::AnyRow>>,
    fetch_idx: usize,
}

impl FetchStream {
    pub fn start<'s>(
        query: Rc<Query>,
        scope: &mut v8::HandleScope<'s>,
        js_arg: v8::Local<'s, v8::Value>,
    ) -> Result<Self> {
        ensure!(query.output.is_some(), "cannot fetch using a query that does not have output");
        let sql_args = eval::eval_sql_args(&query, scope, js_arg)?;
        Ok(Self { query, sql_args: Some(sql_args), rows: None, fetch_idx: 0 })
    }

    pub async fn fetch(&mut self, ctx: &mut DataCtx) -> Result<bool> {
        if let Some(rows) = self.rows.as_ref() {
            self.fetch_idx += 1;
            Ok(self.fetch_idx < rows.len())
        } else {
            guard!{let Some(sql_args) = self.sql_args.take() else {
                bail!("fetch stream is in invalid state")
            }};

            let sql_stmt = ctx.txn.prepare(self.query.sql_text.as_str()).await
                .with_context(|| {
                    if cfg!(debug_assertions) {
                        format!("could not prepare SQL statement {:?}", self.query.sql_text)
                    } else {
                        "could not prepare SQL statement".into()
                    }
                })?
                .to_owned();
            let sql_args = unsafe { reduce_args_lifetime(sql_args) };
            let sql_query = sql_stmt.query_with(sql_args);
            let rows = ctx.txn.fetch_all(sql_query).await
                .context("could not perform SQL query")?;

            let row_count = rows.len();
            self.rows = Some(rows);
            Ok(row_count > 0)
        }
    }

    pub fn read<'s>(&self, scope: &mut v8::HandleScope<'s>) -> Result<v8::Local<'s, v8::Value>> {
        guard!{let Some(rows) = self.rows.as_ref() else {
            bail!("cannot read row from a fetch stream that has not been fetched")
        }};
        guard!{let Some(row) = rows.get(self.fetch_idx) else {
            bail!("trying to read row after end of stream was reached")
        }};
        eval::eval_output_expr(
            &self.query.schema, self.query.output.as_ref().unwrap(),
            scope, row,
        ).context("could not evaluate JS output from SQL row")
    }
}

pub struct ExecuteFuture {
    query: Rc<Query>,
    sql_args: Option<sqlx::any::AnyArguments<'static>>,
    result: Option<sqlx::any::AnyQueryResult>,
}

impl ExecuteFuture {
    pub fn start<'s>(
        query: Rc<Query>,
        scope: &mut v8::HandleScope<'s>,
        js_arg: v8::Local<'s, v8::Value>,
    ) -> Result<Self> {
        let sql_args = eval::eval_sql_args(&query, scope, js_arg)?;
        Ok(Self { query, sql_args: Some(sql_args), result: None })
    }

    pub async fn execute(&mut self, ctx: &mut DataCtx) -> Result<()> {
        guard!{let Some(sql_args) = self.sql_args.take() else {
            bail!("execute future has already been executed")
        }};

        let sql_stmt = ctx.txn.prepare(self.query.sql_text.as_str()).await
            .with_context(|| {
                if cfg!(debug_assertions) {
                    format!("could not prepare SQL statement {:?}", self.query.sql_text)
                } else {
                    "could not prepare SQL statement".into()
                }
            })?
            .to_owned();
        let sql_args = unsafe { reduce_args_lifetime(sql_args) };
        let sql_query = sql_stmt.query_with(sql_args);
        let result = ctx.txn.execute(sql_query).await
            .context("could not execute SQL statement")?;

        self.result = Some(result);
        Ok(())
    }

    pub fn rows_affected(&self) -> Result<u64> {
        match self.result.as_ref() {
            Some(result) => Ok(result.rows_affected()),
            None => bail!("the execute future has not (yet) terminated successfully"),
        }
    }
}
