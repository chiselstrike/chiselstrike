mod parse;
mod query;
mod rewrite;
mod symbols;
mod transforms;
mod utils;

use crate::parse::compile;
use crate::rewrite::Target;
use crate::symbols::Symbols;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(name = "chiselc")]
struct Opt {
    /// Input file
    #[structopt(parse(from_os_str))]
    input: Option<PathBuf>,
    /// Output file (optional).
    #[structopt(short, long)]
    output: Option<PathBuf>,
    /// Entity types
    #[structopt(short, long)]
    entities: Vec<String>,
    /// Entity types
    #[structopt(short, long, default_value = "js")]
    target: Target,
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    let mut input = match opt.input {
        Some(path) => {
            let file = File::open(path.clone())
                .with_context(|| format!("Failed to open `{}`.", path.display()))?;
            Box::new(file) as Box<dyn Read>
        }
        None => Box::new(io::stdin()) as Box<dyn Read>,
    };
    let output = match opt.output {
        Some(path) => File::create(path).map(|file| Box::new(file) as Box<dyn Write>),
        None => Ok(Box::new(io::stdout()) as Box<dyn Write>),
    }?;
    let mut data = String::new();
    input.read_to_string(&mut data)?;
    let mut symbols = Symbols::new();
    for entity in opt.entities {
        symbols.register_entity(&entity);
    }
    compile(data, symbols, opt.target, output)?;
    Ok(())
}
