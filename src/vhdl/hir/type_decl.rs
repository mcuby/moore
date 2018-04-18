// Copyright (c) 2018 Fabian Schuiki

//! Type and subtype declarations

#![allow(unused_variables)]
#![allow(unused_imports)]

use hir::prelude::*;
use hir::{EnumLit, Range};
use term::TermContext;

/// A type declaration.
///
/// See IEEE 1076-2008 section 6.2.
#[derive(Debug)]
pub struct TypeDecl2 {
    span: Span,
    name: Spanned<Name>,
    data: TypeData,
}

/// The meat of a type declaration.
#[derive(Debug)]
enum TypeData {
    /// An incomplete type declaration.
    Incomplete,
    // /// An enumeration type.
    // Enum(Vec<EnumLit>),
    // /// An integer or floating point type.
    // Range(Range),
}

impl<'t> FromAst<'t> for TypeDecl2 {
    type AllocInput = &'t ast::TypeDecl;
    type LatentInput = Self::AllocInput;
    type Context = AllocContext<'t>;
    type Latent = &'t Slot<'t, Self>;

    fn alloc_slot(ast: Self::AllocInput, context: Self::Context) -> Result<Self::Latent> {
        let slot = context.alloc(Slot::new(ast, context));
        // TODO: Make the definition weak such that an actual type definition
        // may override it.
        context.define(ast.name.map_into(), Def2::Type(slot))?;
        Ok(slot)
    }

    fn from_ast(ast: Self::LatentInput, context: Self::Context) -> Result<Self> {
        let data = match ast.data {
            Some(ref data) => unpack_type_data(data, ast.name, context)?,
            None => TypeData::Incomplete,
        };
        Ok(TypeDecl2 {
            span: ast.span,
            name: ast.name,
            data: data,
        })
    }
}

impl<'t> Node<'t> for TypeDecl2 {
    fn span(&self) -> Span {
        self.span
    }

    fn desc_kind(&self) -> String {
        "type declaration".into()
    }

    fn desc_name(&self) -> String {
        format!("type declaration `{}`", self.name.value)
    }

    fn accept(&'t self, visitor: &mut Visitor<'t>) {
        visitor.visit_type_decl(self);
    }

    fn walk(&'t self, visitor: &mut Visitor<'t>) {
        visitor.visit_name(self.name);
    }
}

impl<'t> Decl2<'t> for TypeDecl2 {
    fn name(&self) -> Spanned<ResolvableName> {
        self.name.map_into()
    }
}

fn unpack_type_data(data: &Spanned<ast::TypeData>, type_name: Spanned<Name>, context: AllocContext) -> Result<TypeData> {
    match data.value {
        ast::RangeType(ref range_expr, ref units) => {
            let termctx = TermContext::new2(context);
            let range_expr = termctx.termify_expr(range_expr)?;
            debugln!("termified range expr to {:#?}", range_expr);
            // TODO:
            // - termify range expr
            // - map to range
            // - handle units
            panic!("range type not fully implemented");
        }
        _ => unimplemented!(
            "type `{}` unsupported type data {:#?}",
            type_name.value,
            data.value
        ),
    }
}