// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use structopt_toml::StructOptToml;

#[derive(StructOpt, Debug, Clone, StructOptToml, Deserialize, Serialize)]
#[structopt(name = "chiseld", version = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT"))]
#[serde(deny_unknown_fields, default)]
pub struct Opt {
    /// User-visible HTTP API server listen address.
    #[structopt(short, long, default_value = "localhost:8080")]
    pub api_listen_addr: String,
    /// RPC server listen address.
    #[structopt(short, long, default_value = "127.0.0.1:50051")]
    pub rpc_listen_addr: SocketAddr,
    /// Internal routes (for k8s) listen address
    #[structopt(short, long, default_value = "127.0.0.1:9090")]
    pub internal_routes_listen_addr: SocketAddr,

    /// Metadata database URI. [deprecated: use --db-uri instead]
    #[structopt(short, long, default_value = "sqlite://chiseld.db?mode=rwc")]
    pub _metadata_db_uri: String,
    /// Data database URI. [deprecated: use --db-uri instead]
    #[structopt(short, long, default_value = "sqlite://chiseld-data.db?mode=rwc")]
    pub _data_db_uri: String,
    /// Database URI.
    #[structopt(long, default_value = "sqlite://.chiseld.db?mode=rwc")]
    pub db_uri: String,
    /// Kafka connection.
    #[structopt(long)]
    pub kafka_connection: Option<String>,
    /// Kafka topics to subscribe to.
    #[structopt(long)]
    pub kafka_topics: Vec<String>,

    /// Activate inspector and let a debugger attach at any time.
    #[structopt(long)]
    pub inspect: bool,
    /// Activate inspector, but pause the runtime at startup to wait for a debugger to attach.
    #[structopt(long)]
    pub inspect_brk: bool,
    /// Activate debug mode, it will show runtime exceptions in HTTP responses.
    #[structopt(long)]
    pub debug: bool,
    /// size of database connection pool.
    #[structopt(short, long, default_value = "10")]
    pub nr_connections: usize,
    /// How many worker threads to create for every version.
    /// (The `executor_threads` alias is DEPRECATED)
    #[structopt(short, long, default_value = "1", alias = "executor-threads")]
    pub worker_threads: usize,
    /// V8 flags.
    #[structopt(long)]
    pub v8_flags: Vec<String>,
    /// Read default configuration from this toml configuration file
    #[structopt(long, short)]
    #[serde(skip)]
    pub config: Option<PathBuf>,

    #[structopt(long, env = "CHISEL_SECRET_KEY_LOCATION")]
    pub chisel_secret_key_location: Option<String>,

    #[structopt(long, env = "CHISEL_SECRET_LOCATION")]
    pub chisel_secret_location: Option<String>,

    /// Sets secrets polling period in seconds (can be float).
    #[structopt(long, default_value = "1")]
    pub secrets_polling_period_s: f32,

    /// Prints the configuration resulting from the merging of all the configuration sources,
    /// including default values, in the JSON format.
    /// This is the configuration that will be used when starting chiseld.
    #[structopt(long)]
    #[serde(skip)]
    pub show_config: bool,
}

impl Opt {
    pub async fn from_file(path: &Path) -> Result<Self> {
        let content = tokio::fs::read(path).await?;
        let content = std::str::from_utf8(&content)?;

        Self::from_args_with_toml(content).map_err(|e| anyhow!(e.to_string()))
    }
}
