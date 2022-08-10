use anyhow::Result;
use bytes::BytesMut;
use checked_command::CheckedCommand;
use rand::{distributions::Alphanumeric, Rng};
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, net::SocketAddr};
use tempdir::TempDir;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug)]
pub enum OpMode {
    Deno,
    Node,
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
}

#[derive(Clone, Debug)]
pub struct ChiseldConfig {
    pub public_address: SocketAddr,
    pub internal_address: SocketAddr,
    pub rpc_address: SocketAddr,
}

impl PostgresConfig {
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

#[derive(Debug, Clone)]
pub enum DatabaseConfig {
    Postgres(PostgresConfig),
    Sqlite,
}

pub enum Database {
    Postgres(PostgresDb),
    Sqlite(SqliteDb),
}

impl Database {
    fn url(&self) -> Result<String> {
        match self {
            Database::Postgres(db) => db.url(),
            Database::Sqlite(db) => db.url(),
        }
    }
}

pub struct PostgresDb {
    config: PostgresConfig,
}

impl Drop for PostgresDb {
    fn drop(&mut self) {
        CheckedCommand::new("psql")
            .args([
                self.config.url_prefix().as_str(),
                "-c",
                format!("DROP DATABASE {}", &self.config.db_name).as_str(),
            ])
            .execute()
            .expect("failed to drop test database on cleanup");
    }
}

impl PostgresDb {
    fn new(config: PostgresConfig) -> Self {
        CheckedCommand::new("psql")
            .args([
                config.url_prefix().as_str(),
                "-c",
                format!("CREATE DATABASE {}", &config.db_name).as_str(),
            ])
            .execute()
            .expect("failed to create testing Postgres database");
        Self { config }
    }

    fn url(&self) -> Result<String> {
        Ok(self
            .config
            .url_prefix()
            .join(&self.config.db_name)?
            .as_str()
            .to_string())
    }
}

pub struct SqliteDb {
    tmp_dir: Arc<TempDir>,
}

impl SqliteDb {
    fn url(&self) -> Result<String> {
        let path = self.tmp_dir.path().join("chiseld.db");
        Ok(format!("sqlite://{}?mode=rwc", path.display()))
    }
}

pub struct GuardedChild {
    child: tokio::process::Child,
    pub stdout: AsyncTestableOutput,
    pub stderr: AsyncTestableOutput,
}

impl GuardedChild {
    fn new(command: &mut tokio::process::Command) -> Self {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("failed to spawn GuardedChild");

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        Self {
            child,
            stdout: AsyncTestableOutput::new(OutputType::Stdout, Box::pin(stdout)),
            stderr: AsyncTestableOutput::new(OutputType::Stderr, Box::pin(stderr)),
        }
    }

    async fn wait(&mut self) -> Result<ExitStatus> {
        Ok(self.child.wait().await?)
    }

    /// Prints both stdout and stderr to standard output.
    pub async fn show_output(&mut self) {
        self.stdout.show().await.unwrap();
        self.stderr.show().await.unwrap();
    }
}

trait ExecutableExt {
    fn execute(&mut self) -> Result<MixedTestableOutput, ProcessError>;
}

impl ExecutableExt for checked_command::CheckedCommand {
    /// Executes the command while mapping the output to MixedTestableOutput and
    /// error into ProcessError for easier manipulation and debugging.
    fn execute(&mut self) -> Result<MixedTestableOutput, ProcessError> {
        self.output()
            .map(|output| MixedTestableOutput::from_output(&output).unwrap())
            .map_err(|e| e.into())
    }
}

pub struct ProcessError {
    output: MixedTestableOutput,
}

impl From<checked_command::Error> for ProcessError {
    fn from(e: checked_command::Error) -> Self {
        if let checked_command::Error::Failure(_, Some(output)) = e {
            Self {
                output: MixedTestableOutput::from_output(&output).unwrap(),
            }
        } else {
            Self {
                output: MixedTestableOutput {
                    stdout: TestableOutput::new(OutputType::Stdout, &vec![]).unwrap(),
                    stderr: TestableOutput::new(OutputType::Stderr, &vec![]).unwrap(),
                },
            }
        }
    }
}

impl std::fmt::Debug for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "ProcessError:\nSTDOUT:\n{}\nSTDERR:\n{}",
            textwrap::indent(&self.stdout().output, "    "),
            textwrap::indent(&self.stderr().output, "    ")
        )
    }
}

impl ProcessError {
    #[allow(dead_code)]
    pub fn stdout(&self) -> TestableOutput {
        self.output.stdout.clone()
    }

    pub fn stderr(&self) -> TestableOutput {
        self.output.stderr.clone()
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum OutputType {
    Stdout,
    Stderr,
}

impl OutputType {
    fn to_str(&self) -> &str {
        match self {
            OutputType::Stdout => "stdout",
            OutputType::Stderr => "stderr",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MixedTestableOutput {
    pub stdout: TestableOutput,
    pub stderr: TestableOutput,
}

impl MixedTestableOutput {
    fn from_output(output: &checked_command::Output) -> Result<Self> {
        Ok(Self {
            stdout: TestableOutput::new(OutputType::Stdout, &output.stdout)?,
            stderr: TestableOutput::new(OutputType::Stderr, &output.stderr)?,
        })
    }

    fn from_std_output(output: &std::process::Output) -> Result<Self> {
        Ok(Self {
            stdout: TestableOutput::new(OutputType::Stdout, &output.stdout)?,
            stderr: TestableOutput::new(OutputType::Stderr, &output.stderr)?,
        })
    }

    /// Prints both stdout and stderr to standard output.
    #[allow(dead_code)]
    pub fn show_output(&self) {
        self.stdout.show();
        self.stderr.show();
    }
}

#[derive(Debug, Clone)]
pub struct TestableOutput {
    pub output_type: OutputType,
    pub output: String,
    cursor: usize,
}

impl TestableOutput {
    fn new(output_type: OutputType, raw_output: &[u8]) -> Result<Self> {
        let colorless_output = strip_ansi_escapes::strip(raw_output)?;
        let output = String::from_utf8(colorless_output)?;
        Ok(Self {
            output_type,
            output,
            cursor: 0,
        })
    }

    /// Tries to find `pattern` in the output starting from the last successfully read
    /// position (cursor). If given `pattern` is found, the function will store the
    /// position of the end of its first occurrence by updating the cursor. If not found,
    /// the function will panic.
    pub fn read(&mut self, pattern: &str) -> &mut Self {
        if let Some(idx) = self.output[self.cursor..].find(pattern) {
            self.cursor = idx + pattern.len();
            self
        } else {
            let out_type = self.output_type.to_str();
            let output = &self.output;
            panic!("failed to find text in the {out_type}: {pattern:?}\nFull output:\n{output}");
        }
    }

    /// Tries to find `pattern` in the output starting from the last successfully read
    /// position (cursor). If not found, the function will panic.
    pub fn peek(self, pattern: &str) -> Self {
        if !self.output[self.cursor..].contains(pattern) {
            let out_type = self.output_type.to_str();
            let output = &self.output;
            panic!("failed to find text in the {out_type}: {pattern:?}\nFull output:\n{output}");
        }
        self
    }

    #[allow(dead_code)]
    pub fn show(&self) {
        println!("{}", self.output);
    }
}

pub struct AsyncTestableOutput {
    #[allow(dead_code)]
    pub output_type: OutputType,
    async_output: Pin<Box<dyn AsyncRead + Send>>,
    pub raw_output: BytesMut,
    cursor: usize,
}

impl AsyncTestableOutput {
    fn new(output_type: OutputType, async_output: Pin<Box<dyn AsyncRead + Send>>) -> Self {
        Self {
            output_type,
            async_output,
            raw_output: BytesMut::new(),
            cursor: 0,
        }
    }

    /// Tries to find `pattern` in the output starting from the last successfully read
    /// position (cursor). If given `pattern` is found, the function will store the
    /// position of the end of its first occurrence by updating the cursor. If the pattern
    /// is not found until given `timeout` expires, the function will panic.
    pub async fn read_with_timeout(&mut self, pattern: &str, timeout: Duration) {
        let pattern = pattern.to_string();
        let checking_fut = async {
            loop {
                if self.internal_read(&pattern) {
                    break;
                }
                self.async_output
                    .read_buf(&mut self.raw_output)
                    .await
                    .unwrap();
            }
        };
        let r = tokio::time::timeout(timeout, checking_fut).await;
        if r.is_err() {
            let out_type = self.output_type.to_str();
            let output = self.decoded_output();
            panic!("failed to find text before timeout in the {out_type}: {pattern}\nFull output:\n{output}")
        }
    }

    /// Tries to find `pattern` in the output starting from the last successfully read
    /// position (cursor). If given `pattern` is found, the function will store the
    /// position of the end of its first occurrence by updating the cursor. If the pattern
    /// is not found until 1s timeout expires, the function will panic.
    pub async fn read(&mut self, pattern: &str) {
        self.read_with_timeout(pattern, Duration::from_secs(1))
            .await
    }

    fn internal_read(&mut self, pattern: &str) -> bool {
        let output = self.decoded_output();
        if let Some(idx) = output[self.cursor..].find(pattern) {
            self.cursor = idx + pattern.len();
            true
        } else {
            false
        }
    }

    pub fn decoded_output(&self) -> String {
        let colorless_output = strip_ansi_escapes::strip(&self.raw_output).unwrap();
        String::from_utf8(colorless_output).unwrap()
    }

    #[allow(dead_code)]
    pub async fn load_to_buffer(&mut self, timeout: core::time::Duration) {
        let _ = tokio::time::timeout(timeout, async {
            loop {
                self.async_output
                    .read_buf(&mut self.raw_output)
                    .await
                    .unwrap();
            }
        })
        .await;
    }

    /// Prints all of the output so far onto stdout.
    #[allow(dead_code)]
    pub async fn show(&mut self) -> Result<()> {
        self.load_to_buffer(Duration::from_secs(1)).await;

        let mut stdout = tokio::io::stdout();
        stdout.write_all(&self.raw_output).await?;
        stdout.flush().await?;
        Ok(())
    }
}

pub struct Chisel {
    config: ChiseldConfig,
    tmp_dir: Arc<TempDir>,
    client: reqwest::Client,
}

impl Chisel {
    async fn exec(&self, cmd: &str, args: &[&str]) -> Result<MixedTestableOutput, ProcessError> {
        let rpc_url = format!("http://{}", self.config.rpc_address);
        let chisel_path = bin_dir().join("chisel").to_str().unwrap().to_string();
        let args = [&["--rpc-addr", &rpc_url, cmd], args].concat();

        let output = tokio::process::Command::new(chisel_path)
            .args(args)
            .current_dir(&*self.tmp_dir)
            .output()
            .await
            .expect(&format!("could not execute `chisel {}`", cmd));

        let mto = MixedTestableOutput::from_std_output(&output).unwrap();
        if output.status.success() {
            Ok(mto)
        } else {
            Err(ProcessError { output: mto })
        }
    }

    /// Runs chisel apply
    pub async fn apply(&self) -> Result<MixedTestableOutput, ProcessError> {
        self.exec("apply", &[]).await
    }

    /// Runs chisel wait awaiting the readiness of chiseld service
    pub async fn wait(&self) -> Result<MixedTestableOutput, ProcessError> {
        self.exec("wait", &[]).await
    }

    /// Writes given text (probably code) into a file on given relative `path`
    /// in ChiselStrike project.
    pub fn write(&self, path: &str, data: &str) {
        let full_path = self.tmp_dir.path().join(path);
        fs::write(full_path, data).expect(&format!("Unable to write to {:?}", path));
    }

    /// Copies given `file` to a relative directory path `to` inside ChiselStrike project.
    pub fn copy_to_dir<P, Q>(&self, from: P, to: Q) -> u64
    where
        P: AsRef<std::path::Path> + std::fmt::Debug,
        Q: AsRef<std::path::Path> + std::fmt::Debug,
    {
        let options = fs_extra::dir::CopyOptions {
            copy_inside: true,
            ..Default::default()
        };
        fs_extra::copy_items(&[&from], self.tmp_dir.path().join(&to), &options)
            .unwrap_or_else(|_| panic!("failed to copy '{:?}' to '{:?}'", from, to))
    }

    /// Copies given `file` to a relative path `to` inside ChiselStrike project.
    pub fn copy_and_rename<P, Q>(&self, from: P, to: Q) -> u64
    where
        P: AsRef<std::path::Path> + std::fmt::Debug,
        Q: AsRef<std::path::Path> + std::fmt::Debug,
    {
        std::fs::copy(&from, self.tmp_dir.path().join(&to))
            .unwrap_or_else(|_| panic!("failed to copy '{:?}' to '{:?}'", from, to))
    }

    /// Posts given `data` to an `url` of a running ChielStrike service.
    pub async fn post(&self, url: &str, data: serde_json::Value) -> Result<reqwest::Response> {
        let url = url::Url::parse(&format!("http://{}", self.config.public_address))
            .unwrap()
            .join(url)
            .unwrap();
        let resp = self
            .client
            .post(url.as_str())
            .body(data.to_string())
            .timeout(core::time::Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?;
        Ok(resp)
    }

    /// Posts given `data` to an `url` of a running ChielStrike service and unwraps the
    /// response as text.
    pub async fn post_text(&self, url: &str, data: serde_json::Value) -> String {
        self.post(url, data).await.unwrap().text().await.unwrap()
    }
}

fn bin_dir() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path
}

fn repo_dir() -> std::path::PathBuf {
    let mut path = bin_dir();
    path.pop();
    path.pop();
    path
}

fn chiseld() -> String {
    bin_dir().join("chiseld").to_str().unwrap().to_string()
}

async fn setup_chiseld(config: &TestConfig) -> Result<(Database, GuardedChild, Chisel)> {
    let tmp_dir = Arc::new(TempDir::new("chiseld_test")?);

    let chisel = Chisel {
        config: config.chiseld_config.clone(),
        tmp_dir: tmp_dir.clone(),
        client: reqwest::Client::new(),
    };

    let optimize_str = format!("{}", config.optimize);

    match config.mode {
        OpMode::Deno => {
            chisel
                .exec(
                    "init",
                    &[
                        "--no-examples",
                        "--optimize",
                        &optimize_str,
                        "--auto-index",
                        &optimize_str,
                    ],
                )
                .await
                .expect("chisel init failed");
        }
        OpMode::Node => {
            let create_app = repo_dir()
                .join("packages/create-chiselstrike-app/dist/index.js")
                .to_str()
                .unwrap()
                .to_string();

            CheckedCommand::new("node")
                .args([&create_app, "--chisel-version", "latest", "./"])
                .current_dir(&*tmp_dir)
                .execute()
                .expect("failed to init chisel project in node mode");
        }
    }

    let db: Database = match config.db_config.clone() {
        DatabaseConfig::Postgres(config) => Database::Postgres(PostgresDb::new(config)),
        DatabaseConfig::Sqlite => Database::Sqlite(SqliteDb { tmp_dir }),
    };

    let mut chiseld = GuardedChild::new(tokio::process::Command::new(chiseld()).args([
        "--webui",
        "--db-uri",
        db.url()?.as_str(),
        "--api-listen-addr",
        &config.chiseld_config.public_address.to_string(),
        "--internal-routes-listen-addr",
        &config.chiseld_config.internal_address.to_string(),
        "--rpc-listen-addr",
        &config.chiseld_config.rpc_address.to_string(),
    ]));

    tokio::select! {
        res = chiseld.wait() => {
            let exit_status = res.expect("could not wait() for chiseld");
            chiseld.show_output().await;
            panic!("chiseld prematurely exited with {}", exit_status);
        },
        res = chisel.wait() => {
            res.expect("failed to start up chiseld");
        },
    }

    Ok((db, chiseld, chisel))
}

#[derive(Clone, Debug)]
pub struct TestConfig {
    pub mode: OpMode,
    pub db_config: DatabaseConfig,
    pub optimize: bool,
    pub chiseld_config: ChiseldConfig,
}

impl TestConfig {
    pub async fn setup(self) -> TestContext {
        let (db, chiseld, chisel) = setup_chiseld(&self).await.expect("failed to setup chiseld");
        TestContext {
            mode: self.mode,
            chiseld,
            chisel,
            _db: db,
        }
    }
}

pub struct TestContext {
    pub mode: OpMode,
    pub chiseld: GuardedChild,
    pub chisel: Chisel,
    // Note: The Database must come after chiseld to ensure that chiseld is dropped and terminated
    // before we try to drop the database.
    _db: Database,
}

impl TestContext {
    pub fn get_chisels(&mut self) -> (&mut Chisel, &mut GuardedChild) {
        (&mut self.chisel, &mut self.chiseld)
    }
}

use futures::future::BoxFuture;
pub trait TestFn {
    fn call(&self, args: TestConfig) -> BoxFuture<'static, ()>;
}

impl<T, F> TestFn for T
where
    T: Fn(TestConfig) -> F,
    F: std::future::Future<Output = ()> + 'static + std::marker::Send,
{
    fn call(&self, config: TestConfig) -> BoxFuture<'static, ()> {
        Box::pin(self(config))
    }
}

pub struct IntegrationTest {
    pub name: &'static str,
    pub mode: OpMode,
    pub test_fn: &'static (dyn TestFn + Sync),
}
