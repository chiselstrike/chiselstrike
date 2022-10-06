use anyhow::{Result, Context};
use chisel_snapshot::schema;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use crate::layout;
use crate::sql_writer::SqlWriter;

pub struct DataCtx {
    pub(crate) layout: Arc<layout::Layout>,
    pub(crate) txn: sqlx::Transaction<'static, sqlx::Any>,
    pub(crate) find_by_id_cache: HashMap<schema::EntityName, Rc<sqlx::any::AnyStatement<'static>>>,
    pub(crate) store_with_id_cache: HashMap<schema::EntityName, Rc<sqlx::any::AnyStatement<'static>>>,
}

impl DataCtx {
    pub(crate) fn sql_writer(&self) -> SqlWriter {
        SqlWriter::new(self.txn.kind())
    }

    pub(crate) fn entity_table(&self, name: &schema::EntityName) -> Result<Arc<layout::EntityTable>> {
        self.layout.entity_tables.get(name).cloned()
            .context("could not find entity with given name")
    }
}
