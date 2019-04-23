use docopt::Docopt;
use serde::Deserialize;
use std::fs;
use std::path::Path;

use gimli::write;

mod convert;
mod gc;
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

struct AddressTranslator(bool);

impl convert::AddressTranslator for AddressTranslator {
    fn translate_address(&self, addr: u64) -> Vec<write::Address> {
        if addr == 0 && self.0 {
            return vec![];
        }
        vec![write::Address::Absolute(addr)]
    }
    fn translate_range(&self, start: u64, len: u64) -> Vec<(write::Address, u64)> {
        if start == 0 && self.0 {
            return vec![];
        }
        vec![(write::Address::Absolute(start), len)]
    }
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.help(true).deserialize())
        .unwrap_or_else(|e| e.exit());

    let bin = fs::read(Path::new(&args.arg_source_file)).expect("file data");
    let dwarf = wasm::read_dwarf(&bin);

    let new_dwarf = convert::from_dwarf(&dwarf, &AddressTranslator(false));
    let deps = gc::build_dependencies(&dwarf, &AddressTranslator(true));
    println!("Hello, world! {:?}", deps.expect("").get_reachable());
}
