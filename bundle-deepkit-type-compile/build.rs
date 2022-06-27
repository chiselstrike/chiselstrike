use std::env;
use std::path::Path;
use std::process::Command;

pub fn run(cmd: &str, args: &[&str], dir: &Path) {
    let status = Command::new(cmd)
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    let dkf = "../third_party/deepkit-framework";
    let cur = env::current_dir().unwrap();

    // We need to rerun when deepkit-framework changes. Since the
    // packages.json files get a new mtime even when the content
    // doesn't change, we build in a copy of the deepkit-framework
    // directory and tell deno to watch the original.
    println!("cargo:rerun-if-changed={}", dkf);

    // Don't trust a non clean build
    let dkf_copy = &format!("{}/deepkit-framework-copy", out_dir);
    run("rm", &["-rf", dkf_copy], &cur);
    run("git", &["clone", dkf, dkf_copy], &cur);

    let cur_copy = &format!("{}/bundle-deepkit-type-compile-copy", out_dir);
    run("rm", &["-rf", cur_copy], &cur);
    run("cp", &["-r", cur.to_str().unwrap(), cur_copy], &cur);

    let dkf = Path::new(dkf_copy);
    let cur = Path::new(cur_copy);

    // This seems to be the fastest way to build only the files we need
    run("npm", &["install"], dkf);
    run(
        "npm",
        &[
            "run",
            "bootstrap",
            "--",
            "--ignore",
            "@deepkit/mongo",
            "--ignore",
            "@deepkit/postgres",
            "--ignore",
            "@deepkit/orm-browser-example",
            "--ignore",
            "@deepkit/benchmark",
        ],
        dkf,
    );
    run(
        "npx",
        &["tsc", "--build", "./packages/type-compiler/tsconfig.json"],
        dkf,
    );

    // Install @deepkit/type-compiler and bundle it, but exclude
    // typescript for two reasons:
    // * We want to use the copy we get from deno
    // * @deepkit/type-compiler tries to get to typescript at runtime,
    //   so we need to hack it (see tsc_compile_build/src/prefix.js)
    run("npm", &["install"], cur);
    run(
        "npm",
        &["install", &format!("{}/packages/type-compiler", dkf_copy)],
        cur,
    );
    run(
        "./node_modules/.bin/esbuild",
        &[
            "--bundle",
            "--platform=node",
            "--external:typescript",
            "--outfile=bundle.js",
            "--tsconfig=tsconfig.json",
            "./reexport.js",
        ],
        cur,
    );
}
