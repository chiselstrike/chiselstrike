use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use tokio::sync::oneshot;

use crate::datastore::DataContext;
use crate::http::HttpResponse;

pub enum JobInfo {
    HttpRequest {
        method: String,
        path: String,
        headers: HashMap<String, String>,
        user_id: Option<String>,
        response_tx: RefCell<Option<oneshot::Sender<HttpResponse>>>,
    },
    KafkaEvent,
}

impl JobInfo {
    pub fn path(&self) -> Option<&str> {
        match self {
            JobInfo::HttpRequest { ref path, .. } => Some(path),
            JobInfo::KafkaEvent => None,
        }
    }

    pub fn request_headers(&self) -> Option<&HashMap<String, String>> {
        match self {
            JobInfo::HttpRequest { ref headers, .. } => Some(headers),
            JobInfo::KafkaEvent => None,
        }
    }

    pub fn user_id(&self) -> Option<&str> {
        match self {
            JobInfo::HttpRequest { ref user_id, .. } => user_id.as_deref(),
            JobInfo::KafkaEvent => None,
        }
    }
}

pub struct JobContext {
    pub job_info: Rc<JobInfo>,
    pub current_data_ctx: RefCell<Option<DataContext>>,
}

impl JobContext {
    /// Attempts to return a reference to the current data context.
    pub fn data_context(&self) -> anyhow::Result<Ref<DataContext>> {
        let ctx = self.current_data_ctx.borrow();
        anyhow::ensure!(ctx.is_some(), "No transaction in the current context");
        Ok(Ref::map(ctx, |ctx| ctx.as_ref().unwrap()))
    }
}

impl deno_core::Resource for JobContext {}
