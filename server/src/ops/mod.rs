// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::version::VersionInfo;
use crate::worker::WorkerState;
use anyhow::{Result, bail};
use deno_core::{v8, serde_v8};

mod datastore;
mod request;

pub fn extension() -> deno_core::Extension {
    deno_core::Extension::builder()
        .ops(vec![
            op_chisel_ready::decl(),
            op_chisel_get_secret::decl(),
            op_chisel_get_version_id::decl(),
            op_chisel_get_version_info::decl(),
            op_format_file_name::decl(),
            datastore::op_chisel_begin_transaction::decl(),
            datastore::op_chisel_commit_transaction::decl(),
            datastore::op_chisel_rollback_transaction::decl(),
            datastore::op_chisel_store::decl(),
            datastore::op_chisel_delete::decl(),
            datastore::op_chisel_crud_delete::decl(),
            datastore::op_chisel_crud_query::decl(),
            datastore::op_chisel_relational_query_create::decl(),
            datastore::op_chisel_query_next::decl(),
            request::op_chisel_accept::decl(),
            request::op_chisel_respond::decl(),
        ])
        .build()
}

#[deno_core::op]
fn op_chisel_ready(state: &mut deno_core::OpState) -> Result<()> {
    if let Some(ready_tx) = state.borrow_mut::<WorkerState>().ready_tx.take() {
        let _: Result<_, _> = ready_tx.send(());
        Ok(())
    } else {
        bail!("op_chisel_ready has already been called")
    }
}

#[deno_core::op(v8)]
fn op_chisel_get_secret<'a>(
    scope: &mut v8::HandleScope<'a>,
    state: &mut deno_core::OpState,
    key: String,
) -> serde_v8::Value<'a> {
    let secrets = state.borrow::<WorkerState>().server.secrets.read();
    match secrets.get(&key).cloned() {
        Some(v) => {
            let v = serde_v8::to_v8(scope, v).unwrap();
            serde_v8::from_v8(scope, v).unwrap()
        },
        None => {
            // this is necessary to return undefined
            // https://github.com/denoland/deno/issues/14779
            let u = v8::undefined(scope);
            serde_v8::from_v8(scope, u.into()).unwrap()
        },
    }
}

#[deno_core::op]
fn op_chisel_get_version_id(state: &mut deno_core::OpState) -> String {
    state.borrow::<WorkerState>().version.version_id.clone()
}

#[deno_core::op]
fn op_chisel_get_version_info(state: &mut deno_core::OpState) -> VersionInfo {
    state.borrow::<WorkerState>().version.info.clone()
}


// Used by deno to format names in errors
#[deno_core::op]
fn op_format_file_name(file_name: String) -> Result<String> {
    Ok(file_name)
}

