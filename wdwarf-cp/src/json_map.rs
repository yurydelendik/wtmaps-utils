use serde::{Deserialize, Serialize};
use std::io::Read;
use std::io::Result;
use std::vec::Vec;
use vlq::decode;

#[derive(Debug, Deserialize, Serialize)]
struct SourceMap {
    version: u8,
    sources: Vec<String>,
    names: Vec<String>,
    mappings: String,
}

pub fn read_json_map_transform<R: Read>(reader: R, code_section_offset: u64) -> Result<()> {
    let map: SourceMap = serde_json::from_reader(reader)?;
    if map.version != 3 {
        panic!("invalid map version");
    }
    let mappings = map.mappings;
    if mappings.contains(';') {
        panic!("invalid mappings")
    }

    let mut decoded = Vec::new();
    let mut last_addr = -(code_section_offset as i64);
    let mut last_col = 0;
    for (addr_delta, col_delta) in mappings.split(',').map(|entry: &str| {
        let mut it = entry.bytes();
        let addr_delta = decode(&mut it).expect("addr");
        let _source = decode(&mut it).expect("source");
        let _line = decode(&mut it).expect("line");
        let col_delta = decode(&mut it).expect("col");
        (addr_delta, col_delta)
    }) {
        last_addr += addr_delta;
        last_col += col_delta;
        decoded.push((last_addr, last_col));
    }
    Ok(())
}
