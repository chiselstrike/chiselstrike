// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::env;
use std::ffi::OsString;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicU16, Ordering};

use once_cell::sync::Lazy;

pub static CHISEL_BIN_DIR: Lazy<PathBuf> = Lazy::new(|| repo_dir().join(".chisel_dev"));
pub static CHISEL_LOCAL_PATH: Lazy<OsString> = Lazy::new(|| {
    let new_path = format!("{}/bin:{}", CHISEL_BIN_DIR.display(), env!("PATH"));
    OsString::from(new_path)
});

pub struct Command {
    pub cmd: String,
    pub args: Vec<String>,
    pub inner: process::Command,
}

impl Drop for Command {
    fn drop(&mut self) {
        let out = self.inner.output().unwrap_or_else(|err| {
            panic!(
                "Spawning of command {:?} {:?} failed: {}",
                self.cmd, self.args, err
            );
        });
        let stderr = String::from_utf8_lossy(&out.stderr);
        eprintln!("{stderr}");
        assert!(
            out.status.success(),
            "Command {:?} {:?} exited with status {}",
            self.cmd,
            self.args,
            out.status,
        );
    }
}

impl Deref for Command {
    type Target = process::Command;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Command {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[allow(dead_code)]
pub fn run_in<'a, T: IntoIterator<Item = &'a str>>(cmd: &str, args: T, dir: PathBuf) -> Command {
    assert!(
        dir.exists(),
        "{:?} does not exist. Current directory is {:?}",
        dir,
        env::current_dir().unwrap()
    );
    assert!(dir.is_dir(), "{:?} is not a directory", dir);

    let cmd = cmd.to_string();
    let args = args.into_iter().map(|arg| arg.to_string()).collect();

    let mut inner = process::Command::new(&cmd);
    inner.args(&args).current_dir(dir);

    inner.env("PATH", &*CHISEL_LOCAL_PATH);

    Command { cmd, args, inner }
}

#[allow(dead_code)]
pub fn run<'a, T: IntoIterator<Item = &'a str>>(cmd: &str, args: T) -> Command {
    run_in(cmd, args, repo_dir())
}

#[allow(dead_code)]
pub fn bin_dir() -> PathBuf {
    let mut path = env::current_exe().unwrap();
    path.pop();
    path.pop();
    path
}

#[allow(dead_code)]
pub fn chisel_exe() -> PathBuf {
    bin_dir().join("chisel")
}

#[allow(dead_code)]
pub fn repo_dir() -> PathBuf {
    let mut path = bin_dir();
    path.pop();
    path.pop();
    path
}

#[allow(dead_code)]
pub fn get_free_port(ports_counter: &AtomicU16) -> u16 {
    for _ in 0..10000 {
        let port = ports_counter.fetch_add(1, Ordering::Relaxed);
        if port_scanner::local_port_available(port) {
            return port;
        }
    }
    panic!("failed to find free port in 10000 iterations");
}
