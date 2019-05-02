use gimli::write::Address;
use std::collections::BTreeMap;
use std::vec::Vec;

#[derive(Debug)]
struct Range {
    keypoints: Vec<(usize, usize)>,
    last: usize,
}

#[derive(Debug)]
pub struct AddressMap {
    ranges: Vec<Range>,
}

impl AddressMap {
    pub fn new() -> Self {
        AddressMap { ranges: vec![] }
    }

    fn start_range(&mut self, key: usize, addr: usize) {
        self.ranges.push(Range {
            keypoints: vec![(addr, key)],
            last: key,
        });
    }

    pub fn insert(&mut self, key: usize, addr: usize) {
        if self.ranges.len() == 0 {
            self.start_range(key, addr);
            return;
        }
        let last_range = self.ranges.last_mut().unwrap();
        if last_range.last <= addr {
            last_range.keypoints.push((addr, key));
            last_range.last = key;
            return;
        }
        last_range.last = key;
        self.start_range(key, addr);
    }
}

type AddressMapIndexRanges = Vec<usize>;

#[derive(Debug)]
struct AddressMapIndexed {
    map: AddressMap,
    index: BTreeMap<usize, AddressMapIndexRanges>,
}

impl AddressMapIndexed {
    fn generate_index(map: &AddressMap) -> BTreeMap<usize, AddressMapIndexRanges> {
        // Sorting ranges by first address.
        let mut sorted_map: BTreeMap<usize, (&Range, usize)> = BTreeMap::new();
        for (index, range) in map.ranges.iter().enumerate() {
            let first_addr = range.keypoints.first().unwrap().0;
            sorted_map.insert(first_addr, (range, index));
        }
        // Sweeping all sorted by start address ranges and populating the result as
        // we pass the stored in active_ranges temp values.
        let mut active_ranges: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut result: BTreeMap<usize, AddressMapIndexRanges> = BTreeMap::new();
        for (first_addr, (range, index)) in sorted_map {
            let last_addr = range.keypoints.last().unwrap().0;
            loop {
                // Removing ranges we already passed.
                let addr = *if let Some(addr) = active_ranges.keys().next() {
                    addr
                } else {
                    break;
                };
                if addr >= first_addr {
                    break;
                }
                result.insert(addr, active_ranges.remove(&addr).unwrap());
            }
            if !active_ranges.contains_key(&first_addr) {
                let last = active_ranges.range(..first_addr).last();
                if let Some((_, ranges)) = last {
                    active_ranges.insert(first_addr, ranges.clone());
                } else {
                    active_ranges.insert(first_addr, Vec::new());
                }
            }
            if !active_ranges.contains_key(&last_addr) {
                let last = active_ranges.range(..last_addr).last();
                if let Some((_, ranges)) = last {
                    active_ranges.insert(first_addr, ranges.clone());
                } else {
                    active_ranges.insert(first_addr, Vec::new());
                }
            }
            for (_, ranges) in active_ranges.range_mut(first_addr..=last_addr) {
                ranges.push(index);
            }
        }
        for (addr, ranges) in active_ranges.into_iter() {
            result.insert(addr, ranges);
        }
        result
    }

    fn from(map: AddressMap) -> AddressMapIndexed {
        let index = AddressMapIndexed::generate_index(&map);
        AddressMapIndexed { map, index }
    }

    fn lookup_address(&self, addr: u64) -> Option<u64> {
        let addr = addr as usize;
        let ranges = self.index.range(..=addr).last();
        if ranges.is_none() {
            return None;
        }
        for range_index in ranges.unwrap().1 {
            let range = &self.map.ranges[*range_index];
            let pos = range.keypoints.binary_search_by(|a| a.0.cmp(&addr));
            let result = match pos {
                Ok(i) => range.keypoints[i].1,
                Err(i) => {
                    if i < range.keypoints.len() {
                        range.keypoints[i + 1].1
                    } else {
                        range.last
                    }
                }
            };
            // FIXME iterate on all possible variants.
            return Some(result as u64);
        }
        None
    }
}

fn calc_address_offset(addr1: Address, addr2: Address) -> u64 {
    match (addr1, addr2) {
        (Address::Constant(val1), Address::Constant(val2)) => val2 - val1,
        (
            Address::Symbol {
                symbol: s1,
                addend: a1,
            },
            Address::Symbol {
                symbol: s2,
                addend: a2,
            },
        ) if s1 == s2 => (a2 - a1) as u64,
        _ => panic!("incompatible addresses"),
    }
}

pub trait AddressTranslator {
    fn translate_address(&self, addr: u64) -> Vec<Address>;

    fn translate_range(&self, start: u64, len: u64) -> Vec<(Address, u64)>;

    fn translate_base_address(&self, addr: u64) -> Option<Address> {
        let addresses = self.translate_address(addr);
        if addresses.len() == 0 {
            None
        } else {
            Some(addresses[0])
        }
    }

    fn translate_offset(&self, base: u64, offset: u64) -> Vec<u64> {
        let translated_base = self.translate_base_address(base);
        if translated_base.is_none() {
            return vec![];
        }
        let addresses = self.translate_address(base + offset);
        addresses
            .into_iter()
            .map(|a| calc_address_offset(translated_base.unwrap(), a))
            .collect::<Vec<_>>()
    }

    fn can_translate_address(&self, addr: u64) -> bool {
        self.translate_address(addr).len() > 0
    }
}

pub struct IdentityAddressTranslator(pub bool);

impl AddressTranslator for IdentityAddressTranslator {
    fn translate_address(&self, addr: u64) -> Vec<Address> {
        if addr == 0 && self.0 {
            return vec![];
        }
        vec![Address::Constant(addr)]
    }

    fn translate_range(&self, start: u64, len: u64) -> Vec<(Address, u64)> {
        if start == 0 && self.0 {
            return vec![];
        }
        vec![(Address::Constant(start), len)]
    }
}

pub struct TranformAddressTranslator {
    map: AddressMapIndexed,
}

impl TranformAddressTranslator {
    pub fn new(map: AddressMap) -> Self {
        let map = AddressMapIndexed::from(map);
        TranformAddressTranslator { map }
    }
}

impl AddressTranslator for TranformAddressTranslator {
    fn translate_address(&self, addr: u64) -> Vec<Address> {
        if addr == 0 {
            return vec![];
        }
        if let Some(addr) = self.map.lookup_address(addr) {
            vec![Address::Constant(addr)]
        } else {
            vec![]
        }
    }

    fn translate_range(&self, start: u64, len: u64) -> Vec<(Address, u64)> {
        if start == 0 {
            return vec![];
        }
        if let (Some(start), Some(end)) = (
            self.map.lookup_address(start),
            self.map.lookup_address(start + len),
        ) {
            vec![(Address::Constant(start), end - start)]
        } else {
            vec![]
        }
    }
}
