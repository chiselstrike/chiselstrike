// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use chiselc::parse::compile;
use chiselc::rewrite::Target;
use chiselc::symbols::Symbols;

#[derive(Parser)]
#[command(name = "chiselc")]
struct Opt {
    /// Input file
    #[arg(value_parser)]
    input: Option<PathBuf>,
    /// Output file (optional).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Entity types
    #[arg(short, long)]
    entities: Vec<String>,
    /// Entity types
    #[arg(short, long, default_value = "js")]
    target: Target,
}

fn main() -> Result<()> {
    let opt = Opt::parse();
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
