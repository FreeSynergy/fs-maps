//! `TypeCatalog` interface: the data types of the program, for a generated frontend (ADR 0066).
//!
//! Pattern: Adapter — the descriptions of `fs_maps_core::describe` written onto the wire; the root hands this
//! capability out with `types`. It holds no logic of its own.

use std::rc::Rc;

use fs_interfaces::types_capnp::type_catalog;
use fs_maps_core::describe::{registry, write_catalog};
use fs_types::TypeDescription;

pub(crate) struct CatalogImpl {
    types: Vec<TypeDescription>,
}

impl CatalogImpl {
    pub(crate) fn new() -> Self {
        Self {
            types: registry().iter().cloned().collect(),
        }
    }
}

impl type_catalog::Server for CatalogImpl {
    async fn describe(
        self: Rc<Self>,
        _: type_catalog::DescribeParams,
        mut results: type_catalog::DescribeResults,
    ) -> Result<(), capnp::Error> {
        let n = u32::try_from(self.types.len()).unwrap_or(u32::MAX);
        write_catalog(&self.types, results.get().init_types(n));
        Ok(())
    }
}
