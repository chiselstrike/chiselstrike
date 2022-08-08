use deno_runtime::inspector_server::InspectorServer;
use endpoint_tsc::Compiler;
use std::net::SocketAddr;
use std::path::Path;
use structopt::StructOpt;
use url::Url;

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
        inspector.register_inspector("main_module".to_string(), &mut compiler.tsc.runtime, true);
        _maybe_inspector = Some(inspector);
        compiler
            .tsc
            .runtime
            .inspector()
            .wait_for_session_and_break_on_next_statement();
    }

    let url = Url::from_file_path(Path::new(&opt.file)).unwrap();
    compiler.compile(url).await.unwrap();
}
