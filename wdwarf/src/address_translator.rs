use gimli::write::Address;
use std::collections::{BTreeMap, BTreeSet};
use std::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetAddress(pub u64);

impl Into<u64> for TargetAddress {
    fn into(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OriginalAddress(pub u64);

impl Into<u64> for OriginalAddress {
    fn into(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
struct Range {
    keypoints: Vec<(OriginalAddress, TargetAddress)>,
    last: TargetAddress,
}

#[derive(Debug)]
pub struct AddressMap {
    ranges: Vec<Range>,
}

impl AddressMap {
    pub fn new() -> Self {
        AddressMap { ranges: vec![] }
    }

    fn start_range(&mut self, key: TargetAddress, addr: OriginalAddress) {
        self.ranges.push(Range {
            keypoints: vec![(addr, key)],
            last: key,
        });
    }

    pub fn insert(&mut self, key: TargetAddress, addr: OriginalAddress) {
        if self.ranges.len() == 0 {
            self.start_range(key, addr);
            return;
        }
        let last_range = self.ranges.last_mut().unwrap();
        if last_range.keypoints.last().unwrap().0 <= addr {
            last_range.keypoints.push((addr, key));
            last_range.last = key;
            return;
        }
        last_range.last = key;
        self.start_range(key, addr);
    }
}

type AddressMapIndexRanges = Vec<usize>;
type TargetAddressRange = std::ops::Range<TargetAddress>;

fn to_addr_len(range: &TargetAddressRange) -> (Address, u64) {
    let start: u64 = range.start.into();
    let end: u64 = range.end.into();
    (Address::Constant(start), end - start)
}

#[derive(Debug)]
struct AddressMapIndexed {
    map: AddressMap,
    index: BTreeMap<OriginalAddress, AddressMapIndexRanges>,
    function_ranges: Box<[TargetAddressRange]>,
}

enum LookupAddressIterator<'a> {
    Empty,
    Mapped {
        map: &'a AddressMap,
        range_indicies: &'a [usize],
        addr: OriginalAddress,
    },
}

impl<'a> Iterator for LookupAddressIterator<'a> {
    type Item = TargetAddress;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            LookupAddressIterator::Empty => None,
            LookupAddressIterator::Mapped {
                map,
                range_indicies,
                addr,
            } => {
                while range_indicies.len() > 0 {
                    let range_index = range_indicies[0];
                    *range_indicies = &range_indicies[1..];
                    let range = &map.ranges[range_index];
                    let pos = range.keypoints.binary_search_by(|a| a.0.cmp(&addr));
                    let result = match pos {
                        Ok(i) => range.keypoints[i].1,
                        Err(i) => {
                            if i < range.keypoints.len() {
                                range.keypoints[i].1
                            } else {
                                range.last
                            }
                        }
                    };
                    return Some(result);
                }
                None
            }
        }
    }
}

struct LookupRangeIterator<'a> {
    map: &'a AddressMap,
    ranges: BTreeSet<usize>,
    start: OriginalAddress,
    end: OriginalAddress,
}

impl<'a> Iterator for LookupRangeIterator<'a> {
    type Item = TargetAddressRange;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.ranges.is_empty() {
            let range_index = *self.ranges.iter().next().unwrap();
            self.ranges.take(&range_index);
            let range = &self.map.ranges[range_index];
            let start = {
                let pos = range.keypoints.binary_search_by(|a| a.0.cmp(&self.start));
                match pos {
                    Ok(i) => range.keypoints[i].1,
                    Err(i) => {
                        if i < range.keypoints.len() {
                            range.keypoints[i].1
                        } else {
                            range.last
                        }
                    }
                }
            };
            let end = {
                let pos = range.keypoints.binary_search_by(|a| a.0.cmp(&self.end));
                match pos {
                    Ok(i) => range.keypoints[i].1,
                    Err(i) => {
                        if i < range.keypoints.len() {
                            range.keypoints[i].1
                        } else {
                            range.last
                        }
                    }
                }
            };
            // Skip empty ranges
            if start.0 < end.0 {
                return Some(start..end);
            }
        }
        None
    }
}

impl AddressMapIndexed {
    fn generate_index(map: &AddressMap) -> BTreeMap<OriginalAddress, AddressMapIndexRanges> {
        // Sorting ranges by first address.
        let mut sorted_map: BTreeMap<OriginalAddress, (&Range, usize)> = BTreeMap::new();
        for (index, range) in map.ranges.iter().enumerate() {
            let first_addr = range.keypoints.first().unwrap().0;
            sorted_map.insert(first_addr, (range, index));
        }
        // Sweeping all sorted by start address ranges and populating the result as
        // we pass the stored in active_ranges temp values.
        let mut active_ranges: BTreeMap<OriginalAddress, Vec<usize>> = BTreeMap::new();
        let mut result: BTreeMap<OriginalAddress, AddressMapIndexRanges> = BTreeMap::new();
        for (first_addr, (range, index)) in sorted_map {
            let last_addr = range.keypoints.last().unwrap().0;
            loop {
                // Removing ranges we already passed.
                let addr: OriginalAddress = *if let Some(addr) = active_ranges.keys().next() {
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
                let ranges = if let Some((_, ranges)) = last {
                    ranges.clone()
                } else {
                    Vec::new()
                };
                active_ranges.insert(first_addr, ranges);
            }
            if !active_ranges.contains_key(&last_addr) {
                let last = active_ranges.range(..last_addr).last();
                let ranges = if let Some((_, ranges)) = last {
                    ranges.clone()
                } else {
                    Vec::new()
                };
                active_ranges.insert(first_addr, ranges);
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

    fn from(map: AddressMap, mut function_ranges: Box<[(u64, u64)]>) -> AddressMapIndexed {
        function_ranges.sort();
        let function_ranges = function_ranges
            .into_iter()
            .map(|(b, e)| TargetAddress(*b)..TargetAddress(*e))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let index = AddressMapIndexed::generate_index(&map);
        AddressMapIndexed {
            map,
            index,
            function_ranges,
        }
    }

    fn lookup_function_range_by_target_address(
        &self,
        addr: TargetAddress,
    ) -> Option<&TargetAddressRange> {
        match self
            .function_ranges
            .binary_search_by(|x| x.start.cmp(&addr))
        {
            // Landed on start of the function range -- simple.
            Ok(i) => return Some(&self.function_ranges[i]),
            Err(i) => {
                // Check if previous range contains the addr
                if i > 0
                    && self.function_ranges[i - 1].start <= addr
                    && addr < self.function_ranges[i - 1].end
                {
                    return Some(&self.function_ranges[i - 1]);
                }
            }
        }
        None
    }

    fn lookup_function_range(&self, addrs: &[OriginalAddress]) -> Option<&TargetAddressRange> {
        // The function range is found if one of TargetAddress in the function range.
        for addr in addrs {
            let ranges = self.index.range(..=addr).last();
            if ranges.is_none() {
                break;
            }
            for range_index in ranges.unwrap().1 {
                let range = &self.map.ranges[*range_index];
                let pos = range.keypoints.binary_search_by(|x| x.0.cmp(&addr));
                match pos {
                    Ok(i) => {
                        // Check if keypoint's target address located in the function range.
                        if let Some(range) =
                            self.lookup_function_range_by_target_address(range.keypoints[i].1)
                        {
                            return Some(range);
                        }
                    }
                    Err(i) => {
                        // Not found the exact keypoint.
                        if i == 0 {
                            assert!(i < range.keypoints.len());
                            // No left boundary to check, assuming it is the same as the next
                            // keypoint's target address function range.
                            if let Some(range) =
                                self.lookup_function_range_by_target_address(range.keypoints[i].1)
                            {
                                return Some(range);
                            }
                        } else if let Some(left_range) =
                            self.lookup_function_range_by_target_address(range.keypoints[i - 1].1)
                        {
                            if i >= range.keypoints.len() {
                                // No right boundary, but we already found the left keypoint has
                                // the function range -- using that.
                                return Some(left_range);
                            } else if let Some(right_range) =
                                self.lookup_function_range_by_target_address(range.keypoints[i].1)
                            {
                                // We prefer right function range
                                return Some(right_range);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn lookup_address(&self, addr: OriginalAddress) -> LookupAddressIterator {
        let ranges = self.index.range(..=addr).last();
        if ranges.is_none() {
            return LookupAddressIterator::Empty;
        }
        LookupAddressIterator::Mapped {
            map: &self.map,
            range_indicies: &ranges.unwrap().1,
            addr,
        }
    }

    fn lookup_range(&self, start: OriginalAddress, end: OriginalAddress) -> LookupRangeIterator {
        use std::ops::Bound::*;
        let index_range = (
            match self.index.range(..=start).last() {
                Some((start, _)) => Included(*start),
                None => Unbounded,
            },
            Included(end),
        );
        let mut ranges = BTreeSet::new();
        for range in self.index.range(index_range) {
            range.1.iter().for_each(|i| {
                ranges.insert(*i);
            });
        }
        LookupRangeIterator {
            map: &self.map,
            ranges,
            start,
            end,
        }
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

fn compare_addresses(addr1: &Address, addr2: &Address) -> std::cmp::Ordering {
    match (addr1, addr2) {
        (Address::Constant(val1), Address::Constant(val2)) => val1.cmp(val2),
        (
            Address::Symbol {
                symbol: s1,
                addend: a1,
            },
            Address::Symbol {
                symbol: s2,
                addend: a2,
            },
        ) if s1 == s2 => a1.cmp(a2),
        _ => panic!("incompatible addresses"),
    }
}

pub trait AddressTranslator {
    fn translate_address(&self, addr: u64) -> Vec<Address>;

    fn translate_range(&self, start: u64, len: u64) -> Vec<(Address, u64)>;

    fn translate_function_range(&self, start: u64, len: u64) -> Option<(Address, u64)>;

    fn translate_base_address(&self, addr: u64) -> Option<Address> {
        let addresses = self.translate_address(addr);
        addresses.into_iter().min_by(compare_addresses)
    }

    fn translate_offset(&self, base: u64, offset: u64) -> Vec<u64> {
        let translated_base = self.translate_base_address(base);
        if translated_base.is_none() {
            return vec![];
        }
        let addresses = self.translate_address(base + offset);
        addresses
            .into_iter()
            .filter(|a| {
                match compare_addresses(translated_base.as_ref().unwrap(), &a) {
                    std::cmp::Ordering::Greater => {
                        eprintln!("TODO: translated_base.as_ref().unwrap() <= a: {:?} > {:?} (base: {}, offset: {})", translated_base.unwrap(), a, base, offset);
                        false
                    }
                    _ => true,
                }
            })
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

    fn translate_function_range(&self, start: u64, len: u64) -> Option<(Address, u64)> {
        if start == 0 && self.0 {
            return None;
        }
        Some((Address::Constant(start), len))
    }
}

pub struct TranformAddressTranslator {
    map: AddressMapIndexed,
}

impl TranformAddressTranslator {
    pub fn new(map: AddressMap, function_ranges: Box<[(u64, u64)]>) -> Self {
        let map = AddressMapIndexed::from(map, function_ranges);
        TranformAddressTranslator { map }
    }
}

fn from_target_address(addr: TargetAddress) -> Address {
    Address::Constant(addr.0.into())
}

impl AddressTranslator for TranformAddressTranslator {
    fn translate_base_address(&self, addr: u64) -> Option<Address> {
        if addr == 0 {
            return None;
        }
        self.map
            .lookup_address(OriginalAddress(addr))
            .next()
            .map(from_target_address)
    }

    fn translate_address(&self, addr: u64) -> Vec<Address> {
        if addr == 0 {
            return vec![];
        }
        self.map
            .lookup_address(OriginalAddress(addr))
            .map(from_target_address)
            .collect()
    }

    fn translate_range(&self, start: u64, len: u64) -> Vec<(Address, u64)> {
        if start == 0 {
            return vec![];
        }
        let mut it = self
            .map
            .lookup_range(OriginalAddress(start), OriginalAddress(start + len));
        let mut current = if let Some(r) = it.next() {
            r
        } else {
            return vec![];
        };
        // Merge two ranges if needed.
        let mut result = Vec::new();
        while let Some(next) = it.next() {
            if current.end == next.start {
                current.end = next.end;
            } else {
                result.push(to_addr_len(&current));
                current = next;
            }
        }
        result.push(to_addr_len(&current));
        result
    }

    fn translate_function_range(&self, start: u64, len: u64) -> Option<(Address, u64)> {
        let addrs = if len == 0 {
            vec![OriginalAddress(start)]
        } else {
            vec![OriginalAddress(start), OriginalAddress(start + len - 1)]
        };
        self.map
            .lookup_function_range(&addrs)
            .map(|p| to_addr_len(p))
    }
}
