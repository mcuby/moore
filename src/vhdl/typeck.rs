// Copyright (c) 2017 Fabian Schuiki

//! This module implements the type calculation of the scoreboard.

use moore_common::errors::*;
use moore_common::score::{NodeMaker, Result};
use score::*;
use ty::*;
use konst::*;
use hir;

/// Performs a type check.
pub trait Typeck<I> {
	fn typeck(&self, id: I) -> Result<()>;
}

/// A macro to implement the `Typeck` trait.
macro_rules! impl_typeck {
	($slf:tt, $id:ident: $id_ty:ty => $blk:block) => {
		impl<'sb, 'ast, 'ctx> Typeck<$id_ty> for ScoreContext<'sb, 'ast, 'ctx> {
			fn typeck(&$slf, $id: $id_ty) -> Result<()> $blk
		}
	}
}

// Implement the `Typeck` trait for everything that supports type calculation.
impl<'ctx, I, T> Typeck<I> for T where T: NodeMaker<I, &'ctx Ty> {
	fn typeck(&self, id: I) -> Result<()> {
		T::make(self, id).map(|_| ())
	}
}

macro_rules! unimp {
	($slf:tt, $id:expr) => {{
		$slf.sess.emit(DiagBuilder2::bug(format!("typeck of {:?} not implemented", $id)));
		return Err(());
	}}
}

macro_rules! unimpmsg {
	($slf:tt, $span:expr, $msg:expr) => {{
		$slf.sess.emit(DiagBuilder2::bug(format!("{} not implemented", $msg)).span($span));
		return Err(());
	}}
}

impl_typeck!(self, id: EntityRef => {
	let hir = self.hir(id)?;
	for &generic in &hir.generics {
		self.typeck(generic)?;
	}
	for &port in &hir.ports {
		self.typeck(port)?;
	}
	Ok(())
});

impl_typeck!(self, id: ArchRef => {
	let hir = self.hir(id)?;
	self.typeck(hir.entity)?;
	for &decl in &hir.decls {
		self.typeck(decl)?;
	}
	for &stmt in &hir.stmts {
		self.typeck(stmt)?;
	}
	Ok(())
});

impl_typeck!(self, id: GenericRef => {
	match id {
		GenericRef::Type(id)    => self.typeck(id),
		GenericRef::Subprog(id) => self.typeck(id),
		GenericRef::Pkg(id)     => self.typeck(id),
		GenericRef::Const(id)   => self.typeck(id),
	}
});

// impl_typeck!(self, id: IntfSignalRef => {
// 	self.typeck(self.hir(id)?.ty)
// });

impl_typeck!(self, id: IntfTypeRef => {
	unimp!(self, id)
	// self.typeck(self.hir(id)?.ty)
});

impl_typeck!(self, id: IntfSubprogRef => {
	unimp!(self, id)
	// self.typeck(self.hir(id)?.ty)
});

impl_typeck!(self, id: IntfPkgRef => {
	unimp!(self, id)
	// self.typeck(self.hir(id)?.ty)
});

impl_typeck!(self, id: IntfConstRef => {
	unimp!(self, id)
	// self.typeck(self.hir(id)?.ty)
});

impl_typeck!(self, id: DeclInPkgRef => {
	match id {
		DeclInPkgRef::Pkg(id)     => self.typeck(id),
		DeclInPkgRef::PkgInst(id) => self.typeck(id),
		DeclInPkgRef::Type(id)    => self.typeck(id),
		DeclInPkgRef::Subtype(id) => self.typeck(id),
	}
});

impl_typeck!(self, id: DeclInBlockRef => {
	match id {
		DeclInBlockRef::Pkg(id)       => self.typeck(id),
		DeclInBlockRef::PkgInst(id)   => self.typeck(id),
		DeclInBlockRef::Type(id)      => self.typeck(id),
		DeclInBlockRef::Subtype(id)   => self.typeck(id),
		DeclInBlockRef::Const(id)     => self.typeck(id),
		DeclInBlockRef::Signal(id)    => self.typeck(id),
		DeclInBlockRef::SharedVar(id) => self.typeck(id),
		DeclInBlockRef::File(id)      => self.typeck(id),
	}
});

impl_typeck!(self, id: DeclInProcRef => {
	match id {
		DeclInProcRef::Pkg(id)     => self.typeck(id),
		DeclInProcRef::PkgBody(id) => self.typeck(id),
		DeclInProcRef::PkgInst(id) => self.typeck(id),
		DeclInProcRef::Type(id)    => self.typeck(id),
		DeclInProcRef::Subtype(id) => self.typeck(id),
		DeclInProcRef::Const(id)   => self.typeck(id),
		DeclInProcRef::Var(id)     => self.typeck(id),
		DeclInProcRef::File(id)    => self.typeck(id),
	}
});

impl_typeck!(self, id: ConcStmtRef => {
	match id {
		ConcStmtRef::Block(id)         => self.typeck(id),
		ConcStmtRef::Process(id)       => self.typeck(id),
		ConcStmtRef::ConcProcCall(id)  => self.typeck(id),
		ConcStmtRef::ConcAssert(id)    => self.typeck(id),
		ConcStmtRef::ConcSigAssign(id) => self.typeck(id),
		ConcStmtRef::CompInst(id)      => self.typeck(id),
		ConcStmtRef::ForGen(id)        => self.typeck(id),
		ConcStmtRef::IfGen(id)         => self.typeck(id),
		ConcStmtRef::CaseGen(id)       => self.typeck(id),
	}
});

impl_typeck!(self, id: SeqStmtRef => {
	match id {
		SeqStmtRef::Wait(id)      => self.typeck(id),
		SeqStmtRef::Assert(id)    => self.typeck(id),
		SeqStmtRef::Report(id)    => self.typeck(id),
		SeqStmtRef::SigAssign(id) => self.typeck(id),
		SeqStmtRef::VarAssign(id) => self.typeck(id),
		SeqStmtRef::ProcCall(id)  => self.typeck(id),
		SeqStmtRef::If(id)        => self.typeck(id),
		SeqStmtRef::Case(id)      => self.typeck(id),
		SeqStmtRef::Loop(id)      => self.typeck(id),
		SeqStmtRef::Next(id)      => self.typeck(id),
		SeqStmtRef::Exit(id)      => self.typeck(id),
		SeqStmtRef::Return(id)    => self.typeck(id),
		SeqStmtRef::Null(id)      => self.typeck(id),
	}
});

impl_typeck!(self, id: PkgDeclRef => {
	let hir = self.hir(id)?;
	for &generic in &hir.generics {
		self.typeck(generic)?;
	}
	for &decl in &hir.decls {
		self.typeck(decl)?;
	}
	Ok(())
});

impl_typeck!(self, id: PkgBodyRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: PkgInstRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: ConstDeclRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: SharedVarDeclRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: VarDeclRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: FileDeclRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: BlockStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: ProcessStmtRef => {
	let hir = self.hir(id)?;
	for &decl in &hir.decls {
		self.typeck(decl)?;
	}
	for &stmt in &hir.stmts {
		self.typeck(stmt)?;
	}
	Ok(())
});

impl_typeck!(self, id: ConcProcCallStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: ConcAssertStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: ConcSigAssignStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: CompInstStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: ForGenStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: IfGenStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: CaseGenStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: WaitStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: AssertStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: ReportStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: SigAssignStmtRef => {
	let hir = self.hir(id)?;
	let lhs_ty = match hir.target {
		hir::SigAssignTarget::Name(sig) => self.ty(sig)?,
		hir::SigAssignTarget::Aggregate => unimpmsg!(self, hir.target_span, "signal assignment to aggregate"),
	};
	self.sess.emit(
		DiagBuilder2::warning("type of right-hand side not checked")
		.span(hir.kind_span)
	);
	// TODO: Check right hand side.
	// let rhs_ty = match hir.kind {

	// };
	Ok(())
});

impl_typeck!(self, id: VarAssignStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: ProcCallStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: IfStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: CaseStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: LoopStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: NextStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: ExitStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, id: ReturnStmtRef => {
	unimp!(self, id)
});

impl_typeck!(self, _id: NullStmtRef => {
	// The null statement always typechecks.
	Ok(())
});

impl<'sb, 'ast, 'ctx> ScoreContext<'sb, 'ast, 'ctx> {
	/// Replace `Ty::Named` by the actual type definition recursively.
	pub fn deref_named_type<'a>(&self, ty: &'a Ty) -> Result<&'a Ty> where 'ctx: 'a {
		match ty {
			&Ty::Named(_, tmr) => {
				let inner = self.ty(tmr)?;
				self.deref_named_type(inner)
			}
			other => Ok(other)
		}
	}
}


/// Determine the type of a type mark.
impl_make!(self, id: TypeMarkRef => &Ty {
	match id {
		TypeMarkRef::Type(id) => self.make(id),
		TypeMarkRef::Subtype(id) => self.make(id),
	}
});


/// Determine the type of a subtype indication.
impl_make!(self, id: SubtypeIndRef => &Ty {
	let hir = self.hir(id)?;
	match hir.constraint {
		hir::Constraint::None => Ok(self.intern_ty(Ty::Named(hir.type_mark.span, hir.type_mark.value))),

		// For range constraints, we first have to check if the constraint is
		// applicable given the type mark. If it is, check if the provided
		// range actually is a proper subtype, and then apply the constraint.
		hir::Constraint::Range(span, expr_id) => {
			let inner = self.deref_named_type(self.ty(hir.type_mark.value)?)?;
			match *inner {
				Ty::Int(ref inner) => {
					// Evaluate the expression to a constant range.
					let range = match *self.const_value(expr_id)? {
						Const::IntRange(ref r) => r,
						ref wrong => {
							self.sess.emit(
								DiagBuilder2::error(format!("{} used to constrain integer type", wrong.kind_desc()))
								.span(span)
							);
							return Err(());
						}
					};

					// Make sure that this is actually a subtype.
					if inner.dir != range.dir || inner.left_bound > range.left_bound.value || inner.right_bound < range.right_bound.value {
						self.sess.emit(
							DiagBuilder2::error(format!("`{}` is not a subrange of `{}`", range, inner))
							.span(span)
						);
						return Err(());
					}

					// Create the new type.
					Ok(self.intern_ty(IntTy::new(inner.dir, range.left_bound.value.clone(), range.right_bound.value.clone()).maybe_null()))
				}

				// All other types we simply cannot constrain by range.
				_ => {
					self.sess.emit(
						DiagBuilder2::error(format!("{} cannot be constrained by range", inner.kind_desc()))
						.span(span)
					);
					return Err(());
				}
			}
		}

		hir::Constraint::Array(ref ac) => {
			self.sess.emit(
				DiagBuilder2::error("Array constraints on subtypes not yet supported")
				.span(ac.span)
			);
			Err(())
		}

		hir::Constraint::Record(ref rc) => {
			self.sess.emit(
				DiagBuilder2::error("Record constraints on subtypes not yet supported")
				.span(rc.span)
			);
			Err(())
		}
	}
});


/// Determine the type of a type declaration.
impl_make!(self, id: TypeDeclRef => &Ty {
	let hir = self.hir(id)?;
	let data = match hir.data {
		Some(ref d) => d,
		None => {
			self.sess.emit(
				DiagBuilder2::error(format!("Declaration of type `{}` is incomplete", hir.name.value))
				.span(hir.name.span)
			);
			return Err(());
		}
	};
	match *data {
		hir::TypeData::Range(span, dir, lb_id, rb_id) => {
			let lb = self.const_value(lb_id)?;
			let rb = self.const_value(rb_id)?;
			Ok(match (lb, rb) {
				(&Const::Int(ref lb), &Const::Int(ref rb)) => {
					self.intern_ty(IntTy::new(dir, lb.value.clone(), rb.value.clone()).maybe_null())
				}

				(&Const::Float(ref _lb), &Const::Float(ref _rb)) => {
					self.sess.emit(
						DiagBuilder2::error("Float range bounds not yet supported")
						.span(span)
					);
					return Err(());
				}

				_ => {
					self.sess.emit(
						DiagBuilder2::error("Bounds of range are not of the same type")
						.span(span)
					);
					return Err(());
				}
			})
		}

		hir::TypeData::Enum(..) => {
			Ok(self.intern_ty(EnumTy::new(id)))
		}
	}
});


/// Determine the type of a subtype declaration.
impl_make!(self, id: SubtypeDeclRef => &Ty {
	let hir = self.hir(id)?;
	self.ty(hir.subty)
});


/// Determine the type of a signal declaration.
impl_make!(self, id: SignalDeclRef => &Ty {
	let hir = self.existing_hir(id)?;
	self.ty(hir.subty)
});


/// Determine the type of an expression.
impl_make!(self, id: ExprRef => &Ty {
	let hir = self.hir(id)?;
	match hir.data {
		_ => panic!("typeck not impl for expr {:?}", hir.data)
	}
});


/// Determine the type of a typed node.
impl_make!(self, id: TypedNodeRef => &Ty {
	match id {
		TypedNodeRef::SubtypeInd(id) => self.make(id),
	}
});

impl_make!(self, id: SignalRef => &Ty {
	match id {
		SignalRef::Intf(id) => self.ty(id),
		SignalRef::Decl(id) => self.ty(id),
	}
});

impl_make!(self, id: IntfSignalRef => &Ty {
	let hir = self.hir(id)?;
	self.ty(hir.ty)
});
