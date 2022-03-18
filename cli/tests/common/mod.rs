use std::env;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::process;

pub struct Command {
    inner: process::Command,
}

impl Drop for Command {
    fn drop(&mut self) {
        let status = self.inner.status().unwrap();
        assert!(status.success());
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
    let mut inner = process::Command::new(cmd);
    inner.args(args).current_dir(dir);
    Command { inner }
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
pub fn repo_dir() -> PathBuf {
    let mut path = bin_dir();
    path.pop();
    path.pop();
    path
}
