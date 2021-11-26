use std::env;
use std::path::PathBuf;

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
