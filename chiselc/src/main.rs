// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use std::fs::File;
use std::io::{self, Cursor, Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use chiselc::parse::compile;
use chiselc::rewrite::Target;
use chiselc::symbols::Symbols;

#[derive(Parser, Debug)]
#[command(name = "chiselc", version)]
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
    let mut data = String::new();
    input.read_to_string(&mut data)?;
    let mut symbols = Symbols::new();
    for entity in opt.entities {
        symbols.register_entity(&entity);
    }

    let mut output = Cursor::new(Vec::new());
    compile(data, symbols, opt.target, &mut output)?;

    match opt.output {
        Some(path) => {
            File::create(path)?.write_all(output.get_ref())?;
        }
        None => {
            io::stdout().lock().write_all(output.get_ref())?;
        }
    };

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn test_version_arg() {
        let opts = Opt::try_parse_from(["prg", "--version"]);
        let err = opts.unwrap_err().kind();
        assert!(matches!(err, ErrorKind::DisplayVersion));
    }
}
