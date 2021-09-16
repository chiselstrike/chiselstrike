pub mod parser;

use chisel::chisel_rpc_client::ChiselRpcClient;
use chisel::StatusRequest;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "chisel")]
enum Opt {
    /// Shows information about ChiselStrike server status.
    Status,
}

pub mod chisel {
    tonic::include_proto!("chisel");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();
    match opt {
        Opt::Status => {
            let mut client = ChiselRpcClient::connect("http://localhost:50051").await?;
            let request = tonic::Request::new(StatusRequest {});
            let response = client.get_status(request).await?.into_inner();
            println!("Server status is {}", response.message);
        }
    }
    Ok(())
}
