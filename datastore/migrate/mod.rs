use anyhow::{Result, Context};
use chisel_snapshot::schema;
use std::sync::Arc;
use crate::layout;

mod exec;
mod plan;
mod repr;

pub use self::plan::PlanOpts;

pub async fn migrate(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    old_layout: &layout::Layout,
    new_schema: Arc<schema::Schema>,
    plan_opts: &PlanOpts,
) -> Result<layout::Layout> {
    let plan = plan::plan_migration(old_layout, new_schema, plan_opts)
        .context("could not plan migration")?;
    for step in plan.steps.iter() {
        exec::exec_migration_step(txn, step).await?;
    }
    Ok(plan.new_layout)
}
