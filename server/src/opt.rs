// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use structopt_toml::StructOptToml;

#[derive(StructOpt, Debug, Clone, StructOptToml, Deserialize, Serialize)]
#[structopt(name = "chiseld", version = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT"))]
#[serde(deny_unknown_fields, default)]
pub struct Opt {
    /// user-visible API server listen address.
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
    /// Should we wait for a debugger before executing any JS?
    #[structopt(long)]
    pub inspect_brk: bool,
    /// size of database connection pool.
    #[structopt(short, long, default_value = "10")]
    pub nr_connections: usize,
    /// How many executor threads to create
    #[structopt(short, long, default_value = "1")]
    pub executor_threads: usize,
    /// If on, serve a web UI on an internal route.
    #[structopt(long)]
    pub webui: bool,
    /// Read default configuration from this toml configuration file
    #[structopt(long, short)]
    #[serde(skip)]
    pub config: Option<PathBuf>,

    #[structopt(long, env = "CHISEL_SECRET_KEY_LOCATION")]
    pub chisel_secret_key_location: Option<String>,

    #[structopt(long, env = "CHISEL_SECRET_LOCATION")]
    pub chisel_secret_location: Option<String>,

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

