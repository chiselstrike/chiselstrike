use anyhow::{Result, Context};
use std::sync::Arc;
use crate::conn::DataConn;
use crate::layout;

pub struct DataCtx {
    pub layout: Arc<layout::Layout>,
    pub txn: sqlx::Transaction<'static, sqlx::Any>,
}

impl DataCtx {
    pub async fn begin(conn: &DataConn) -> Result<DataCtx> {
        let txn = conn.pool.begin().await
            .context("could not begin an SQL transaction")?;
        Ok(Self { layout: conn.layout.clone(), txn })
    }

    pub async fn commit(self) -> Result<()> {
        self.txn.commit().await.context("could not commit SQL transaction")
    }

    pub async fn rollback(self) -> Result<()> {
        self.txn.rollback().await.context("could not rollback SQL transaction")
    }
}
