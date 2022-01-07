use std::env;
use std::path::PathBuf;
use std::process::Command;

#[allow(dead_code)]
pub fn run_in<T: IntoIterator<Item = &'static str>>(cmd: &str, args: T, dir: PathBuf) {
    assert!(
        dir.exists(),
        "{:?} does not exist. Current directory is {:?}",
        dir,
        env::current_dir().unwrap()
    );
    assert!(dir.is_dir(), "{:?} is not a directory", dir);
    let status = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success());
}

#[allow(dead_code)]
pub fn run<T: IntoIterator<Item = &'static str>>(cmd: &str, args: T) {
    run_in(cmd, args, repo_dir());
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
