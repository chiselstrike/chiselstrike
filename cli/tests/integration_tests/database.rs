// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use crate::framework::execute;
use rand::{distributions::Alphanumeric, Rng};
use std::process::Command;
use std::sync::Arc;
use tempdir::TempDir;

#[derive(Debug, Clone)]
pub enum DatabaseConfig {
    Postgres(PostgresConfig),
    Sqlite,
    LegacySplitSqlite,
}

#[derive(Debug, Clone)]
pub struct PostgresConfig {
    host: String,
    user: Option<String>,
    password: Option<String>,
    db_name: String,
}

impl PostgresConfig {
    pub fn new(host: String, user: Option<String>, password: Option<String>) -> PostgresConfig {
        let db_id = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(40)
            .map(char::from)
            .collect::<String>()
            .to_lowercase();
        let db_name = format!("datadb_{db_id}");
        PostgresConfig {
            host,
            user,
            password,
            db_name,
        }
    }

    fn url_prefix(&self) -> url::Url {
        let user = self.user.clone().unwrap_or_else(whoami::username);
        let mut url_prefix = "postgres://".to_string();
        url_prefix.push_str(&user);
        if let Some(password) = &self.password {
            url_prefix.push(':');
            url_prefix.push_str(password);
        }
        url_prefix.push('@');
        url_prefix.push_str(&self.host);

        url::Url::parse(&url_prefix).expect("failed to generate postgres db url")
    }
}

pub enum Database {
    Postgres(PostgresDb),
    Sqlite(SqliteDb),
    LegacySplitSqlite { meta: SqliteDb, data: SqliteDb },
}

pub struct PostgresDb {
    config: PostgresConfig,
}

impl PostgresDb {
    pub fn new(config: PostgresConfig) -> Self {
        execute(Command::new("psql").args([
            config.url_prefix().as_str(),
            "-c",
            format!("CREATE DATABASE {}", &config.db_name).as_str(),
        ]))
        .expect("failed to create testing Postgres database");
        Self { config }
    }

    pub fn url(&self) -> String {
        self.config
            .url_prefix()
            .join(&self.config.db_name)
            .unwrap()
            .as_str()
            .to_string()
    }
}

impl Drop for PostgresDb {
    fn drop(&mut self) {
        execute(Command::new("psql").args([
            self.config.url_prefix().as_str(),
            "-c",
            format!("DROP DATABASE {}", &self.config.db_name).as_str(),
        ]))
        .expect("failed to drop test database on cleanup");
    }
}

pub struct SqliteDb {
    pub tmp_dir: Arc<TempDir>,
    pub file_name: String,
}

impl SqliteDb {
    pub fn new(tmp_dir: Arc<TempDir>, file_name: &str) -> Self {
        Self { tmp_dir, file_name: file_name.into() }
    }

    pub fn url(&self) -> String {
        let path = self.tmp_dir.path().join(&self.file_name);
        format!("sqlite://{}?mode=rwc", path.display())
    }
}
