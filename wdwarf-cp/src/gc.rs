use gimli::constants;
use gimli::read;
use gimli::{Reader, UnitSectionOffset};
use std::collections::{HashMap, HashSet};
use std::vec::Vec;

use crate::convert::AddressTranslator;

#[derive(Debug)]
pub struct Dependencies {
    edges: HashMap<UnitSectionOffset, HashSet<UnitSectionOffset>>,
    roots: HashSet<UnitSectionOffset>,
}

impl Dependencies {
    fn new() -> Dependencies {
        Dependencies {
            edges: HashMap::new(),
            roots: HashSet::new(),
        }
    }

    fn add_edge(&mut self, a: UnitSectionOffset, b: UnitSectionOffset) {
        use std::collections::hash_map::Entry;
        match self.edges.entry(a) {
            Entry::Occupied(mut o) => {
                o.get_mut().insert(b);
            }
            Entry::Vacant(v) => {
                let mut set = HashSet::new();
                set.insert(b);
                v.insert(set);
            }
        }
    }

    fn add_root(&mut self, root: UnitSectionOffset) {
        self.roots.insert(root);
    }

    pub fn get_reachable(&self) -> HashSet<UnitSectionOffset> {
        let mut reachable = self.roots.clone();
        let mut queue = Vec::new();
        for i in self.roots.iter() {
            if let Some(deps) = self.edges.get(i) {
                for j in deps {
                    if reachable.contains(j) {
                        continue;
                    }
                    reachable.insert(*j);
                    queue.push(*j);
                }
            }
        }
        while let Some(i) = queue.pop() {
            if let Some(deps) = self.edges.get(&i) {
                for j in deps {
                    if reachable.contains(j) {
                        continue;
                    }
                    reachable.insert(*j);
                    queue.push(*j);
                }
            }
        }
        reachable
    }
}

pub fn build_dependencies<R: Reader<Offset = usize>, A: AddressTranslator>(
    dwarf: &read::Dwarf<R>,
    at: &A,
) -> read::Result<Dependencies> {
    let mut deps = Dependencies::new();
    let mut units = dwarf.units();
    while let Some(unit) = units.next()? {
        build_unit_dependencies(unit, dwarf, at, &mut deps)?;
    }
    Ok(deps)
}

fn build_unit_dependencies<R: Reader<Offset = usize>, A: AddressTranslator>(
    header: read::CompilationUnitHeader<R>,
    dwarf: &read::Dwarf<R>,
    at: &A,
    deps: &mut Dependencies,
) -> read::Result<()> {
    let unit = dwarf.unit(header)?;
    let mut tree = unit.entries_tree(None)?;
    let root = tree.root()?;
    build_die_dependencies(root, dwarf, &unit, at, deps)?;
    Ok(())
}

fn has_die_back_edge<R: Reader<Offset = usize>>(die: &read::DebuggingInformationEntry<R>) -> bool {
    match die.tag() {
        constants::DW_TAG_variable
        | constants::DW_TAG_constant
        | constants::DW_TAG_inlined_subroutine
        | constants::DW_TAG_lexical_block
        | constants::DW_TAG_label
        | constants::DW_TAG_with_stmt
        | constants::DW_TAG_try_block
        | constants::DW_TAG_catch_block
        | constants::DW_TAG_template_type_parameter
        | constants::DW_TAG_member
        | constants::DW_TAG_formal_parameter => true,
        _ => false,
    }
}

fn has_valid_code_range<R: Reader<Offset = usize>, A: AddressTranslator>(
    die: &read::DebuggingInformationEntry<R>,
    at: &A,
) -> read::Result<bool> {
    match die.tag() {
        constants::DW_TAG_subprogram => {
            if let Some(low_pc) = die.attr_value(constants::DW_AT_low_pc)? {
                if let read::AttributeValue::Addr(a) = low_pc {
                    return Ok(at.translate_address(a).len() > 0);
                }
            }
            if let Some(ranges) = die.attr_value(constants::DW_AT_ranges)? {
                // FIXME check ranges
                return Ok(true);
            }
        }
        _ => (),
    }
    Ok(false)
}

fn build_die_dependencies<R: Reader<Offset = usize>, A: AddressTranslator>(
    die: read::EntriesTreeNode<R>,
    dwarf: &read::Dwarf<R>,
    unit: &read::Unit<R>,
    at: &A,
    deps: &mut Dependencies,
) -> read::Result<()> {
    let entry = die.entry();
    let offset = entry.offset().to_unit_section_offset(unit);
    let mut attrs = entry.attrs();
    while let Some(attr) = attrs.next()? {
        build_attr_dependencies(&attr, offset, dwarf, unit, at, deps)?;
    }

    let mut children = die.children();
    while let Some(child) = children.next()? {
        let child_entry = child.entry();
        let child_offset = child_entry.offset().to_unit_section_offset(unit);
        deps.add_edge(child_offset, offset);
        if has_die_back_edge(child_entry) {
            deps.add_edge(offset, child_offset);
        }
        if has_valid_code_range(child_entry, at)? {
            deps.add_root(child_offset);
        }
        build_die_dependencies(child, dwarf, unit, at, deps)?;
    }
    Ok(())
}

fn build_attr_dependencies<R: Reader<Offset = usize>, A: AddressTranslator>(
    attr: &read::Attribute<R>,
    offset: UnitSectionOffset,
    dwarf: &read::Dwarf<R>,
    unit: &read::Unit<R>,
    at: &A,
    deps: &mut Dependencies,
) -> read::Result<()> {
    match attr.value() {
        read::AttributeValue::UnitRef(val) => {
            let ref_offset = val.to_unit_section_offset(unit);
            deps.add_edge(offset, ref_offset);
        }
        read::AttributeValue::DebugInfoRef(val) => {
            let ref_offset = UnitSectionOffset::DebugInfoOffset(val);
            deps.add_edge(offset, ref_offset);
        }
        _ => (),
    }
    Ok(())
}
