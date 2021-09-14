use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "chiseld")]
struct Opt {
    /// Server listen address.
    #[structopt(short, long, default_value = "localhost")]
    listen_addr: String,
}

fn main() {
    let opt = Opt::from_args();
}
