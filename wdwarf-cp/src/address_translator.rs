use gimli::write::Address;
use std::vec::Vec;

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
