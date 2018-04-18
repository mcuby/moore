// Copyright (c) 2018 Fabian Schuiki

//! Arena to allocate HIR nodes into.

use scope2::ScopeData;
use hir::*;

make_arenas!(
    /// An arena to allocate HIR nodes into.
    pub struct Arenas2<'t> {
        scope_data: ScopeData<'t>,

        library: Library<'t>,
        package: Package2<'t>,
        type_decl: TypeDecl2,
        const_decl: ConstDecl<'t>,

        package_slot: Slot<'t, Package2<'t>>,
        type_decl_slot: Slot<'t, TypeDecl2>,
        const_decl_slot: Slot<'t, ConstDecl<'t>>,
    }
);