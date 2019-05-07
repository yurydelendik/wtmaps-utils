mod address_translator;
mod convert;
mod gc;
mod wasm;

pub use address_translator::{
    AddressMap, AddressTranslator, IdentityAddressTranslator, OriginalAddress, TargetAddress,
    TranformAddressTranslator,
};
pub use convert::from_dwarf;
pub use gc::build_dependencies;
pub use wasm::{create_dwarf_sections, read_dwarf};
