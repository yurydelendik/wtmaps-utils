use gimli::{self, Dwarf, SectionId};
use std::collections::HashMap;
use wasmparser::{ModuleReader, SectionCode};

pub fn read_dwarf<'a>(bin: &'a [u8]) -> Dwarf<gimli::EndianSlice<'a, gimli::LittleEndian>> {
    let sections = read_dwarf_sections(bin);
    let empty = &bin[0..0];
    Dwarf::load(
        |section_id| -> Result<gimli::EndianSlice<_>, gimli::Error> {
            Ok(if let Some(buf) = sections.get(section_id.name()) {
                gimli::EndianSlice::new(buf, gimli::LittleEndian)
            } else {
                gimli::EndianSlice::new(empty, gimli::LittleEndian)
            })
        },
        |_| -> Result<gimli::EndianSlice<_>, gimli::Error> {
            Ok(gimli::EndianSlice::new(empty, gimli::LittleEndian))
        },
    )
    .expect("dwarf")
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
