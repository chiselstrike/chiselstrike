use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use sqlx::{Any, Transaction};
use tokio::sync::oneshot;

use crate::datastore::engine::TransactionStatic;
use crate::datastore::DataContext;
use crate::http::HttpResponse;
use crate::policies::FieldPolicies;
use crate::types::ObjectType;
use crate::{policies::PolicySystem, types::TypeSystem};

/// RequestContext bears a mix of contextual variables used by QueryPlan
/// and Mutations.
pub struct RequestContext {
    /// Policies to be applied on the query.
    policy_system: Arc<PolicySystem>,
    /// Type system to be used of version `version_id`
    type_system: Arc<TypeSystem>,
    /// Version to be used.
    version_id: String,
    /// transaction ascociated with the current request.
    transaction: RefCell<Option<TransactionStatic>>,

    inner: RequestMeta,
}

impl deno_core::Resource for RequestContext {}

impl DataContext for RequestContext {
    #[inline]
    fn type_system(&self) -> &TypeSystem {
        self.type_system()
    }

    #[inline]
    fn policy_system(&self) -> &PolicySystem {
        self.policy_system()
    }

    #[inline]
    fn make_field_policies(&self, ty: &ObjectType) -> FieldPolicies {
        self.policy_system.make_field_policies(
            self.user_id(),
            self.request_path().unwrap_or(""),
            ty,
        )
    }

    #[inline]
    fn request_headers(&self) -> Option<&HashMap<String, String>> {
        self.http_request().map(|r| &r.headers)
    }

    fn request_path(&self) -> Option<&str> {
        self.request_path()
    }

    fn version_id(&self) -> &str {
        self.version_id()
    }
}

impl RequestContext {
    pub fn new(
        ts: Arc<TypeSystem>,
        ps: Arc<PolicySystem>,
        version_id: String,
        inner: RequestMeta,
    ) -> Self {
        Self {
            policy_system: ps,
            type_system: ts,
            version_id,
            inner,
            transaction: Default::default(),
        }
    }

    pub fn request_path(&self) -> Option<&str> {
        match self.inner {
            RequestMeta::HttpRequest(HttpRequest { ref path, .. }) => Some(path),
            RequestMeta::KafkaEvent => None,
        }
    }

    pub fn type_system(&self) -> &TypeSystem {
        &self.type_system
    }

    pub fn policy_system(&self) -> &PolicySystem {
        &self.policy_system
    }

    pub fn version_id(&self) -> &str {
        &self.version_id
    }

    pub fn transaction(&self) -> anyhow::Result<TransactionStatic> {
        self.transaction
            .borrow()
            .as_ref()
            .cloned()
            .context("no transaction in the current context.")
    }

    pub fn put_transaction(&self, txn: TransactionStatic) {
        self.transaction.borrow_mut().replace(txn);
    }

    pub fn take_transaction(&self) -> anyhow::Result<Option<Transaction<'static, Any>>> {
        let txn = self.transaction.borrow_mut().take();

        match txn {
            Some(txn) => {
                let txn = Arc::try_unwrap(txn)
                    .ok()
                    .context(
                        "Cannot commit a transaction because there is an operation \
                        in progress that uses this transaction",
                    )?
                    .into_inner();
                Ok(Some(txn))
            }
            None => Ok(None),
        }
    }

    fn user_id(&self) -> Option<&str> {
        match self.inner {
            RequestMeta::HttpRequest(HttpRequest { ref user_id, .. }) => user_id.as_deref(),
            RequestMeta::KafkaEvent => None,
        }
    }

    pub fn http_request(&self) -> Option<&HttpRequest> {
        match self.inner {
            RequestMeta::HttpRequest(ref r) => Some(r),
            RequestMeta::KafkaEvent => None,
        }
    }
}

pub struct HttpRequest {
    /// Id of user making the request.
    pub user_id: Option<String>,
    /// Current URL path from which this request originated.
    pub path: String,
    /// Current HTTP headers.
    pub headers: HashMap<String, String>,
    pub response_tx: RefCell<Option<oneshot::Sender<HttpResponse>>>,
}

pub enum RequestMeta {
    HttpRequest(HttpRequest),
    KafkaEvent,
}

impl From<HttpRequest> for RequestMeta {
    fn from(r: HttpRequest) -> Self {
        Self::HttpRequest(r)
    }
}
