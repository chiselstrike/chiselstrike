use std::sync::Arc;
use crate::layout;

/// Database connection.
#[derive(Debug)]
pub struct DataConn {
    pub layout: Arc<layout::Layout>,
    pub pool: sqlx::AnyPool,
}

impl DataConn {
    pub fn new(layout: Arc<layout::Layout>, pool: sqlx::AnyPool) -> Self {
        Self { layout, pool }
    }

    pub fn kind(&self) -> sqlx::any::AnyKind {
        self.pool.any_kind()
    }
}
