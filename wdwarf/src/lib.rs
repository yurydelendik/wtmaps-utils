mod address_translator;
mod convert;
mod gc;

pub use address_translator::{
    AddressMap, AddressTranslator, IdentityAddressTranslator, OriginalAddress, TargetAddress,
    TranformAddressTranslator,
};
pub use convert::from_dwarf;
pub use gc::build_dependencies;
