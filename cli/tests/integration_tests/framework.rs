use crate::database::Database;
use anyhow::{anyhow, Context, Result};
use bytes::{Bytes, BytesMut};
use std::borrow::Borrow;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;
use std::{error, fmt, str};
use std::{fs, net::SocketAddr};
use tempdir::TempDir;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

pub mod prelude {
    pub use super::{json_is_subset, Chisel, TestContext};
    pub use bytes::Bytes;
    pub use chisel_macros::test;
    pub use reqwest::Method;
    pub use serde_json::json;
}

pub struct GuardedChild {
    child: tokio::process::Child,
    command: tokio::process::Command,
    pub stdout: AsyncTestableOutput,
    pub stderr: AsyncTestableOutput,
}

impl GuardedChild {
    pub fn new(mut cmd: tokio::process::Command) -> Self {
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().expect("failed to spawn GuardedChild");

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        Self {
            child,
            command: cmd,
            stdout: AsyncTestableOutput::new(OutputType::Stdout, Box::pin(stdout)),
            stderr: AsyncTestableOutput::new(OutputType::Stderr, Box::pin(stderr)),
        }
    }

    pub async fn wait(&mut self) -> ExitStatus {
        self.child
            .wait()
            .await
            .expect("wait() on a child process failed")
    }

    /// Prints both stdout and stderr to standard output.
    pub async fn show_output(&mut self) {
        self.stdout.show().await;
        self.stderr.show().await;
    }

    async fn restart(&mut self) {
        use nix::sys::signal;
        use nix::unistd::Pid;

        let pid = Pid::from_raw(self.child.id().unwrap().try_into().unwrap());
        signal::kill(pid, signal::Signal::SIGTERM).unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.wait())
            .await
            .expect("child process failed to respond to SIGTERM");

        self.child = self.command.spawn().expect("failed to spawn GuardedChild");

        let stdout = self.child.stdout.take().unwrap();
        let stderr = self.child.stderr.take().unwrap();
        self.stdout = AsyncTestableOutput::new(OutputType::Stdout, Box::pin(stdout));
        self.stderr = AsyncTestableOutput::new(OutputType::Stderr, Box::pin(stderr));
    }
}

#[derive(PartialEq, Debug, Clone, Copy, Eq)]
pub enum OutputType {
    Stdout,
    Stderr,
}

impl OutputType {
    fn as_str(&self) -> &'static str {
        match self {
            OutputType::Stdout => "stdout",
            OutputType::Stderr => "stderr",
        }
    }
}

pub struct ProcessOutput {
    pub status: ExitStatus,
    pub stdout: TestableOutput,
    pub stderr: TestableOutput,
}

impl ProcessOutput {
    pub fn into_result(self) -> Result<Self, Self> {
        if self.status.success() {
            Ok(self)
        } else {
            Err(self)
        }
    }
}

impl From<std::process::Output> for ProcessOutput {
    fn from(output: std::process::Output) -> Self {
        Self {
            status: output.status,
            stdout: TestableOutput::new(OutputType::Stdout, &output.stdout),
            stderr: TestableOutput::new(OutputType::Stderr, &output.stderr),
        }
    }
}

impl fmt::Display for ProcessOutput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ProcessOutput ({}):\nSTDOUT:\n{}\nSTDERR:\n{}",
            self.status,
            textwrap::indent(&self.stdout.output, "    "),
            textwrap::indent(&self.stderr.output, "    ")
        )
    }
}

impl fmt::Debug for ProcessOutput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        <ProcessOutput as fmt::Display>::fmt(self, f)
    }
}

impl error::Error for ProcessOutput {}

/// Executes the command while mapping the output to ProcessOutput for easier manipulation and
/// debugging.
pub fn execute(cmd: &mut std::process::Command) -> Result<ProcessOutput> {
    Ok(ProcessOutput::from(cmd.output()?).into_result()?)
}

/// Executes the command while mapping the output to ProcessOutput for easier manipulation and
/// debugging.
pub async fn execute_async(cmd: &mut tokio::process::Command) -> Result<ProcessOutput> {
    Ok(ProcessOutput::from(cmd.output().await?).into_result()?)
}

#[derive(Debug)]
pub struct TestableOutput {
    output_type: OutputType,
    output: String,
    cursor: usize,
}

impl TestableOutput {
    fn new(output_type: OutputType, raw_output: &[u8]) -> Self {
        let colorless_output = strip_ansi_escapes::strip(raw_output)
            .expect("Could not strip ANSI escapes from output");
        let output = String::from_utf8_lossy(&colorless_output).into();
        Self {
            output_type,
            output,
            cursor: 0,
        }
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
            let out_type = self.output_type.as_str();
            let output = &self.output;
            panic!("failed to find text in the {out_type}: {pattern:?}\nFull output:\n{output}");
        }
    }

    /// Tries to find `pattern` in the output starting from the last successfully read
    /// position (cursor). If not found, the function will panic.
    pub fn peek(&self, pattern: &str) -> &Self {
        if !self.output[self.cursor..].contains(pattern) {
            let out_type = self.output_type.as_str();
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
            let out_type = self.output_type.as_str();
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
    pub async fn show(&mut self) {
        self.load_to_buffer(Duration::from_secs(1)).await;

        let mut stdout = tokio::io::stdout();
        stdout.write_all(&self.raw_output).await.unwrap();
        stdout.flush().await.unwrap();
    }
}

pub struct Chisel {
    pub rpc_address: SocketAddr,
    pub api_address: SocketAddr,
    pub chisel_path: PathBuf,
    pub tmp_dir: Arc<TempDir>,
    pub client: reqwest::Client,
}

impl Chisel {
    /// Runs a `chisel` subcommand.
    pub async fn exec(&self, cmd: &str, args: &[&str]) -> Result<ProcessOutput, ProcessOutput> {
        let rpc_url = format!("http://{}", self.rpc_address);
        let args = [&["--rpc-addr", &rpc_url, cmd], args].concat();

        let output = tokio::process::Command::new(&self.chisel_path)
            .args(args)
            .current_dir(&*self.tmp_dir)
            .output()
            .await
            .unwrap_or_else(|e| panic!("could not execute `chisel {}`: {}", cmd, e));
        ProcessOutput::from(output).into_result()
    }

    /// Runs `chisel apply`.
    pub async fn apply(&self) -> Result<ProcessOutput, ProcessOutput> {
        self.exec("apply", &[]).await
    }

    /// Runs `chisel apply` and asserts that it succeeds.
    pub async fn apply_ok(&self) -> ProcessOutput {
        self.apply().await.expect("chisel apply failed")
    }

    /// Runs `chisel apply` and asserts that it fails.
    pub async fn apply_err(&self) -> ProcessOutput {
        self.apply()
            .await
            .expect_err("chisel apply succeeded, but it should have failed")
    }

    /// Runs `chisel wait` awaiting the readiness of chiseld service
    pub async fn wait(&self) -> Result<ProcessOutput, ProcessOutput> {
        self.exec("wait", &[]).await
    }

    /// Runs `chisel describe` awaiting the readiness of chiseld service
    pub async fn describe(&self) -> Result<ProcessOutput, ProcessOutput> {
        self.exec("describe", &[]).await
    }

    /// Writes given `text` (probably code) into a file on given relative `path`
    /// in ChiselStrike project.
    pub fn write(&self, path: &str, text: &str) {
        let full_path = self.tmp_dir.path().join(path);
        fs::create_dir_all(full_path.parent().unwrap())
            .unwrap_or_else(|e| panic!("Unable to create directory for {:?}: {}", path, e));
        fs::write(full_path, text)
            .unwrap_or_else(|e| panic!("Unable to write to {:?}: {}", path, e));
    }

    /// Writes given `text` (probably code) into a file on given relative `path`
    /// in ChiselStrike project while unindenting everything as left as possible.
    pub fn write_unindent(&self, path: &str, text: &str) {
        let unindent_text = unindent::unindent(text);
        self.write(path, &unindent_text);
    }

    /// Copies given `file` to a relative directory path `to` inside ChiselStrike project.
    pub fn copy_to_dir<P, Q>(&self, from: P, to: Q) -> u64
    where
        P: AsRef<Path> + fmt::Debug,
        Q: AsRef<Path> + fmt::Debug,
    {
        let options = fs_extra::dir::CopyOptions {
            copy_inside: true,
            ..Default::default()
        };
        fs_extra::copy_items(&[&from], self.tmp_dir.path().join(&to), &options)
            .unwrap_or_else(|_| panic!("failed to copy {:?} to {:?}", from, to))
    }

    /// Copies given `file` to a relative path `to` inside ChiselStrike project.
    #[allow(dead_code)]
    pub fn copy_and_rename<P, Q>(&self, from: P, to: Q) -> u64
    where
        P: AsRef<Path> + fmt::Debug,
        Q: AsRef<Path> + fmt::Debug,
    {
        std::fs::copy(&from, self.tmp_dir.path().join(&to))
            .unwrap_or_else(|_| panic!("failed to copy {:?} to {:?}", from, to))
    }

    pub fn remove_file<P>(&self, path: P)
    where
        P: AsRef<Path> + fmt::Debug,
    {
        std::fs::remove_file(self.tmp_dir.path().join(&path))
            .unwrap_or_else(|_| panic!("failed to remove file {:?}", path))
    }

    pub fn request(&self, method: reqwest::Method, url: &str) -> RequestBuilder {
        let chisel_url = reqwest::Url::parse(&format!("http://{}", self.api_address)).unwrap();
        let url = chisel_url.join(url).unwrap();
        RequestBuilder {
            client: self.client.clone(),
            builder: self.client.request(method, url),
        }
    }

    pub fn get(&self, url: &str) -> RequestBuilder {
        self.request(reqwest::Method::GET, url)
    }

    pub fn post(&self, url: &str) -> RequestBuilder {
        self.request(reqwest::Method::POST, url)
    }

    pub fn delete(&self, url: &str) -> RequestBuilder {
        self.request(reqwest::Method::DELETE, url)
    }

    pub async fn get_text(&self, url: &str) -> String {
        self.get(url).send().await.text()
    }

    pub async fn get_json(&self, url: &str) -> serde_json::Value {
        self.get(url).send().await.json()
    }

    pub async fn post_json_ok<V: Borrow<serde_json::Value>>(&self, url: &str, data: V) {
        self.post(url).json(data).send().await.assert_ok();
    }

    pub async fn post_json_status<V: Borrow<serde_json::Value>>(&self, url: &str, data: V) -> u16 {
        self.post(url).json(data).send().await.status()
    }

    pub async fn post_json_text<V: Borrow<serde_json::Value>>(&self, url: &str, data: V) -> String {
        self.post(url).json(data).send().await.text()
    }
}

#[must_use]
pub struct RequestBuilder {
    client: reqwest::Client,
    builder: reqwest::RequestBuilder,
}

impl RequestBuilder {
    fn map<F: FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder>(self, f: F) -> Self {
        Self {
            client: self.client,
            builder: f(self.builder),
        }
    }

    pub fn json<V: Borrow<serde_json::Value>>(self, data: V) -> Self {
        self.map(|b| b.json(data.borrow()))
    }

    pub fn header(self, name: &str, value: &str) -> Self {
        self.map(|b| b.header(name, value))
    }

    pub async fn send(self) -> Response {
        let request = self.builder.build().unwrap();
        let (method, url) = (request.method().clone(), request.url().clone());
        let response = self
            .client
            .execute(request)
            .await
            .unwrap_or_else(|err| panic!("HTTP error for {} {}: {}", method, url, err));

        let status = response.status();
        let body = response.bytes().await.unwrap_or_else(|err| {
            panic!(
                "HTTP error for {} {} while reading response {}: {}",
                method, url, status, err
            )
        });

        Response {
            method,
            url,
            status,
            body,
        }
    }
}

#[must_use]
pub struct Response {
    method: reqwest::Method,
    url: reqwest::Url,
    status: reqwest::StatusCode,
    body: Bytes,
}

impl Response {
    pub fn status(&self) -> u16 {
        self.status.as_u16()
    }

    pub fn assert_ok(&self) -> &Self {
        assert!(
            self.status.is_success(),
            "HTTP error response for {} {}: {}\nResponse body {:?}",
            self.method,
            self.url,
            self.status,
            self.body,
        );
        self
    }

    pub fn assert_status(&self, expected: u16) -> &Self {
        assert!(
            self.status.as_u16() == expected,
            "Expected status {}, got {} for HTTP {} {}: {}\nResponse body {:?}",
            expected,
            self.status,
            self.method,
            self.url,
            self.status,
            self.body,
        );
        self
    }

    /*
    pub fn body(&self) -> Bytes {
        self.body.clone()
    }
    */

    pub fn text(&self) -> String {
        match str::from_utf8(&self.body) {
            Ok(text) => text.into(),
            Err(err) => panic!(
                "HTTP response for {} {} is not UTF-8: {}\nResponse status {}, body {:?}",
                self.method, self.url, err, self.status, self.body,
            ),
        }
    }

    pub fn assert_text(&self, expected: &str) -> &Self {
        let actual = self.text();
        assert!(
            actual == expected,
            "Unexpected text response for HTTP {} {}\nResponse status {}, body {:?}, expected {:?}",
            self.method,
            self.url,
            self.status,
            actual,
            expected,
        );
        self
    }

    pub fn assert_text_contains(&self, expected: &str) -> &Self {
        let actual = self.text();
        assert!(
            actual.contains(expected),
            "Unexpected text response for HTTP {} {}\nResponse status {}, body {:?}, expected {:?}",
            self.method,
            self.url,
            self.status,
            actual,
            expected,
        );
        self
    }

    pub fn json(&self) -> serde_json::Value {
        match serde_json::from_slice(&self.body) {
            Ok(json) => json,
            Err(err) => panic!(
                "HTTP response for {} {} is not JSON: {}\nResponse status {}, body {:?}",
                self.method, self.url, err, self.status, self.body,
            ),
        }
    }

    pub fn assert_json<V: Borrow<serde_json::Value>>(&self, expected: V) -> &Self {
        let actual = self.json();
        assert!(
            &actual == expected.borrow(),
            "Unexpected JSON response for HTTP {} {}\nResponse status {}, body {}, expected {}",
            self.method,
            self.url,
            self.status,
            actual,
            expected.borrow(),
        );
        self
    }
}

pub struct TestContext {
    pub chiseld: GuardedChild,
    pub chisel: Chisel,
    // Note: The Database must come after chiseld to ensure that chiseld is dropped and terminated
    // before we try to drop the database.
    pub _db: Database,
}

impl TestContext {
    /// Restarts the chiseld service and waits for it to come back online.
    pub async fn restart_chiseld(&mut self) {
        self.chiseld.restart().await;
        wait_for_chiseld_startup(&mut self.chiseld, &self.chisel).await;
    }
}

pub async fn wait_for_chiseld_startup(chiseld: &mut GuardedChild, chisel: &Chisel) {
    tokio::select! {
        exit_status = chiseld.wait() => {
            chiseld.show_output().await;
            panic!("chiseld prematurely exited with {}", exit_status);
        },
        res = chisel.wait() => {
            res.expect("failed to start up chiseld");
        },
    }
}

pub fn json_is_subset(val: &serde_json::Value, subset: &serde_json::Value) -> Result<()> {
    use serde_json::Value;
    let val = val.borrow();
    let subset = subset.borrow();

    match subset {
        Value::Object(sub_obj) => {
            let obj = val.as_object().context(anyhow!(
                "subset value is object but reference value is {val}"
            ))?;
            for (key, value) in sub_obj {
                let ref_value = obj
                    .get(key)
                    .context(anyhow!("reference object doesn't contain key `{key}`"))?;
                json_is_subset(ref_value, value).context(anyhow!(
                    "value of key `{key}` is not a subset of given value"
                ))?;
            }
        }
        Value::Array(sub_array) => {
            let arr = val.as_array().context(anyhow!(
                "subset value is array but reference value is {val}"
            ))?;
            anyhow::ensure!(
                arr.len() == sub_array.len(),
                "arrays have different lengths"
            );
            for (i, e) in arr.iter().enumerate() {
                let sub_e = &sub_array[i];
                json_is_subset(e, sub_e)
                    .context(anyhow!("failed to match elements of array on position {i}"))?
            }
        }
        Value::Number(_) => {
            anyhow::ensure!(
                val.is_number(),
                "subset value is number but reference value is {val}",
            );
            anyhow::ensure!(val == subset);
        }
        Value::String(_) => {
            anyhow::ensure!(
                val.is_string(),
                "subset value is string but reference value is {val}",
            );
            anyhow::ensure!(val == subset);
        }
        Value::Bool(_) => {
            anyhow::ensure!(
                val.is_boolean(),
                "subset value is bool but reference value is {val}",
            );
            anyhow::ensure!(val == subset);
        }
        Value::Null => {
            anyhow::ensure!(
                val.is_null(),
                "subset value is null but reference value is {val}",
            );
        }
    }
    Ok(())
}
