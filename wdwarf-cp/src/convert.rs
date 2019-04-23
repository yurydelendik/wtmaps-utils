use gimli::constants;
use gimli::read;
use gimli::write::{
    Address, AttributeValue, ConvertError, ConvertResult, DebuggingInformationEntry, Dwarf,
    Expression, FileId, FileInfo, LineProgram, LineString, LineStringTable, Range, RangeList,
    RangeListTable, StringTable, Unit, UnitEntryId, UnitId, UnitTable,
};
use gimli::{DebugLineOffset, DwTag, Reader, UnitSectionOffset};
use std::collections::HashMap;
use std::vec::Vec;

pub trait AddressTranslator {
    fn translate_address(&self, addr: u64) -> Vec<Address>;
    fn translate_range(&self, start: u64, len: u64) -> Vec<(Address, u64)>;
}
// Getting logic from gimli's src/write/{unit,range,line}.rs files.

pub fn from_dwarf<R: Reader<Offset = usize>, A: AddressTranslator>(
    dwarf: &read::Dwarf<R>,
    at: &A,
) -> ConvertResult<Dwarf> {
    let mut line_strings = LineStringTable::default();
    let mut strings = StringTable::default();
    let units = from_unit_table(dwarf, &mut line_strings, &mut strings, at)?;
    // TODO: convert the line programs that were not referenced by a unit.
    let line_programs = Vec::new();
    Ok(Dwarf {
        units,
        line_programs,
        line_strings,
        strings,
    })
}

pub fn from_unit_table<R: Reader<Offset = usize>, A: AddressTranslator>(
    dwarf: &read::Dwarf<R>,
    line_strings: &mut LineStringTable,
    strings: &mut StringTable,
    at: &A,
) -> ConvertResult<UnitTable> {
    let mut units = UnitTable::default();
    let mut unit_entry_offsets = HashMap::new();

    let mut from_units = dwarf.units();
    let mut converted = Vec::new();
    while let Some(from_unit) = from_units.next()? {
        converted.push(from_unit_entry(
            from_unit,
            &mut units,
            &mut unit_entry_offsets,
            dwarf,
            line_strings,
            strings,
            at,
        )?);
    }

    // Convert all DebugInfoOffset to UnitEntryId
    for (unit_id, entries) in converted {
        let unit = units.get_mut(unit_id);
        for entry_id in entries {
            let entry = unit.get_mut(entry_id);
            for attr in &mut entry.attrs_mut() {
                let id = match attr.get() {
                    AttributeValue::UnitSectionRef(ref offset) => {
                        match unit_entry_offsets.get(offset) {
                            Some(id) => Some(*id),
                            None => return Err(ConvertError::InvalidDebugInfoOffset),
                        }
                    }
                    _ => None,
                };
                if let Some(id) = id {
                    if id.0 == unit_id {
                        attr.set(AttributeValue::ThisUnitEntryRef(id.1));
                    } else {
                        attr.set(AttributeValue::AnyUnitEntryRef(id));
                    }
                }
            }
        }
    }

    Ok(units)
}

struct ConvertUnitContext<'a, R: Reader<Offset = usize>, A: AddressTranslator> {
    pub dwarf: &'a read::Dwarf<R>,
    pub unit: &'a read::Unit<R>,
    pub line_strings: &'a mut LineStringTable,
    pub strings: &'a mut StringTable,
    pub ranges: &'a mut RangeListTable,
    pub at: &'a A,
    pub base_address: Address,
    pub line_program_offset: Option<DebugLineOffset>,
    pub line_program_files: Vec<FileId>,
}

fn from_unit_entry<R: Reader<Offset = usize>, A: AddressTranslator>(
    from_header: read::CompilationUnitHeader<R>,
    units: &mut UnitTable,
    unit_entry_offsets: &mut HashMap<UnitSectionOffset, (UnitId, UnitEntryId)>,
    dwarf: &read::Dwarf<R>,
    line_strings: &mut LineStringTable,
    strings: &mut StringTable,
    at: &A,
) -> ConvertResult<(UnitId, Vec<UnitEntryId>)> {
    let from_unit = dwarf.unit(from_header)?;
    let encoding = from_unit.encoding();
    let base_address = *at
        .translate_address(from_unit.low_pc)
        .get(0)
        .ok_or(ConvertError::InvalidAddress)?;

    let (line_program_offset, line_program, line_program_files) = match from_unit.line_program {
        Some(ref from_program) => {
            let from_program = from_program.clone();
            let line_program_offset = from_program.header().offset();
            let (line_program, line_program_files) =
                from_line_program(from_program, dwarf, line_strings, strings, at)?;
            (Some(line_program_offset), line_program, line_program_files)
        }
        None => (None, LineProgram::none(), Vec::new()),
    };

    let unit = Unit::new(encoding, line_program);
    let unit_id = units.add(unit);
    let mut unit = units.get_mut(unit_id);
    let mut entries = Vec::new();

    let mut ranges = RangeListTable::default();
    let mut context = ConvertUnitContext {
        dwarf,
        unit: &from_unit,
        line_strings,
        strings,
        ranges: &mut ranges,
        at,
        base_address,
        line_program_offset,
        line_program_files,
    };
    let mut from_tree = from_unit.entries_tree(None)?;
    let from_root = from_tree.root()?;
    let root_id = unit.root();
    entries.push(root_id);
    from_die(
        &mut context,
        from_root,
        &mut unit,
        unit_id,
        root_id,
        &mut entries,
        unit_entry_offsets,
    )?;

    Ok((unit_id, entries))
}

fn get_tag<R: Reader<Offset = usize>>(from: &read::EntriesTreeNode<R>) -> DwTag {
    let from = from.entry();
    from.tag()
}

fn from_die<R: Reader<Offset = usize>, A: AddressTranslator>(
    context: &mut ConvertUnitContext<R, A>,
    from: read::EntriesTreeNode<R>,
    unit: &mut Unit,
    unit_id: UnitId,
    entry_id: UnitEntryId,
    entries: &mut Vec<UnitEntryId>,
    unit_entry_offsets: &mut HashMap<UnitSectionOffset, (UnitId, UnitEntryId)>,
) -> ConvertResult<()> {
    {
        let from = from.entry();
        let entry = unit.get_mut(entry_id);

        let offset = from.offset().to_unit_section_offset(context.unit);
        unit_entry_offsets.insert(offset, (unit_id, entry_id));

        let mut from_attrs = from.attrs();
        while let Some(from_attr) = from_attrs.next()? {
            if from_attr.name() == constants::DW_AT_sibling {
                // This may point to a null entry, so we have to treat it differently.
                entry.set_sibling(true);
            } else {
                from_entry_attr(context, &from_attr, entry)?;
            }
        }
    }

    let mut from_children = from.children();
    while let Some(from_child) = from_children.next()? {
        let child_id = unit.add(entry_id, get_tag(&from_child));
        entries.push(child_id);
        from_die(
            context,
            from_child,
            unit,
            unit_id,
            child_id,
            entries,
            unit_entry_offsets,
        )?;
    }
    Ok(())
}

fn from_entry_attr<R: Reader<Offset = usize>, A: AddressTranslator>(
    context: &mut ConvertUnitContext<R, A>,
    from: &read::Attribute<R>,
    entry: &mut DebuggingInformationEntry,
) -> ConvertResult<()> {
    if let Some(value) = from_attr_value(context, from.value())? {
        entry.set(from.name(), value);
    }
    Ok(())
}

fn from_attr_value<R: Reader<Offset = usize>, A: AddressTranslator>(
    context: &mut ConvertUnitContext<R, A>,
    from: read::AttributeValue<R>,
) -> ConvertResult<Option<AttributeValue>> {
    let to = match from {
        read::AttributeValue::Addr(val) => match context.at.translate_address(val).get(0) {
            Some(val) => AttributeValue::Address(*val),
            None => return Err(ConvertError::InvalidAddress),
        },
        read::AttributeValue::Block(r) => AttributeValue::Block(r.to_slice()?.into()),
        read::AttributeValue::Data1(val) => AttributeValue::Data1(val),
        read::AttributeValue::Data2(val) => AttributeValue::Data2(val),
        read::AttributeValue::Data4(val) => AttributeValue::Data4(val),
        read::AttributeValue::Data8(val) => AttributeValue::Data8(val),
        read::AttributeValue::Sdata(val) => AttributeValue::Sdata(val),
        read::AttributeValue::Udata(val) => AttributeValue::Udata(val),
        // TODO: addresses and offsets in expressions need special handling.
        read::AttributeValue::Exprloc(read::Expression(val)) => {
            AttributeValue::Exprloc(Expression(val.to_slice()?.into()))
        }
        // TODO: it would be nice to preserve the flag form.
        read::AttributeValue::Flag(val) => AttributeValue::Flag(val),
        read::AttributeValue::DebugAddrBase(_base) => {
            // We convert all address indices to addresses,
            // so this is unneeded.
            return Ok(None);
        }
        read::AttributeValue::DebugAddrIndex(index) => {
            let val = context.dwarf.address(context.unit, index)?;
            match context.at.translate_address(val).get(0) {
                Some(val) => AttributeValue::Address(*val),
                None => return Err(ConvertError::InvalidAddress),
            }
        }
        read::AttributeValue::UnitRef(val) => {
            AttributeValue::UnitSectionRef(val.to_unit_section_offset(context.unit))
        }
        read::AttributeValue::DebugInfoRef(val) => {
            AttributeValue::UnitSectionRef(UnitSectionOffset::DebugInfoOffset(val))
        }
        read::AttributeValue::DebugInfoRefSup(val) => AttributeValue::DebugInfoRefSup(val),
        read::AttributeValue::DebugLineRef(val) => {
            // There should only be the line program in the CU DIE which we've already
            // converted, so check if it matches that.
            if Some(val) == context.line_program_offset {
                AttributeValue::LineProgramRef
            } else {
                return Err(ConvertError::InvalidLineRef);
            }
        }
        read::AttributeValue::DebugMacinfoRef(val) => AttributeValue::DebugMacinfoRef(val),
        read::AttributeValue::LocationListsRef(val) => AttributeValue::LocationListsRef(val),
        read::AttributeValue::DebugLocListsBase(_base) => {
            // We convert all location list indices to offsets,
            // so this is unneeded.
            return Ok(None);
        }
        read::AttributeValue::DebugLocListsIndex(index) => {
            let offset = context.dwarf.locations_offset(context.unit, index)?;
            AttributeValue::LocationListsRef(offset)
        }
        read::AttributeValue::RangeListsRef(val) => {
            let iter = context
                .dwarf
                .ranges
                .raw_ranges(val, context.unit.encoding())?;
            let range_list = from_rangelist(iter, context)?;
            let range_id = context.ranges.add(range_list);
            AttributeValue::RangeListRef(range_id)
        }
        read::AttributeValue::DebugRngListsBase(_base) => {
            // We convert all range list indices to offsets,
            // so this is unneeded.
            return Ok(None);
        }
        read::AttributeValue::DebugRngListsIndex(index) => {
            let offset = context.dwarf.ranges_offset(context.unit, index)?;
            let iter = context
                .dwarf
                .ranges
                .raw_ranges(offset, context.unit.encoding())?;
            let range_list = from_rangelist(iter, context)?;
            let range_id = context.ranges.add(range_list);
            AttributeValue::RangeListRef(range_id)
        }
        read::AttributeValue::DebugTypesRef(val) => AttributeValue::DebugTypesRef(val),
        read::AttributeValue::DebugStrRef(offset) => {
            let r = context.dwarf.string(offset)?;
            let id = context.strings.add(r.to_slice()?);
            AttributeValue::StringRef(id)
        }
        read::AttributeValue::DebugStrRefSup(val) => AttributeValue::DebugStrRefSup(val),
        read::AttributeValue::DebugStrOffsetsBase(_base) => {
            // We convert all string offsets to `.debug_str` references,
            // so this is unneeded.
            return Ok(None);
        }
        read::AttributeValue::DebugStrOffsetsIndex(index) => {
            let offset = context.dwarf.string_offset(context.unit, index)?;
            let r = context.dwarf.string(offset)?;
            let id = context.strings.add(r.to_slice()?);
            AttributeValue::StringRef(id)
        }
        read::AttributeValue::DebugLineStrRef(offset) => {
            let r = context.dwarf.line_string(offset)?;
            let id = context.line_strings.add(r.to_slice()?);
            AttributeValue::LineStringRef(id)
        }
        read::AttributeValue::String(r) => AttributeValue::String(r.to_slice()?.into()),
        read::AttributeValue::Encoding(val) => AttributeValue::Encoding(val),
        read::AttributeValue::DecimalSign(val) => AttributeValue::DecimalSign(val),
        read::AttributeValue::Endianity(val) => AttributeValue::Endianity(val),
        read::AttributeValue::Accessibility(val) => AttributeValue::Accessibility(val),
        read::AttributeValue::Visibility(val) => AttributeValue::Visibility(val),
        read::AttributeValue::Virtuality(val) => AttributeValue::Virtuality(val),
        read::AttributeValue::Language(val) => AttributeValue::Language(val),
        read::AttributeValue::AddressClass(val) => AttributeValue::AddressClass(val),
        read::AttributeValue::IdentifierCase(val) => AttributeValue::IdentifierCase(val),
        read::AttributeValue::CallingConvention(val) => AttributeValue::CallingConvention(val),
        read::AttributeValue::Inline(val) => AttributeValue::Inline(val),
        read::AttributeValue::Ordering(val) => AttributeValue::Ordering(val),
        read::AttributeValue::FileIndex(val) => {
            if val == 0 {
                // 0 means not specified, even for version 5.
                AttributeValue::FileIndex(None)
            } else {
                match context.line_program_files.get(val as usize - 1) {
                    Some(id) => AttributeValue::FileIndex(Some(*id)),
                    None => return Err(ConvertError::InvalidFileIndex),
                }
            }
        }
        // Should always be a more specific section reference.
        read::AttributeValue::SecOffset(_) => {
            return Err(ConvertError::InvalidAttributeValue);
        }
    };
    Ok(Some(to))
}

fn from_rangelist<R: Reader<Offset = usize>, A: AddressTranslator>(
    mut from: read::RawRngListIter<R>,
    context: &ConvertUnitContext<R, A>,
) -> ConvertResult<RangeList> {
    let mut have_base_address = context.base_address != Address::Absolute(0);
    let convert_address = |x| {
        if let Some(addr) = context.at.translate_address(x).get(0) {
            Ok(*addr)
        } else {
            Err(ConvertError::InvalidAddress)
        }
    };
    let mut ranges = Vec::new();
    while let Some(from_range) = from.next()? {
        let range = match from_range {
            read::RawRngListEntry::AddressOrOffsetPair { begin, end } => {
                // These were parsed as addresses, even if they are offsets.
                let begin = convert_address(begin)?;
                let end = convert_address(end)?;
                match (begin, end) {
                    (Address::Absolute(begin_offset), Address::Absolute(end_offset)) => {
                        if have_base_address {
                            Range::OffsetPair {
                                begin: begin_offset,
                                end: end_offset,
                            }
                        } else {
                            Range::StartEnd { begin, end }
                        }
                    }
                    _ => {
                        if have_base_address {
                            // At least one of begin/end is an address, but we also have
                            // a base address. Adding addresses is undefined.
                            return Err(ConvertError::InvalidRangeRelativeAddress);
                        }
                        Range::StartEnd { begin, end }
                    }
                }
            }
            read::RawRngListEntry::BaseAddress { addr } => {
                have_base_address = true;
                let address = convert_address(addr)?;
                Range::BaseAddress { address }
            }
            read::RawRngListEntry::BaseAddressx { addr } => {
                have_base_address = true;
                let address = convert_address(context.dwarf.address(context.unit, addr)?)?;
                Range::BaseAddress { address }
            }
            read::RawRngListEntry::StartxEndx { begin, end } => {
                let begin = convert_address(context.dwarf.address(context.unit, begin)?)?;
                let end = convert_address(context.dwarf.address(context.unit, end)?)?;
                Range::StartEnd { begin, end }
            }
            read::RawRngListEntry::StartxLength { begin, length } => {
                let begin = convert_address(context.dwarf.address(context.unit, begin)?)?;
                Range::StartLength { begin, length }
            }
            read::RawRngListEntry::OffsetPair { begin, end } => Range::OffsetPair { begin, end },
            read::RawRngListEntry::StartEnd { begin, end } => {
                let begin = convert_address(begin)?;
                let end = convert_address(end)?;
                Range::StartEnd { begin, end }
            }
            read::RawRngListEntry::StartLength { begin, length } => {
                let begin = convert_address(begin)?;
                Range::StartLength { begin, length }
            }
        };
        ranges.push(range);
    }
    Ok(RangeList(ranges))
}

fn from_line_program<R: Reader<Offset = usize>, A: AddressTranslator>(
    mut from_program: read::IncompleteLineProgram<R>,
    dwarf: &read::Dwarf<R>,
    line_strings: &mut LineStringTable,
    strings: &mut StringTable,
    at: &A,
) -> ConvertResult<(LineProgram, Vec<FileId>)> {
    // Create mappings in case the source has duplicate files or directories.
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    let mut program = {
        let from_header = from_program.header();

        let comp_dir = from_header
            .directory(0)
            .ok_or(ConvertError::MissingCompilationDirectory)?;
        let comp_dir = from_line_string(comp_dir, dwarf, line_strings, strings)?;

        let comp_file = from_header
            .file(0)
            .ok_or(ConvertError::MissingCompilationFile)?;
        let comp_name = from_line_string(comp_file.path_name(), dwarf, line_strings, strings)?;
        if comp_file.directory_index() != 0 {
            return Err(ConvertError::InvalidDirectoryIndex);
        }
        let comp_file_info = FileInfo {
            timestamp: comp_file.timestamp(),
            size: comp_file.size(),
            md5: *comp_file.md5(),
        };

        if from_header.line_base() > 0 {
            return Err(ConvertError::InvalidLineBase);
        }
        let mut program = LineProgram::new(
            from_header.encoding(),
            from_header.line_encoding(),
            comp_dir,
            comp_name,
            Some(comp_file_info),
        );

        let file_skip;
        if from_header.version() <= 4 {
            // The first directory is implicit.
            dirs.push(program.default_directory());
            // A file index of 0 is invalid for version <= 4, but putting
            // something there makes the indexing easier.
            file_skip = 0;
        // FIXME files.push(FileId::zero());
        } else {
            // We don't add the first file to `files`, but still allow
            // it to be referenced from converted instructions.
            file_skip = 1;
            // FIXME files.push(FileId::zero());
        }

        for from_dir in from_header.include_directories() {
            let from_dir = from_line_string(from_dir.clone(), dwarf, line_strings, strings)?;
            dirs.push(program.add_directory(from_dir));
        }

        program.file_has_timestamp = from_header.file_has_timestamp();
        program.file_has_size = from_header.file_has_size();
        program.file_has_md5 = from_header.file_has_md5();
        for from_file in from_header.file_names().iter().skip(file_skip) {
            let from_name = from_line_string(from_file.path_name(), dwarf, line_strings, strings)?;
            let from_dir = from_file.directory_index();
            if from_dir >= dirs.len() as u64 {
                return Err(ConvertError::InvalidDirectoryIndex);
            }
            let from_dir = dirs[from_dir as usize];
            let from_info = Some(FileInfo {
                timestamp: from_file.timestamp(),
                size: from_file.size(),
                md5: *from_file.md5(),
            });
            files.push(program.add_file(from_name, from_dir, from_info));
        }

        program
    };

    // We can't use the `from_program.rows()` because that wouldn't let
    // us preserve address relocations.
    let mut from_row = read::LineRow::new(from_program.header());
    let mut instructions = from_program.header().instructions();
    let mut address = None;
    while let Some(instruction) = instructions.next_instruction(from_program.header())? {
        match instruction {
            read::LineInstruction::SetAddress(val) => {
                if program.in_sequence() {
                    return Err(ConvertError::UnsupportedLineInstruction);
                }
                match at.translate_address(val).get(0) {
                    Some(val) => address = Some(*val),
                    None => return Err(ConvertError::InvalidAddress),
                }
                from_row.execute(read::LineInstruction::SetAddress(0), &mut from_program);
            }
            read::LineInstruction::DefineFile(_) => {
                return Err(ConvertError::UnsupportedLineInstruction);
            }
            _ => {
                if from_row.execute(instruction, &mut from_program) {
                    if !program.in_sequence() {
                        program.begin_sequence(address);
                        address = None;
                    }
                    if from_row.end_sequence() {
                        program.end_sequence(from_row.address());
                    } else {
                        program.row().address_offset = from_row.address();
                        program.row().op_index = from_row.op_index();
                        program.row().file = {
                            let file = from_row.file_index();
                            if file > files.len() as u64 {
                                return Err(ConvertError::InvalidFileIndex);
                            }
                            if file == 0 && program.version() <= 4 {
                                return Err(ConvertError::InvalidFileIndex);
                            }
                            assert!(file > 0, "not implemented for versio 5's file == 0");
                            files[file as usize - 1]
                        };
                        program.row().line = from_row.line().unwrap_or(0);
                        program.row().column = match from_row.column() {
                            read::ColumnType::LeftEdge => 0,
                            read::ColumnType::Column(val) => val,
                        };
                        program.row().discriminator = from_row.discriminator();
                        program.row().is_statement = from_row.is_stmt();
                        program.row().basic_block = from_row.basic_block();
                        program.row().prologue_end = from_row.prologue_end();
                        program.row().epilogue_begin = from_row.epilogue_begin();
                        program.row().isa = from_row.isa();
                        program.generate_row();
                    }
                    from_row.reset(from_program.header());
                }
            }
        };
    }
    Ok((program, files))
}

fn from_line_string<R: Reader<Offset = usize>>(
    from_attr: read::AttributeValue<R>,
    dwarf: &read::Dwarf<R>,
    line_strings: &mut LineStringTable,
    strings: &mut StringTable,
) -> ConvertResult<LineString> {
    Ok(match from_attr {
        read::AttributeValue::String(r) => LineString::String(r.to_slice()?.to_vec()),
        read::AttributeValue::DebugStrRef(offset) => {
            let r = dwarf.debug_str.get_str(offset)?;
            let id = strings.add(r.to_slice()?);
            LineString::StringRef(id)
        }
        read::AttributeValue::DebugLineStrRef(offset) => {
            let r = dwarf.debug_line_str.get_str(offset)?;
            let id = line_strings.add(r.to_slice()?);
            LineString::LineStringRef(id)
        }
        _ => return Err(ConvertError::UnsupportedLineStringForm),
    })
}
