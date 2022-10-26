use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use tokio::sync::oneshot;

use crate::datastore::DataContext;
use crate::http::HttpResponse;
use crate::policy::engine::ChiselRequestContext;

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

impl ChiselRequestContext for JobInfo {
    fn method(&self) -> &str {
        match self {
            JobInfo::HttpRequest { ref method, .. } => method,
            JobInfo::KafkaEvent => todo!(),
        }
    }

    fn path(&self) -> &str {
        match self {
            JobInfo::HttpRequest { ref path, .. } => path,
            JobInfo::KafkaEvent => todo!(),
        }
    }

    fn headers(&self) -> Box<dyn Iterator<Item = (&str, &str)> + '_> {
        match self {
            JobInfo::HttpRequest { ref headers, .. } => {
                Box::new(headers.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            }
            JobInfo::KafkaEvent => todo!(),
        }
    }

    fn user_id(&self) -> Option<&str> {
        match self {
            JobInfo::HttpRequest { ref user_id, .. } => user_id.as_deref(),
            JobInfo::KafkaEvent => todo!(),
        }
    }
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
    pub current_data_ctx: RefCell<Option<Rc<DataContext>>>,
}

impl JobContext {
    /// Attempts to return a reference to the current data context.
    pub fn data_context(&self) -> anyhow::Result<Rc<DataContext>> {
        let ctx = self.current_data_ctx.borrow();
        match &*ctx {
            Some(ctx) => Ok(ctx.clone()),
            None => anyhow::bail!("No transaction in the current context"),
        }
    }
}

impl deno_core::Resource for JobContext {}
