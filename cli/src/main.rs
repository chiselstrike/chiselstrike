use chisel::chisel_rpc_client::ChiselRpcClient;
use chisel::StatusRequest;

pub mod chisel {
    tonic::include_proto!("chisel");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = ChiselRpcClient::connect("http://localhost:50051").await?;
    let request = tonic::Request::new(StatusRequest {});
    let response = client.get_status(request).await?.into_inner();
    println!("Server status is {}", response.message);
    Ok(())
}
