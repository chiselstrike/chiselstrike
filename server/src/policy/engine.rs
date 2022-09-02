use std::cell::RefCell;

use anyhow::Result;
use chiselc::policies::PolicyEvalContext;

use super::store::Store;
use crate::datastore::expr::Expr;
use crate::deno::ChiselRequestContext;

#[derive(Default)]
pub struct PolicyEngine {
    eval_context: RefCell<PolicyEvalContext>,
    store: Store,
}

impl PolicyEngine {
    pub fn new(store: Store) -> Self {
        Self {
            eval_context: Default::default(),
            store,
        }
    }

    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    /// Returns the filters to be applied to the DB query for reading the entity, if any.
    pub fn read_fitlers(
        &self,
        ty_name: &str,
        version: &str,
        ctx: &ChiselRequestContext,
    ) -> Result<Option<Expr>> {
        let mut eval_ctx = self.eval_context.borrow_mut();
        self.store
            .get_policy(version, ty_name)
            .and_then(|p| p.compute_read_filter(ctx, &mut eval_ctx).transpose())
            .transpose()
    }
}
