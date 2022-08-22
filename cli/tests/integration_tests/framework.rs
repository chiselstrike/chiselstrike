use crate::database::Database;
use anyhow::Result;
use bytes::{Bytes, BytesMut};
use reqwest::header::HeaderMap;
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
    pub use super::TestContext;
    pub use bytes::Bytes;
    pub use chisel_macros::test;
    pub use reqwest::Method;
    pub use serde_json::json;
}

pub struct GuardedChild {
    child: tokio::process::Child,
    pub stdout: AsyncTestableOutput,
    pub stderr: AsyncTestableOutput,
}

impl GuardedChild {
    pub fn new(command: &mut tokio::process::Command) -> Self {
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
}

#[derive(PartialEq, Debug, Clone, Copy)]
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
    pub fn peek(self, pattern: &str) -> Self {
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

    pub async fn restart(&self) -> Result<ProcessOutput, ProcessOutput> {
        self.exec("restart", &[]).await
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

    /// Sends a HTTP request to a relative `url` on the running `chiseld`, using the given request
    /// `method` and `body`. Does not check that the response is successful. Panics if there was an error while
    /// handling the request.
    pub async fn request_with_headers<B>(
        &self,
        method: reqwest::Method,
        url: &str,
        body: B,
        headers: HeaderMap,
    ) -> reqwest::Response
    where
        B: Into<reqwest::Body>,
    {
        let full_url = url::Url::parse(&format!("http://{}", self.api_address))
            .unwrap()
            .join(url)
            .unwrap();
        self.client
            .request(method.clone(), full_url)
            .body(body)
            .headers(headers)
            .timeout(core::time::Duration::from_secs(5))
            .send()
            .await
            .unwrap_or_else(|e| panic!("HTTP error in {} {}: {}", method, url, e))
    }

    pub async fn request<B>(&self, method: reqwest::Method, url: &str, body: B) -> reqwest::Response
    where
        B: Into<reqwest::Body>,
    {
        self.request_with_headers(method, url, body, HeaderMap::new())
            .await
    }

    /// Same as `request()`, but reads the response status and body as bytes.
    pub async fn request_body<B>(&self, method: reqwest::Method, url: &str, body: B) -> (u16, Bytes)
    where
        B: Into<reqwest::Body>,
    {
        self.request_body_with_headers(method, url, body, HeaderMap::new())
            .await
    }

    pub async fn request_body_with_headers<B>(
        &self,
        method: reqwest::Method,
        url: &str,
        body: B,
        headers: HeaderMap,
    ) -> (u16, Bytes)
    where
        B: Into<reqwest::Body>,
    {
        let response = self
            .request_with_headers(method.clone(), url, body, headers)
            .await;
        let status = response.status().as_u16();
        match response.bytes().await {
            Ok(response_body) => (status, response_body),
            Err(err) => panic!(
                "HTTP error in {} {} while reading response: {}",
                method, url, err
            ),
        }
    }

    /// Same as `request()`, but returns the response body as text
    pub async fn request_text<B>(&self, method: reqwest::Method, url: &str, body: B) -> String
    where
        B: Into<reqwest::Body>,
    {
        self.request_text_with_headers(method, url, body, HeaderMap::new())
            .await
    }

    /// Same as `request_text()`, but with headers
    pub async fn request_text_with_headers<B>(
        &self,
        method: reqwest::Method,
        url: &str,
        body: B,
        headers: HeaderMap,
    ) -> String
    where
        B: Into<reqwest::Body>,
    {
        let (status, response_body) = self
            .request_body_with_headers(method.clone(), url, body, headers)
            .await;
        match str::from_utf8(&response_body) {
            Ok(text) => text.into(),
            Err(err) => panic!(
                "HTTP response for {} {} is not UTF-8: {}\nResponse status {}, body {:?}",
                method, url, err, status, response_body
            ),
        }
    }

    /// Same as `request()`, but returns the response body as JSON.
    pub async fn request_json<B>(
        &self,
        method: reqwest::Method,
        url: &str,
        body: B,
    ) -> serde_json::Value
    where
        B: Into<reqwest::Body>,
    {
        let (status, response_body) = self.request_body(method.clone(), url, body).await;
        match serde_json::from_slice(&response_body) {
            Ok(json) => json,
            Err(err) => panic!(
                "HTTP response for {} {} is not JSON: {}\nResponse status {}, body {:?}",
                method, url, err, status, response_body,
            ),
        }
    }

    /// Same as `request()`, but returns the response status.
    pub async fn request_status<B>(&self, method: reqwest::Method, url: &str, body: B) -> u16
    where
        B: Into<reqwest::Body>,
    {
        self.request_status_with_headers(method, url, body, HeaderMap::new())
            .await
    }

    /// Same as `request()`, but returns the response status.
    pub async fn request_status_with_headers<B>(
        &self,
        method: reqwest::Method,
        url: &str,
        body: B,
        headers: HeaderMap,
    ) -> u16
    where
        B: Into<reqwest::Body>,
    {
        self.request_with_headers(method, url, body, headers)
            .await
            .status()
            .as_u16()
    }

    /*
    /// Same as `request()`, but sends GET with no request body.
    pub async fn get(&self, url: &str) -> reqwest::Response {
        self.request(reqwest::Method::GET, url, "").await
    }

    /// Same as `request_body()`, but sends GET with no request body.
    pub async fn get_body(&self, url: &str) -> (u16, Bytes) {
        self.request_body(reqwest::Method::GET, url, "").await
    }
    */

    /// Same as `request_text()`, but sends GET with no request body.
    pub async fn get_text(&self, url: &str) -> String {
        self.request_text(reqwest::Method::GET, url, "").await
    }

    /// Same as `request_text_with_headers()`, but sends GET with no request body.
    pub async fn get_text_with_headers(&self, url: &str, headers: HeaderMap) -> String {
        self.request_text_with_headers(reqwest::Method::GET, url, "", headers)
            .await
    }

    /// Same as `request_json()`, but sends GET with no request body.
    pub async fn get_json(&self, url: &str) -> serde_json::Value {
        self.request_json(reqwest::Method::GET, url, "").await
    }

    /// Same as `request_status()`, but sends GET with no request body.
    pub async fn get_status(&self, url: &str) -> u16 {
        self.get_status_with_headers(url, HeaderMap::new()).await
    }

    pub async fn get_status_with_headers(&self, url: &str, headers: HeaderMap) -> u16 {
        self.request_status_with_headers(reqwest::Method::GET, url, "", headers)
            .await
    }

    /// Same as `request()`, but sends POST with JSON request payload.
    pub async fn post_json<Value>(&self, url: &str, data: Value) -> reqwest::Response
    where
        Value: Borrow<serde_json::Value>,
    {
        self.request(
            reqwest::Method::POST,
            url,
            serde_json::to_string(data.borrow()).unwrap(),
        )
        .await
    }

    /// Same as `post_json()`, but asserts that the response was a success.
    pub async fn post_json_ok<Value>(&self, url: &str, data: Value)
    where
        Value: Borrow<serde_json::Value>,
    {
        self.post_json(url, data)
            .await
            .error_for_status()
            .unwrap_or_else(|e| panic!("HTTP error response in POST {}: {}", url, e));
    }

    /// Same as `request_text()`, but sends POST with JSON request payload.
    pub async fn post_json_text<Value>(&self, url: &str, data: Value) -> String
    where
        Value: Borrow<serde_json::Value>,
    {
        self.request_text(
            reqwest::Method::POST,
            url,
            serde_json::to_string(data.borrow()).unwrap(),
        )
        .await
    }

    /// Same as `request_status()`, but sends POST with JSON request payload.
    pub async fn post_json_status<Value>(&self, url: &str, data: Value) -> u16
    where
        Value: Borrow<serde_json::Value>,
    {
        self.request_status(
            reqwest::Method::POST,
            url,
            serde_json::to_string(data.borrow()).unwrap(),
        )
        .await
    }
}

pub struct TestContext {
    pub chiseld: GuardedChild,
    pub chisel: Chisel,
    // Note: The Database must come after chiseld to ensure that chiseld is dropped and terminated
    // before we try to drop the database.
    pub _db: Database,
}

pub fn header(name: &'static str, value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(name, value.parse().unwrap());
    headers
}
