use docopt::Docopt;
use serde::{Deserialize, Serialize};
use serde_json::to_vec_pretty;
use std::fs;
use std::path::Path;
use std::str;
use vlq::encode;
use wasmparser::{ModuleReader, SectionCode};

const USAGE: &str = "
Create dummy map for wasm file (to be handled with binaryen).

Usage:
    wtmaps <file> -o <output>
    wtmaps --help

Options:
    -h, --help          print this help message
";

#[derive(Deserialize, Debug, Clone)]
struct Args {
    arg_file: String,
    arg_output: String,
}

fn find_wasm_positions<F>(data: &[u8], mut f: F)
where
    F: FnMut(usize, usize),
{
    for section in ModuleReader::new(data).expect("valid wasm") {
        let section = section.expect("section");
        match section.code {
            SectionCode::Code => (),
            _ => continue,
        }
        let mut code_reader = section.get_code_section_reader().expect("code section");
        let code_section_offset = section.range().start;
        for _ in 0..code_reader.get_count() {
            f(code_reader.original_position(), code_section_offset);
            let code = code_reader.read().expect("fn code");
            let mut op_reader = code.get_operators_reader().expect("op reader");
            while !op_reader.eof() {
                f(op_reader.original_position(), code_section_offset);
                op_reader.read().expect("op");
            }
        }
        f(code_reader.original_position(), code_section_offset);
    }
}

fn build_mappings(wasm: &[u8]) -> Vec<u8> {
    let mut buffer = Vec::new();
    let mut last_col = 0;
    let mut last_addr = 0;
    find_wasm_positions(wasm, |col, offset| {
        if last_col >= col {
            return;
        }
        let delta = (col - last_col) as i64;
        encode(delta, &mut buffer).expect("addr");
        encode(0, &mut buffer).expect("source");
        encode(0, &mut buffer).expect("line");
        let delta_addr = (col - offset - last_addr) as i64;
        encode(delta_addr, &mut buffer).expect("col");
        buffer.push(b',');
        last_col = col;
        last_addr = col - offset;
    });
    if buffer.len() > 0 {
        buffer.pop();
    }
    buffer
}

#[derive(Serialize)]
struct SourceMap {
    version: u8,
    sources: Vec<String>,
    names: Vec<String>,
    mappings: String,
}

fn build_json(mappings: &[u8], source: String) -> Vec<u8> {
    let mappings = str::from_utf8(mappings).unwrap();

    let root = SourceMap {
        version: 3,
        sources: vec![source],
        names: vec![],
        mappings: String::from(mappings),
    };
    to_vec_pretty(&root).expect("json out")
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.help(true).deserialize())
        .unwrap_or_else(|e| e.exit());

    let wasm = fs::read(Path::new(&args.arg_file)).expect("wasm file data");
    let mappings = build_mappings(&wasm);
    let json = build_json(&mappings, String::from(args.arg_file));
    fs::write(Path::new(&args.arg_output), &json).expect("json written");
}
