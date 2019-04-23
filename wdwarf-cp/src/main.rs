use docopt::Docopt;
use serde::Deserialize;
use std::fs;
use std::path::Path;

use gimli::write;

mod convert;
mod wasm;

const USAGE: &str = "
Copy WebAssembly DWARF with appling a transform. The dead code will be removed.

Usage:
    wdwarf-cp <source-file> -o <output> [-m <json>]
    wdwarf-cp --help

Options:
    -h, --help             print this help message
    -m, --source-map=JSON  JSON source maps-like transform
";

#[derive(Deserialize, Debug, Clone)]
struct Args {
    arg_source_file: String,
    arg_output: String,
    flag_source_map: Option<String>,
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.help(true).deserialize())
        .unwrap_or_else(|e| e.exit());

    let bin = fs::read(Path::new(&args.arg_source_file)).expect("file data");
    let dwarf = wasm::read_dwarf(&bin);

    let dwarf = convert::from_dwarf(&dwarf, &|a| -> Option<_> {
        Some(write::Address::Absolute(a))
    });
    println!("Hello, world! {:?}", dwarf);
}
