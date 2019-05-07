use gimli::write::{self, EndianVec, Sections};
use gimli::{self, Dwarf};
use std::collections::HashMap;

pub fn read_dwarf<'input, 'a>(
    sections: HashMap<&'input str, &'input [u8]>,
) -> gimli::Result<Dwarf<gimli::EndianSlice<'a, gimli::LittleEndian>>>
where
    'input: 'a,
{
    let empty: &'static [u8] = &[];
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
}

fn write_leb128(out: &mut Vec<u8>, mut value: u32) {
    for _ in 0..5 {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }

        out.push(byte);

        if value == 0 {
            break;
        }
    }
}

pub fn create_dwarf_sections(dwarf: &mut write::Dwarf) -> write::Result<Vec<u8>> {
    let mut sections = Sections::new(EndianVec::new(gimli::LittleEndian));
    dwarf.write(&mut sections)?;

    let mut result = Vec::new();
    sections.for_each(|s, w| -> write::Result<()> {
        let mut section = Vec::new();
        let name = s.name().as_bytes();
        write_leb128(&mut section, name.len() as u32);
        section.extend_from_slice(name);
        let body = w.slice();
        section.extend_from_slice(body);

        write_leb128(&mut result, 0);
        write_leb128(&mut result, section.len() as u32);
        result.extend_from_slice(&section);
        Ok(())
    })?;
    Ok(result)
}
