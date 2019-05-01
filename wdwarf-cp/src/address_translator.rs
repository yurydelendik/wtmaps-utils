use crate::convert;
use gimli::write;
use std::vec::Vec;

pub struct AddressTranslator(pub bool);

impl convert::AddressTranslator for AddressTranslator {
    fn translate_address(&self, addr: u64) -> Vec<write::Address> {
        if addr == 0 && self.0 {
            return vec![];
        }
        vec![write::Address::Constant(addr)]
    }

    fn translate_range(&self, start: u64, len: u64) -> Vec<(write::Address, u64)> {
        if start == 0 && self.0 {
            return vec![];
        }
        vec![(write::Address::Constant(start), len)]
    }
}
