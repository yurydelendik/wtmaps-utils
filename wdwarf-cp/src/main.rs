use docopt::Docopt;
use gimli::{self, read, write};
use serde::Deserialize;
use std::fs;
use std::io::BufReader;
use std::path::Path;
use wdwarf::{
    build_dependencies, create_dwarf_sections, from_dwarf, AddressTranslator,
    IdentityAddressTranslator, TranformAddressTranslator,
};

mod json_map;
mod wasm;

const USAGE: &str = "
Copy WebAssembly DWARF with appling a transform. The dead code will be removed.

Usage:
    wdwarf-cp <source-file> -o <output> [-m <json> -w <wasm>]
    wdwarf-cp <source-file> -i <output> -m <json>
    wdwarf-cp --help

Options:
    -h, --help             print this help message
    -m, --source-map=JSON  JSON source maps-like transform
    -w, --wasm-file=WASM   WebAssembly transformed file
    -i, --in-place         In-place WebAssembly file sections replacement
    -o, --output           Output WebAssembly file
";

#[derive(Deserialize, Debug, Clone)]
struct Args {
    arg_source_file: String,
    arg_output: String,
    flag_source_map: Option<String>,
    flag_wasm_file: Option<String>,
}

fn build_new_dwarf<R: gimli::Reader<Offset = usize>, A: AddressTranslator>(
    dwarf: read::Dwarf<R>,
    at: A,
) -> write::ConvertResult<write::Dwarf> {
    let deps = build_dependencies(&dwarf, &at).expect("deps");
    let reachable = deps.get_reachable();
    from_dwarf(&dwarf, &at, &|uo| reachable.contains(&uo))
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.help(true).deserialize())
        .unwrap_or_else(|e| e.exit());

    let bin = fs::read(Path::new(&args.arg_source_file)).expect("file data");
    let dwarf = wasm::read_dwarf(&bin);

    let (map, input_wasm) = if let Some(source_map_file) = &args.flag_source_map {
        let (input_wasm, code_section_offsets) = {
            let wasm_input_file = args
                .flag_wasm_file
                .as_ref()
                .unwrap_or_else(|| &args.arg_output);
            let mut input = fs::read(Path::new(wasm_input_file)).expect("file data");
            let code_section_offsets = wasm::read_code_section_offsets(&input);
            wasm::remove_debug_sections(&mut input);
            (input, code_section_offsets)
        };
        let file = fs::File::open(source_map_file).expect("json file");
        let map = json_map::read_json_map_transform(
            BufReader::new(file),
            code_section_offsets.code_section_offset,
        )
        .expect("json");

        (
            Some((map, code_section_offsets.function_ranges)),
            input_wasm,
        )
    } else {
        (None, Vec::from(wasm::WASM_HEADER))
    };

    let mut new_dwarf = if let Some(map) = map {
        build_new_dwarf(dwarf, TranformAddressTranslator::new(map.0, map.1))
    } else {
        build_new_dwarf(dwarf, IdentityAddressTranslator(true))
    }
    .expect("new dwarf");

    let mut wasm = Vec::new();
    wasm.extend_from_slice(&input_wasm);
    wasm.extend_from_slice(&create_dwarf_sections(&mut new_dwarf).expect("write dwarf sections"));
    fs::write(Path::new(&args.arg_output), &wasm).expect("write wasm");
}
