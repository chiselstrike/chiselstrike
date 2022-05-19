use deno_runtime::inspector_server::InspectorServer;
use std::net::SocketAddr;
use structopt::StructOpt;
use tsc_compile::Compiler;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(long)]
    inspect: bool,

    file: String,
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    let mut compiler = Compiler::new(false);

    let mut _maybe_inspector = None;
    if opt.inspect {
        let addr: SocketAddr = "127.0.0.1:9229".parse().unwrap();
        let inspector = InspectorServer::new(addr, "my_tsc".to_string());
        inspector.register_inspector("main_module".to_string(), &mut compiler.runtime, true);
        _maybe_inspector = Some(inspector);
        compiler
            .runtime
            .inspector()
            .wait_for_session_and_break_on_next_statement();
    }

    compiler
        .compile_ts_code(&opt.file, Default::default())
        .await
        .unwrap();
}
