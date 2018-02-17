// Copyright (c) 2018 Fabian Schuiki

//! An implementation of lazy compiler passes.

// #![deny(missing_docs)]

use std;
use std::fmt;
use std::cell::RefCell;
use std::collections::HashMap;

use moore_common::NodeId;
use moore_common::score::{NodeStorage, Result};
use score::{ScoreBoard, ScoreContext};
use hir;
use typeck::TypeckContext;
use score::WaitStmtRef;

/// A lazily evaluated node.
pub enum LazyNode<F> {
	/// Evaluation is currently running.
	Running,
	/// The callback which will provide the desired output.
	Pending(F),
}

impl<F> fmt::Debug for LazyNode<F> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			LazyNode::Running => write!(f, "running"),
			LazyNode::Pending(_) => write!(f, "pending"),
		}
	}
}

/// A table of lazy compiler phases.
pub struct LazyPhaseTable<'sb, 'ast: 'sb, 'ctx: 'sb> {
	/// The score board.
	pub sb: &'sb ScoreBoard<'ast, 'ctx>,
	/// The lazy HIR table.
	pub hir: LazyPhase<LazyHirTable<'sb, 'ast, 'ctx>>,
	/// The lazy typeck table.
	pub typeck: RefCell<LazyTypeckTable<'sb, 'ast, 'ctx>>,
}

impl <'sb, 'ast, 'ctx> LazyPhaseTable<'sb, 'ast, 'ctx> {
    /// Create a new phase table.
    pub fn new(sb: &'sb ScoreBoard<'ast, 'ctx>) -> LazyPhaseTable<'sb, 'ast, 'ctx> {
        LazyPhaseTable {
            sb: sb,
            hir: LazyPhase::new(),
            typeck: RefCell::new(HashMap::new()),
        }
    }
}

/// A table of tasks needed to perform a compiler phase.
pub struct LazyPhase<T> {
	pub table: RefCell<T>,
}

impl<T: Default> LazyPhase<T> where T: Default {
	/// Create a new lazy phase.
	pub fn new() -> LazyPhase<T> {
		LazyPhase {
			table: RefCell::new(Default::default()),
		}
	}

	/// Schedule a task to be lazily executed.
	pub fn schedule<I,F>(&self, id: I, f: F)
		where T: NodeStorage<I, Node=LazyNode<F>>
	{
		self.table.borrow_mut().set(id, LazyNode::Pending(f));
	}

	/// Run a task.
	pub fn run<'lazy, 'sb, 'ast, 'ctx, I, R>(&self, id: I, ctx: &ScoreContext<'lazy, 'sb, 'ast, 'ctx>) -> Result<R>
	where
		I: Copy + fmt::Debug,
		T: NodeStorage<I, Node=LazyNode<Box<for<'a,'b> Fn(&'a ScoreContext<'b, 'sb, 'ast, 'ctx>) -> Result<R> + 'sb>>>
	{
		let task = self.table.borrow_mut().set(id, LazyNode::Running);
		match task {
			Some(LazyNode::Pending(f)) => f(ctx),
			Some(LazyNode::Running) => panic!("recursion when running task for {:?}", id),
			None => panic!("no task scheduled for {:?}", id),
		}
	}
}

pub type LazyHir<'sb, 'ast, 'ctx, R> = Box<for<'a,'b> Fn(&'a ScoreContext<'b, 'sb, 'ast, 'ctx>) -> Result<R> + 'sb>;

node_storage!(LazyHirTable<'sb, 'ast, 'ctx> where ('ast: 'sb, 'ctx: 'sb):
	wait_stmts: WaitStmtRef => LazyNode<LazyHir<'sb, 'ast, 'ctx, hir::Stmt<hir::WaitStmt>>>,
);

impl<'sb, 'ast, 'ctx> Default for LazyHirTable<'sb, 'ast, 'ctx> {
	fn default() -> LazyHirTable<'sb, 'ast, 'ctx> {
		LazyHirTable::new()
	}
}

pub type LazyTypeck<'sb, 'ast, 'ctx> = Box<for<'a,'b,'c> Fn(&'a TypeckContext<'b, 'c, 'sb, 'ast, 'ctx>) -> Result<()> + 'sb>;
pub type LazyTypeckTable<'sb, 'ast, 'ctx> = HashMap<NodeId, LazyNode<LazyTypeck<'sb, 'ast ,'ctx>>>;