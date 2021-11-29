use std::env;
use std::path::PathBuf;
use std::process::Command;

pub fn run<T: IntoIterator<Item = &'static str>>(cmd: &str, args: T) {
    let status = Command::new(cmd)
        .args(args)
        .current_dir(repo_dir())
        .status()
        .unwrap();
    assert!(status.success());
}

pub fn bin_dir() -> PathBuf {
    let mut path = env::current_exe().unwrap();
    path.pop();
    path.pop();
    path
}

pub fn repo_dir() -> PathBuf {
    let mut path = bin_dir();
    path.pop();
    path.pop();
    path
}
