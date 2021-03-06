use gimli::{self, Dwarf, SectionId};
use std::boxed::Box;
use std::collections::HashMap;
use wasmparser::{ModuleReader, Range, SectionCode};
use wdwarf;

pub fn read_dwarf<'a>(bin: &'a [u8]) -> Dwarf<gimli::EndianSlice<'a, gimli::LittleEndian>> {
    let sections = read_dwarf_sections(bin);
    wdwarf::read_dwarf(sections).expect("dwarf")
}

fn read_dwarf_sections<'a>(bin: &'a [u8]) -> HashMap<&'a str, &'a [u8]> {
    let mut sections = HashMap::new();
    for sect in ModuleReader::new(bin).expect("wasm reader") {
        let sect = sect.expect("section");
        match sect.code {
            SectionCode::Custom { name, .. } if to_section_id(name).is_some() => {
                sections.insert(name, sect.range().slice(bin));
            }
            _ => (),
        }
    }
    sections
}

pub struct CodeSectionOffsets {
    pub code_section_offset: u64,
    pub function_ranges: Box<[(u64, u64)]>,
}

pub fn read_code_section_offsets(bin: &[u8]) -> CodeSectionOffsets {
    for sect in ModuleReader::new(bin).expect("wasm reader") {
        let sect = sect.expect("section");
        match sect.code {
            SectionCode::Code => {
                let code_section_offset = sect.range().start as u64;
                let code_reader = sect.get_code_section_reader().expect("code section");
                let ranges = code_reader
                    .into_iter()
                    .map(|f| {
                        let Range { start, end } = f.expect("function").range();
                        (
                            start as u64 - code_section_offset,
                            end as u64 - code_section_offset,
                        )
                    })
                    .collect::<Vec<_>>();
                return CodeSectionOffsets {
                    code_section_offset,
                    function_ranges: ranges.into_boxed_slice(),
                };
            }
            _ => (),
        }
    }
    panic!("code section was not found");
}

pub fn remove_debug_sections(bin: &mut Vec<u8>) {
    let mut reader = ModuleReader::new(bin).expect("wasm reader");
    let mut position = reader.current_position();
    // Record debug section locations into the sections_to_remove.
    let mut sections_to_remove = Vec::new();
    while !reader.eof() {
        {
            let sect = reader.read().expect("section");
            match sect.code {
                SectionCode::Custom { name, .. } if to_section_id(name).is_some() => {
                    sections_to_remove.push(position..sect.range().end);
                }
                _ => (),
            }
        }
        position = reader.current_position();
    }
    // In reverse order, remove all of the sections_to_remove.
    for range in sections_to_remove.into_iter().rev() {
        bin.drain(range);
    }
}

fn to_section_id(name: &str) -> Option<SectionId> {
    Some(match name {
        ".debug_abbrev" => SectionId::DebugAbbrev,
        ".debug_addr" => SectionId::DebugAddr,
        ".debug_aranges" => SectionId::DebugAranges,
        ".debug_frame" => SectionId::DebugFrame,
        ".eh_frame" => SectionId::EhFrame,
        ".eh_frame_hdr" => SectionId::EhFrameHdr,
        ".debug_info" => SectionId::DebugInfo,
        ".debug_line" => SectionId::DebugLine,
        ".debug_line_str" => SectionId::DebugLineStr,
        ".debug_loc" => SectionId::DebugLoc,
        ".debug_loclists" => SectionId::DebugLocLists,
        ".debug_macinfo" => SectionId::DebugMacinfo,
        ".debug_pubnames" => SectionId::DebugPubNames,
        ".debug_pubtypes" => SectionId::DebugPubTypes,
        ".debug_ranges" => SectionId::DebugRanges,
        ".debug_rnglists" => SectionId::DebugRngLists,
        ".debug_str" => SectionId::DebugStr,
        ".debug_str_offsets" => SectionId::DebugStrOffsets,
        ".debug_types" => SectionId::DebugTypes,
        _ => return None,
    })
}

pub const WASM_HEADER: &[u8] = &[0, b'a', b's', b'm', 1, 0, 0, 0];
