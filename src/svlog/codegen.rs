// Copyright (c) 2016-2019 Fabian Schuiki

//! This module implements LLHD code generation.

use crate::{
    crate_prelude::*,
    hir::HirNode,
    ty::{Type, TypeKind},
    value::{Value, ValueKind},
    ParamEnv, ParamEnvSource, PortMappingSource,
};
use llhd::ir::{Unit, UnitBuilder};
use num::{BigInt, One, Zero};
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    ops::DerefMut,
};

/// A code generator.
///
/// Use this struct to emit LLHD code for nodes in a [`Context`].
pub struct CodeGenerator<'gcx, C> {
    /// The compilation context.
    cx: C,
    /// The LLHD module to be populated.
    into: llhd::ir::Module,
    /// Tables holding mappings and interned values.
    tables: Tables<'gcx>,
}

impl<'gcx, C> CodeGenerator<'gcx, C> {
    /// Create a new code generator.
    pub fn new(cx: C) -> Self {
        CodeGenerator {
            cx,
            into: llhd::ir::Module::new(),
            tables: Default::default(),
        }
    }

    /// Finalize code generation and return the generated LLHD module.
    pub fn finalize(self) -> llhd::ir::Module {
        self.into
    }
}

#[derive(Default)]
struct Tables<'gcx> {
    module_defs: HashMap<NodeEnvId, Result<llhd::ir::ModUnit>>,
    module_signatures: HashMap<NodeEnvId, (llhd::ir::UnitName, llhd::ir::Signature)>,
    interned_types: HashMap<(Type<'gcx>, ParamEnv), Result<llhd::Type>>,
}

impl<'gcx, C> Deref for CodeGenerator<'gcx, C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.cx
    }
}

impl<'a, 'gcx, C: Context<'gcx>> CodeGenerator<'gcx, &'a C> {
    /// Emit the code for a module and all its dependent modules.
    pub fn emit_module(&mut self, id: NodeId) -> Result<llhd::ir::ModUnit> {
        self.emit_module_with_env(id, self.default_param_env())
    }

    /// Emit the code for a module and all its dependent modules.
    pub fn emit_module_with_env(&mut self, id: NodeId, env: ParamEnv) -> Result<llhd::ir::ModUnit> {
        if let Some(x) = self.tables.module_defs.get(&(id, env)) {
            return x.clone();
        }
        let hir = match self.hir_of(id)? {
            HirNode::Module(m) => m,
            _ => panic!("expected {:?} to be a module", id),
        };
        debug!("emit module `{}` with {:?}", hir.name, env);

        // Determine entity type and port names.
        let mut sig = llhd::ir::Signature::new();
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        let mut port_id_to_name = HashMap::new();
        for &port_id in hir.ports {
            let port = match self.hir_of(port_id)? {
                HirNode::Port(p) => p,
                _ => unreachable!(),
            };
            let ty = self.type_of(port_id, env)?;
            debug!(
                "port {}.{} has type {:?}",
                hir.name.value, port.name.value, ty
            );
            let ty = self.emit_type(ty, env)?;
            let ty = match port.dir {
                ast::PortDir::Ref => llhd::pointer_ty(ty),
                _ => llhd::signal_ty(ty),
            };
            match port.dir {
                ast::PortDir::Input | ast::PortDir::Ref => {
                    sig.add_input(ty);
                    inputs.push(port_id);
                }
                ast::PortDir::Output => {
                    sig.add_output(ty);
                    outputs.push(port_id);
                }
                ast::PortDir::Inout => {
                    sig.add_input(ty.clone());
                    sig.add_output(ty);
                    inputs.push(port_id);
                    outputs.push(port_id);
                }
            }
            port_id_to_name.insert(port_id, port.name);
        }

        // Pick an entity name.
        let mut entity_name: String = hir.name.value.into();
        if env != self.default_param_env() {
            entity_name.push_str(&format!(".param{}", env.0));
        }
        let name = llhd::ir::UnitName::Global(entity_name.clone());

        // Create entity.
        let mut ent = llhd::ir::Entity::new(name.clone(), sig.clone());
        let mut builder = llhd::ir::EntityBuilder::new(&mut ent);
        self.tables.module_signatures.insert((id, env), (name, sig));
        let mut values = HashMap::<NodeId, llhd::ir::Value>::new();
        let mut gen = UnitGenerator {
            gen: self,
            builder: &mut builder,
            values: &mut values,
            interned_consts: Default::default(),
            interned_lvalues: Default::default(),
            interned_rvalues: Default::default(),
            shadows: Default::default(),
        };

        // Assign proper port names and collect ports into a lookup table.
        for (index, port_id) in inputs.into_iter().enumerate() {
            let arg = gen.builder.entity.input_arg(index);
            gen.builder
                .dfg_mut()
                .set_name(arg, port_id_to_name[&port_id].value.to_string());
            gen.values.insert(port_id, arg);
        }
        for (index, &port_id) in outputs.iter().enumerate() {
            let arg = gen.builder.entity.output_arg(index);
            gen.builder
                .dfg_mut()
                .set_name(arg, port_id_to_name[&port_id].value.to_string());
            gen.values.insert(port_id, arg);
        }

        // Emit the actual contents of the entity.
        gen.emit_module_block(id, env, &hir.block, &entity_name)?;

        // Assign default values to undriven output ports.
        for port_id in outputs {
            let driven = {
                let value = gen.values[&port_id];
                gen.builder.inst_layout().insts().any(|inst| {
                    match gen.builder.dfg()[inst].opcode() {
                        llhd::ir::Opcode::Drv => gen.builder.dfg()[inst].args()[0] == value,
                        llhd::ir::Opcode::Inst => {
                            gen.builder.dfg()[inst].output_args().contains(&value)
                        }
                        _ => false,
                    }
                })
            };
            if driven {
                continue;
            }
            let hir = match gen.hir_of(port_id)? {
                HirNode::Port(p) => p,
                _ => unreachable!(),
            };
            let default_value = gen.emit_const(
                if let Some(default) = hir.default {
                    gen.constant_value_of(default, env)?
                } else {
                    gen.type_default_value(gen.type_of(port_id, env)?)
                },
                env,
            )?;
            let zero_time = llhd::ConstTime::new(num::zero(), 0, 0);
            let zero_time = gen.builder.ins().const_time(zero_time);
            gen.builder
                .ins()
                .drv(gen.values[&port_id], default_value, zero_time);
        }

        trace!("{}", ent.dump());
        let result = Ok(self.into.add_entity(ent));
        self.tables.module_defs.insert((id, env), result.clone());
        result
    }

    /// Emit the code for a procedure.
    fn emit_procedure(
        &mut self,
        id: NodeId,
        env: ParamEnv,
        name_prefix: &str,
    ) -> Result<llhd::ir::ModUnit> {
        let hir = match self.hir_of(id)? {
            HirNode::Proc(x) => x,
            _ => unreachable!(),
        };

        // Find the accessed nodes.
        let acc = self.accessed_nodes(hir.stmt)?;
        trace!("process accesses {:#?}", acc);
        let mut sig = llhd::ir::Signature::new();
        let mut inputs = vec![];
        let mut outputs = vec![];
        for &id in &acc.read {
            sig.add_input(llhd::signal_ty(
                self.emit_type(self.type_of(id, env)?, env)?,
            ));
            inputs.push(id);
        }
        for &id in &acc.written {
            sig.add_output(llhd::signal_ty(
                self.emit_type(self.type_of(id, env)?, env)?,
            ));
            outputs.push(id);
        }
        trace!("process inputs = {:#?}", inputs);
        trace!("process outputs = {:#?}", outputs);

        // Create process and entry block.
        let proc_name = format!(
            "{}.{}.{}.{}",
            name_prefix,
            match hir.kind {
                ast::ProcedureKind::Initial => "initial",
                ast::ProcedureKind::Always => "always",
                ast::ProcedureKind::AlwaysComb => "always_comb",
                ast::ProcedureKind::AlwaysLatch => "always_latch",
                ast::ProcedureKind::AlwaysFf => "always_ff",
                ast::ProcedureKind::Final => "final",
            },
            id.as_usize(),
            env.0,
        );
        let mut prok = llhd::ir::Process::new(llhd::ir::UnitName::Local(proc_name), sig);
        let mut builder = llhd::ir::ProcessBuilder::new(&mut prok);

        // Assign names to inputs and outputs.
        let guess_name = |id| match self.hir_of(id).ok()? {
            hir::HirNode::VarDecl(x) => Some(x.name),
            hir::HirNode::Port(x) => Some(x.name),
            _ => None,
        };
        for (i, &id) in acc.read.iter().enumerate() {
            if let Some(name) = guess_name(id) {
                let value = builder.prok.input_arg(i);
                builder.dfg_mut().set_name(value, format!("{}", name));
            }
        }
        for (i, &id) in acc.written.iter().enumerate() {
            if let Some(name) = guess_name(id) {
                let value = builder.prok.output_arg(i);
                builder.dfg_mut().set_name(value, format!("{}", name));
            }
        }

        // Create a mapping from read/written nodes to process parameters.
        let mut values = HashMap::new();
        for (&id, arg) in inputs
            .iter()
            .zip(builder.prok.input_args())
            .chain(outputs.iter().zip(builder.prok.output_args()))
        {
            values.insert(id, arg);
        }
        let mut pg = UnitGenerator {
            gen: self,
            builder: &mut builder,
            values: &mut values,
            interned_consts: Default::default(),
            interned_lvalues: Default::default(),
            interned_rvalues: Default::default(),
            shadows: Default::default(),
        };
        let entry_blk = pg.add_nameless_block();
        pg.builder.append_to(entry_blk);

        // Determine which values are both read and written. These require
        // shadow variables to emulate the expected behaviour under blocking
        // assignments.
        let input_set: HashSet<_> = inputs.iter().cloned().collect();
        let output_set: HashSet<_> = outputs.iter().cloned().collect();
        for &id in input_set.intersection(&output_set) {
            let init = pg.builder.ins().prb(pg.values[&id]);
            let shadow = pg.builder.ins().var(init);
            if let Some(name) = pg
                .builder
                .dfg()
                .get_name(pg.values[&id])
                .map(|name| format!("{}.shadow", name))
            {
                pg.builder.dfg_mut().set_name(shadow, name);
            }
            pg.shadows.insert(id, shadow);
        }

        // Emit prologue and determine which basic block to jump back to.
        let head_blk = match hir.kind {
            ast::ProcedureKind::AlwaysComb | ast::ProcedureKind::AlwaysLatch => {
                let body_blk = pg.add_named_block("body");
                let check_blk = pg.add_named_block("check");
                pg.builder.ins().br(body_blk);
                pg.builder.append_to(check_blk);
                let trigger_on = inputs
                    .iter()
                    .map(|id| pg.emitted_value(*id).clone())
                    .collect();
                pg.builder.ins().wait(body_blk, trigger_on);
                pg.builder.append_to(body_blk);
                pg.emit_shadow_update();
                check_blk
            }
            ast::ProcedureKind::Final => {
                // TODO(fschuiki): Replace this with a cleverer way to implement a trigger-on-end.
                let body_blk = pg.add_named_block("body");
                let endtimes = pg.builder.ins().const_time(llhd::ConstTime::new(
                    "9001".parse().unwrap(),
                    0,
                    0,
                ));
                pg.builder
                    .dfg_mut()
                    .set_name(endtimes, "endtimes".to_string());
                pg.builder.ins().wait_time(body_blk, endtimes, vec![]);
                pg.builder.append_to(body_blk);
                pg.emit_shadow_update();
                entry_blk // This block is ignored for final blocks
            }
            _ => entry_blk,
        };

        // Emit the main statement.
        pg.emit_stmt(hir.stmt, env)?;

        // Emit epilogue.
        match hir.kind {
            ast::ProcedureKind::Initial => {
                pg.builder.ins().halt();
            }
            ast::ProcedureKind::Always
            | ast::ProcedureKind::AlwaysComb
            | ast::ProcedureKind::AlwaysLatch
            | ast::ProcedureKind::AlwaysFf => {
                pg.builder.ins().br(head_blk);
            }
            ast::ProcedureKind::Final => {
                pg.builder.ins().halt();
            }
        }
        Ok(self.into.add_process(prok))
    }

    /// Map a type to an LLHD type (interned).
    fn emit_type(&mut self, ty: Type<'gcx>, env: ParamEnv) -> Result<llhd::Type> {
        if let Some(x) = self.tables.interned_types.get(&(ty, env)) {
            x.clone()
        } else {
            let x = self.emit_type_uninterned(ty, env);
            self.tables.interned_types.insert((ty, env), x.clone());
            x
        }
    }

    /// Map a type to an LLHD type.
    fn emit_type_uninterned(&mut self, ty: Type<'gcx>, env: ParamEnv) -> Result<llhd::Type> {
        #[allow(unreachable_patterns)]
        Ok(match *ty {
            TypeKind::Void => llhd::void_ty(),
            TypeKind::Bit(_) => llhd::int_ty(1),
            TypeKind::Int(width, _) => llhd::int_ty(width),
            TypeKind::Named(_, _, ty) => self.emit_type(ty, env)?,
            TypeKind::Struct(id) => {
                let fields = match self.hir_of(id)? {
                    HirNode::Type(hir::Type {
                        kind: hir::TypeKind::Struct(ref fields),
                        ..
                    }) => fields,
                    _ => unreachable!(),
                };
                let mut types = vec![];
                for &field_id in fields {
                    types.push(self.emit_type(self.type_of(field_id, env)?, env)?);
                }
                llhd::struct_ty(types)
            }
            TypeKind::PackedArray(size, ty) => llhd::array_ty(size, self.emit_type(ty, env)?),
            // TODO(fschuiki): emit logic type depending on value domain
            TypeKind::BitScalar { .. } => llhd::int_ty(1),
            // TODO(fschuiki): emit logic type depending on value domain
            TypeKind::BitVector { range, .. } => llhd::int_ty(range.size),
            _ => unimplemented!("emit type {:?}", ty),
        })
    }

    /// Execute the initialization step of a generate loop.
    fn execute_genvar_init(&mut self, id: NodeId, env: ParamEnv) -> Result<ParamEnv> {
        let hir = self.hir_of(id)?;
        match hir {
            HirNode::GenvarDecl(_) => (),
            _ => unreachable!(),
        }
        Ok(env)
    }

    /// Execute the iteration step of a generate loop.
    fn execute_genvar_step(&mut self, id: NodeId, env: ParamEnv) -> Result<ParamEnv> {
        let hir = self.hir_of(id)?;
        let mut env_data = self.param_env_data(env).clone();
        let next = match hir {
            HirNode::Expr(expr) => match expr.kind {
                hir::ExprKind::Unary(op, target_id) => {
                    let target_id = self.resolve_node(target_id, env)?;
                    let current_value = self.constant_value_of(target_id, env)?;
                    let next_value = match current_value.kind {
                        ValueKind::Int(ref v, ..) => match op {
                            hir::UnaryOp::PostInc | hir::UnaryOp::PreInc => Some(v + 1),
                            hir::UnaryOp::PostDec | hir::UnaryOp::PreDec => Some(v - 1),
                            _ => None,
                        }
                        .map(|v| value::make_int(current_value.ty, v)),
                        _ => unreachable!(),
                    };
                    next_value.map(|v| (target_id, v))
                }
                _ => None,
            },
            _ => None,
        };
        match next {
            Some((target_id, next_value)) => {
                env_data.set_value(target_id, self.intern_value(next_value));
                return Ok(self.intern_param_env(env_data));
            }
            None => {
                self.emit(
                    DiagBuilder2::error(format!(
                        "{} is not a valid genvar iteration step",
                        hir.desc_full()
                    ))
                    .span(hir.human_span()),
                );
                Err(())
            }
        }
    }
}

/// A code generator for functions, processes, and entities.
struct UnitGenerator<'a, 'gcx, C, UB> {
    /// The global code generator.
    gen: &'a mut CodeGenerator<'gcx, C>,
    /// The builder into which instructions are emitted.
    builder: &'a mut UB,
    /// The emitted LLHD values for various nodes.
    values: &'a mut HashMap<NodeId, llhd::ir::Value>,
    /// The constant values emitted into the unit.
    interned_consts: HashMap<Value<'gcx>, Result<llhd::ir::Value>>,
    /// The MIR lvalues emitted into the unit.
    #[allow(dead_code)]
    interned_lvalues: HashMap<NodeId, Result<llhd::ir::Value>>,
    /// The MIR rvalues emitted into the unit.
    interned_rvalues: HashMap<NodeId, Result<llhd::ir::Value>>,
    /// The shadow variables introduced to handle signals which are both read
    /// and written in a process.
    shadows: HashMap<NodeId, llhd::ir::Value>,
}

impl<'a, 'gcx, C, UB> Deref for UnitGenerator<'a, 'gcx, C, UB> {
    type Target = CodeGenerator<'gcx, C>;

    fn deref(&self) -> &CodeGenerator<'gcx, C> {
        &self.gen
    }
}

impl<'a, 'gcx, C, UB> DerefMut for UnitGenerator<'a, 'gcx, C, UB> {
    fn deref_mut(&mut self) -> &mut CodeGenerator<'gcx, C> {
        &mut self.gen
    }
}

impl<'a, 'b, 'gcx, C, UB> UnitGenerator<'a, 'gcx, &'b C, UB>
where
    C: Context<'gcx> + 'b,
    UB: UnitBuilder,
{
    fn emitted_value(&self, node_id: NodeId) -> llhd::ir::Value {
        self.values[&node_id]
    }

    fn set_emitted_value(&mut self, node_id: NodeId, value: llhd::ir::Value) {
        self.values.insert(node_id, value);
    }

    /// Emit the code for the contents of a module.
    fn emit_module_block(
        &mut self,
        id: NodeId,
        env: ParamEnv,
        hir: &hir::ModuleBlock,
        name_prefix: &str,
    ) -> Result<()> {
        // Emit declarations.
        for &decl_id in &hir.decls {
            let hir = match self.hir_of(decl_id)? {
                HirNode::VarDecl(x) => x,
                _ => unreachable!(),
            };
            let ty = self.type_of(decl_id, env)?;
            let init = self.emit_const(
                match hir.init {
                    Some(expr) => self.constant_value_of(expr, env)?,
                    None => self.type_default_value(ty),
                },
                env,
            )?;
            let value = self.builder.ins().sig(init);
            self.builder
                .dfg_mut()
                .set_name(value, hir.name.value.into());
            self.values.insert(decl_id, value.into());
        }

        // Emit assignments.
        for &assign_id in &hir.assigns {
            let hir = match self.hir_of(assign_id)? {
                HirNode::Assign(x) => x,
                _ => unreachable!(),
            };
            let lhs = self.mir_lvalue(hir.lhs, env);
            let rhs = self.mir_rvalue(hir.rhs, env);
            // TODO(fschuiki): The following should happen in a lowering of the
            // assignment to its own MIR.
            let rhs = mir::lower::rvalue::cast_to_type(self.cx, rhs, env, lhs.ty);
            let lhs = self.emit_mir_lvalue(lhs)?.0;
            let rhs = self.emit_mir_rvalue(rhs)?;
            let one_epsilon = llhd::ConstTime::new(num::zero(), 0, 1);
            let one_epsilon = self.builder.ins().const_time(one_epsilon);
            self.builder.ins().drv(lhs, rhs, one_epsilon);
        }

        // Emit instantiations.
        for &inst_id in &hir.insts {
            let hir = match self.hir_of(inst_id)? {
                HirNode::Inst(x) => x,
                _ => unreachable!(),
            };
            let target_hir = match self.hir_of(hir.target)? {
                HirNode::InstTarget(x) => x,
                _ => unreachable!(),
            };
            let resolved = match self.gcx().find_module(target_hir.name.value) {
                Some(id) => id,
                None => {
                    self.emit(
                        DiagBuilder2::error(format!(
                            "unknown module or interface `{}`",
                            target_hir.name.value
                        ))
                        .span(target_hir.name.span),
                    );
                    return Err(());
                }
            };
            let inst_env = self.param_env(ParamEnvSource::ModuleInst {
                module: resolved,
                inst: inst_id,
                env,
                pos: &target_hir.pos_params,
                named: &target_hir.named_params,
            })?;
            trace!("{:?} = {:#?}", inst_env, self.param_env_data(inst_env));
            trace!("pos_ports = {:#?}", hir.pos_ports);
            trace!("named_ports = {:#?}", hir.named_ports);
            trace!("has_wildcard_port = {:#?}", hir.has_wildcard_port);
            let port_mapping = self.port_mapping(PortMappingSource::ModuleInst {
                module: resolved,
                inst: inst_id,
                env: inst_env,
                pos: &hir.pos_ports,
                named: &hir.named_ports,
            })?;
            trace!("port_mapping = {:#?}", port_mapping);
            let mut inputs = Vec::new();
            let mut outputs = Vec::new();
            let resolved_hir = match self.hir_of(resolved)? {
                HirNode::Module(x) => x,
                _ => unreachable!(),
            };
            for &port_id in resolved_hir.ports {
                let port = match self.hir_of(port_id)? {
                    HirNode::Port(p) => p,
                    _ => unreachable!(),
                };
                let mapping = port_mapping.find(port_id);
                let (is_input, is_output) = match port.dir {
                    ast::PortDir::Input | ast::PortDir::Ref => (true, false),
                    ast::PortDir::Output => (false, true),
                    ast::PortDir::Inout => (true, true),
                };
                if let Some(mapping) = mapping {
                    if is_input {
                        inputs.push(self.emit_rvalue_mode(mapping.0, mapping.1, Mode::Signal)?);
                    }
                    if is_output {
                        outputs.push(self.emit_lvalue(mapping.0, mapping.1)?);
                    }
                } else {
                    let ty = self.type_of(port_id, inst_env)?;
                    let value = match port.default {
                        Some(default) => self.emit_rvalue_mode(default, inst_env, Mode::Signal)?,
                        None => {
                            let v = self.type_default_value(ty.clone());
                            let v = self.emit_const(v, inst_env)?;
                            self.builder.ins().sig(v)
                        }
                    };
                    if is_input {
                        inputs.push(value);
                    }
                    if is_output {
                        outputs.push(value);
                    }
                }
            }
            trace!("inputs = {:#?}", inputs);
            trace!("outputs = {:#?}", outputs);
            let target = self.emit_module_with_env(resolved, inst_env)?;
            // let (name, sig) = self.tables.module_signatures[&(resolved, inst_env)].clone();
            let ext_unit = self.builder.add_extern(
                self.into[target].name().clone(),
                self.into[target].sig().clone(),
            );
            self.builder.ins().inst(ext_unit, inputs, outputs);
            // self.builder.dfg_mut().set_name(inst, hir.name.value.into());
        }

        // Emit generate blocks.
        for &gen_id in &hir.gens {
            let hir = match self.hir_of(gen_id)? {
                HirNode::Gen(x) => x,
                _ => unreachable!(),
            };
            #[allow(unreachable_patterns)]
            match hir.kind {
                hir::GenKind::If {
                    cond,
                    ref main_body,
                    ref else_body,
                } => {
                    let k = self.constant_value_of(cond, env)?;
                    if k.is_false() {
                        if let Some(else_body) = else_body {
                            self.emit_module_block(id, env, else_body, name_prefix)?;
                        }
                    } else {
                        self.emit_module_block(id, env, main_body, name_prefix)?;
                    }
                }
                hir::GenKind::For {
                    ref init,
                    cond,
                    step,
                    ref body,
                } => {
                    let mut local_env = env;
                    for &i in init {
                        local_env = self.execute_genvar_init(i, local_env)?;
                    }
                    while self.constant_value_of(cond, local_env)?.is_true() {
                        self.emit_module_block(id, local_env, body, name_prefix)?;
                        local_env = self.execute_genvar_step(step, local_env)?;
                    }
                }
                _ => return self.unimp_msg("code generation for", hir),
            }
        }

        // Emit and instantiate procedures.
        for &proc_id in &hir.procs {
            let prok = self.emit_procedure(proc_id, env, name_prefix)?;
            let acc = self.accessed_nodes(proc_id)?;
            let lookup_value = |&id| match self.values.get(&id) {
                Some(v) => v.clone(),
                None => {
                    self.emit(
                        DiagBuilder2::bug(format!(
                            "{} used as input/output of {}, but no value has been emitted",
                            self.hir_of(id).unwrap().desc_full(),
                            self.hir_of(proc_id).unwrap().desc_full(),
                        ))
                        .span(self.span(id)),
                    );
                    panic!("no value emitted for {:?}", id);
                }
            };
            let inputs = acc.read.iter().map(lookup_value).collect();
            let outputs = acc.written.iter().map(lookup_value).collect();
            let ext_unit = self.builder.add_extern(
                self.into[prok].name().clone(),
                self.into[prok].sig().clone(),
            );
            self.builder.ins().inst(ext_unit, inputs, outputs);
        }

        Ok(())
    }

    /// Map a value to an LLHD constant (interned).
    fn emit_const(&mut self, value: Value<'gcx>, env: ParamEnv) -> Result<llhd::ir::Value> {
        if let Some(x) = self.interned_consts.get(value) {
            x.clone()
        } else {
            let x = self.emit_const_uninterned(value, env);
            // self.interned_consts.insert(value, x.clone());
            x
        }
    }

    /// Map a value to an LLHD constant.
    fn emit_const_uninterned(
        &mut self,
        value: Value<'gcx>,
        env: ParamEnv,
    ) -> Result<llhd::ir::Value> {
        match (value.ty, &value.kind) {
            (&TypeKind::Int(width, _), &ValueKind::Int(ref k, ..))
            | (
                &TypeKind::BitVector {
                    range: ty::Range { size: width, .. },
                    ..
                },
                &ValueKind::Int(ref k, ..),
            ) => Ok(self.builder.ins().const_int(width, k.clone())),
            (&TypeKind::Time, &ValueKind::Time(ref k)) => Ok(self
                .builder
                .ins()
                .const_time(llhd::ConstTime::new(k.clone(), 0, 0))),
            (&TypeKind::Bit(_), &ValueKind::Int(ref k, ..))
            | (&TypeKind::BitScalar { .. }, &ValueKind::Int(ref k, ..)) => {
                Ok(self.builder.ins().const_int(1, k.clone()))
            }
            (&TypeKind::PackedArray(..), &ValueKind::StructOrArray(ref v)) => {
                let fields: Result<Vec<_>> = v
                    .iter()
                    .map(|v| self.emit_const(v, env).map(Into::into))
                    .collect();
                Ok(self.builder.ins().array(fields?))
            }
            (&TypeKind::Struct(..), &ValueKind::StructOrArray(ref v)) => {
                let fields: Result<Vec<_>> = v
                    .iter()
                    .map(|v| self.emit_const(v, env).map(Into::into))
                    .collect();
                Ok(self.builder.ins().strukt(fields?))
            }
            _ => panic!("invalid type/value combination {:#?}", value),
        }
    }

    /// Emit the value `1` for a type.
    fn const_one_for_type(&mut self, ty: Type<'gcx>, env: ParamEnv) -> Result<llhd::ir::Value> {
        use num::one;
        match *ty {
            TypeKind::Bit(..)
            | TypeKind::Int(..)
            | TypeKind::BitScalar { .. }
            | TypeKind::BitVector { .. } => {
                Ok(self.emit_const(self.intern_value(value::make_int(ty, one())), env)?)
            }
            TypeKind::Named(_, _, ty) => self.const_one_for_type(ty, env),
            _ => panic!("no unit-value for type {:?}", ty),
        }
    }

    /// Emit the zero value for an LLHD type.
    ///
    /// This function is ultimately expected to be moved into LLHD.
    fn emit_zero_for_type(&mut self, ty: &llhd::Type) -> llhd::ir::Value {
        match **ty {
            llhd::TimeType => {
                self.builder
                    .ins()
                    .const_time(llhd::ConstTime::new(num::zero(), 0, 0))
            }
            llhd::IntType(w) => self.builder.ins().const_int(w, 0),
            llhd::SignalType(ref ty) => {
                let inner = self.emit_zero_for_type(ty);
                self.builder.ins().sig(inner)
            }
            llhd::ArrayType(l, ref ty) => {
                let inner = self.emit_zero_for_type(ty);
                self.builder.ins().array_uniform(l, inner)
            }
            llhd::StructType(ref tys) => {
                let inner = tys.iter().map(|ty| self.emit_zero_for_type(ty)).collect();
                self.builder.ins().strukt(inner)
            }
            _ => panic!("no zero-value for type {}", ty),
        }
    }

    /// Get the type of an LLHD value.
    fn llhd_type(&self, value: llhd::ir::Value) -> llhd::Type {
        self.builder.dfg().value_type(value)
    }

    /// Emit the code for an rvalue.
    fn emit_rvalue(&mut self, expr_id: NodeId, env: ParamEnv) -> Result<llhd::ir::Value> {
        self.emit_rvalue_mode(expr_id, env, Mode::Value)
    }

    /// Emit the code for an rvalue.
    fn emit_rvalue_mode(
        &mut self,
        expr_id: NodeId,
        env: ParamEnv,
        mode: Mode,
    ) -> Result<llhd::ir::Value> {
        let mir = self.mir_rvalue(expr_id, env);
        let value = self.emit_mir_rvalue(mir)?;
        let actual_mode = Mode::Value;

        match (mode, actual_mode) {
            (Mode::Signal, Mode::Value) => {
                let ty = self.llhd_type(value);
                let init = self.emit_zero_for_type(&ty);
                let sig = self.builder.ins().sig(init);
                let delay = llhd::ConstTime::new(num::zero(), 0, 1);
                let delay_const = self.builder.ins().const_time(delay);
                self.builder.ins().drv(sig, value, delay_const);
                Ok(sig)
            }
            (Mode::Value, Mode::Signal) => unreachable!(),
            _ => Ok(value),
        }
    }

    /// Emit the code for an MIR rvalue.
    fn emit_mir_rvalue(&mut self, mir: &mir::Rvalue<'gcx>) -> Result<llhd::ir::Value> {
        if let Some(x) = self.interned_rvalues.get(&mir.id) {
            x.clone()
        } else {
            let x = self.emit_mir_rvalue_uninterned(mir);
            // self.interned_rvalues.insert(mir.id, x.clone());
            x
        }
    }

    /// Emit the code for an MIR rvalue.
    fn emit_mir_rvalue_uninterned(&mut self, mir: &mir::Rvalue<'gcx>) -> Result<llhd::ir::Value> {
        match mir.kind {
            mir::RvalueKind::Var(id) => {
                let value = self
                    .shadows
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| self.emitted_value(id));
                Ok(match *self.llhd_type(value) {
                    llhd::SignalType(_) => {
                        let value = self.builder.ins().prb(value);
                        self.builder
                            .dfg_mut()
                            .set_name(value, format!("{}", mir.span.extract()));
                        value
                    }
                    llhd::PointerType(_) => {
                        let value = self.builder.ins().ld(value);
                        self.builder
                            .dfg_mut()
                            .set_name(value, format!("{}", mir.span.extract()));
                        value
                    }
                    _ => value,
                })
            }

            mir::RvalueKind::Port(id) => {
                let value = self
                    .shadows
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| self.emitted_value(id));
                let value = self.builder.ins().prb(value);
                self.builder
                    .dfg_mut()
                    .set_name(value, format!("{}", mir.span.extract()));
                Ok(value)
            }

            mir::RvalueKind::CastValueDomain { value, .. } => {
                // TODO(fschuiki): Turn this into an actual `iN` to `lN` cast.
                self.emit_mir_rvalue(value)
            }

            mir::RvalueKind::CastVectorToAtom { value, .. }
            | mir::RvalueKind::CastAtomToVector { value, .. } => {
                // Vector to atom conversions are no-ops since we represent the
                // atom type as a single-element vector anyway.
                self.emit_mir_rvalue(value)
            }

            mir::RvalueKind::CastSign(_, value) => {
                // Sign conversions are no-ops in LLHD since they merely
                // influence the type system.
                self.emit_mir_rvalue(value)
            }

            mir::RvalueKind::CastToBool(value) => {
                let value = self.emit_mir_rvalue(value)?;
                let ty = self.llhd_type(value);
                let zero = self.emit_zero_for_type(&ty);
                Ok(self.builder.ins().neq(value, zero))
            }

            mir::RvalueKind::Truncate(target_width, value) => {
                let llvalue = self.emit_mir_rvalue(value)?;
                Ok(self.builder.ins().ext_slice(llvalue, 0, target_width))
            }

            mir::RvalueKind::ZeroExtend(_, value) => {
                let width = value.ty.width();
                let llty = self.emit_type(mir.ty, mir.env)?;
                let result = self.emit_zero_for_type(&llty);
                let value = self.emit_mir_rvalue(value)?;
                Ok(self.builder.ins().ins_slice(result, value, 0, width))
            }

            mir::RvalueKind::SignExtend(_, value) => {
                let width = value.ty.width();
                let llty = self.emit_type(mir.ty, mir.env)?;
                let value = self.emit_mir_rvalue(value)?;
                let sign = self.builder.ins().ext_slice(value, width - 1, 1);
                let zeros = self.emit_zero_for_type(&llty);
                let ones = self.builder.ins().not(zeros);
                let mux = self.builder.ins().array(vec![zeros, ones]);
                let mux = self.builder.ins().mux(mux, sign);
                Ok(self.builder.ins().ins_slice(mux, value, 0, width))
            }

            mir::RvalueKind::ConstructArray(ref indices) => {
                let llvalue = (0..indices.len())
                    .map(|i| self.emit_mir_rvalue(indices[&i]))
                    .collect::<Result<Vec<_>>>()?;
                Ok(self.builder.ins().array(llvalue))
            }

            mir::RvalueKind::ConstructStruct(ref members) => {
                let members = members
                    .iter()
                    .map(|&v| self.emit_mir_rvalue(v))
                    .collect::<Result<Vec<_>>>()?;
                Ok(self.builder.ins().strukt(members))
            }

            mir::RvalueKind::Const(k) => self.emit_const(k, mir.env),

            mir::RvalueKind::Index {
                value,
                base,
                length,
            } => {
                let target = self.emit_mir_rvalue(value)?;
                let base = self.emit_mir_rvalue(base)?;
                let hidden = self.emit_zero_for_type(&self.llhd_type(target));
                // TODO(fschuiki): make the above a constant of all `x`.
                let shifted = self.builder.ins().shr(target, hidden, base);
                if value.ty.is_array() {
                    if length == 0 {
                        Ok(self.builder.ins().ext_field(shifted, 0))
                    } else {
                        Ok(self.builder.ins().ext_slice(shifted, 0, length))
                    }
                } else {
                    let length = std::cmp::max(1, length);
                    Ok(self.builder.ins().ext_slice(shifted, 0, length))
                }
            }

            mir::RvalueKind::Member { value, field } => {
                let target = self.emit_mir_rvalue(value)?;
                let value = self.builder.ins().ext_field(target, field);
                // let name = format!(
                //     "{}.{}",
                //     self.builder
                //         .dfg()
                //         .get_name(target)
                //         .map(String::from)
                //         .unwrap_or_else(|| "struct".into()),
                //     field
                // );
                // self.builder.dfg_mut().set_name(value, name);
                Ok(value)
            }

            mir::RvalueKind::UnaryBitwise { op, arg } => {
                let arg = self.emit_mir_rvalue(arg)?;
                Ok(match op {
                    mir::UnaryBitwiseOp::Not => self.builder.ins().not(arg),
                })
            }

            mir::RvalueKind::BinaryBitwise { op, lhs, rhs } => {
                let lhs = self.emit_mir_rvalue(lhs)?;
                let rhs = self.emit_mir_rvalue(rhs)?;
                Ok(match op {
                    mir::BinaryBitwiseOp::And => self.builder.ins().and(lhs, rhs),
                    mir::BinaryBitwiseOp::Or => self.builder.ins().or(lhs, rhs),
                    mir::BinaryBitwiseOp::Xor => self.builder.ins().xor(lhs, rhs),
                })
            }

            mir::RvalueKind::IntComp {
                op, lhs, rhs, sign, ..
            } => {
                let lhs = self.emit_mir_rvalue(lhs)?;
                let rhs = self.emit_mir_rvalue(rhs)?;
                let signed = sign.is_signed();
                Ok(match op {
                    mir::IntCompOp::Eq => self.builder.ins().eq(lhs, rhs),
                    mir::IntCompOp::Neq => self.builder.ins().neq(lhs, rhs),
                    mir::IntCompOp::Lt if signed => self.builder.ins().slt(lhs, rhs),
                    mir::IntCompOp::Leq if signed => self.builder.ins().sle(lhs, rhs),
                    mir::IntCompOp::Gt if signed => self.builder.ins().sgt(lhs, rhs),
                    mir::IntCompOp::Geq if signed => self.builder.ins().sge(lhs, rhs),
                    mir::IntCompOp::Lt => self.builder.ins().ult(lhs, rhs),
                    mir::IntCompOp::Leq => self.builder.ins().ule(lhs, rhs),
                    mir::IntCompOp::Gt => self.builder.ins().ugt(lhs, rhs),
                    mir::IntCompOp::Geq => self.builder.ins().uge(lhs, rhs),
                })
            }

            mir::RvalueKind::IntUnaryArith { op, arg, .. } => {
                let arg = self.emit_mir_rvalue(arg)?;
                Ok(match op {
                    mir::IntUnaryArithOp::Neg => self.builder.ins().neg(arg),
                })
            }

            mir::RvalueKind::IntBinaryArith {
                op, lhs, rhs, sign, ..
            } => {
                let lhs = self.emit_mir_rvalue(lhs)?;
                let rhs = self.emit_mir_rvalue(rhs)?;
                let signed = sign.is_signed();
                Ok(match op {
                    mir::IntBinaryArithOp::Add => self.builder.ins().add(lhs, rhs),
                    mir::IntBinaryArithOp::Sub => self.builder.ins().sub(lhs, rhs),
                    mir::IntBinaryArithOp::Mul if signed => self.builder.ins().smul(lhs, rhs),
                    mir::IntBinaryArithOp::Div if signed => self.builder.ins().sdiv(lhs, rhs),
                    mir::IntBinaryArithOp::Mod if signed => self.builder.ins().smod(lhs, rhs),
                    mir::IntBinaryArithOp::Mul => self.builder.ins().umul(lhs, rhs),
                    mir::IntBinaryArithOp::Div => self.builder.ins().udiv(lhs, rhs),
                    mir::IntBinaryArithOp::Mod => self.builder.ins().umod(lhs, rhs),
                    mir::IntBinaryArithOp::Pow => {
                        self.emit(
                            DiagBuilder2::error("`**` operator on non-constants not supported")
                                .span(mir.span),
                        );
                        return Err(());
                    }
                })
            }

            mir::RvalueKind::Concat(ref values) => {
                let mut offset = 0;
                let llty = self.emit_type(mir.ty, mir.env)?;
                let mut value = self.emit_zero_for_type(&llty);
                for v in values.iter().rev() {
                    let w = v.ty.width();
                    let v = self.emit_mir_rvalue(v)?;
                    value = self.builder.ins().ins_slice(value, v, offset, w);
                    offset += w;
                }
                Ok(value)
            }

            mir::RvalueKind::Repeat(times, value) => {
                let width = value.ty.width();
                let value = self.emit_mir_rvalue(value)?;
                let llty = self.emit_type(mir.ty, mir.env)?;
                let mut result = self.emit_zero_for_type(&llty);
                for i in 0..times {
                    result = self
                        .builder
                        .ins()
                        .ins_slice(result, value, i * width, width);
                }
                Ok(result)
            }

            mir::RvalueKind::Shift {
                op,
                arith,
                value,
                amount,
            } => {
                let value = self.emit_mir_rvalue(value)?;
                let amount = self.emit_mir_rvalue(amount)?;
                let value_ty = self.builder.unit().value_type(value);
                let hidden = self.emit_zero_for_type(&value_ty);
                let hidden = if arith && op == mir::ShiftOp::Right {
                    let ones = self.builder.ins().not(hidden);
                    let sign = self
                        .builder
                        .ins()
                        .ext_slice(value, value_ty.unwrap_int() - 1, 1);
                    let mux = self.builder.ins().array(vec![hidden, ones]);
                    self.builder.ins().mux(mux, sign)
                } else {
                    hidden
                };
                Ok(match op {
                    mir::ShiftOp::Left => self.builder.ins().shl(value, hidden, amount),
                    mir::ShiftOp::Right => self.builder.ins().shr(value, hidden, amount),
                })
            }

            mir::RvalueKind::Ternary {
                cond,
                true_value,
                false_value,
            } => {
                let cond = self.emit_mir_rvalue(cond)?;
                let true_value = self.emit_mir_rvalue(true_value)?;
                let false_value = self.emit_mir_rvalue(false_value)?;
                let array = self.builder.ins().array(vec![false_value, true_value]);
                Ok(self.builder.ins().mux(array, cond))
            }

            mir::RvalueKind::Reduction { op, arg } => {
                let width = arg.ty.width();
                let arg = self.emit_mir_rvalue(arg)?;
                let mut value = self.builder.ins().ext_slice(arg, 0, 1);
                for i in 1..width {
                    let bit = self.builder.ins().ext_slice(arg, i, 1);
                    value = match op {
                        mir::BinaryBitwiseOp::And => self.builder.ins().and(value, bit),
                        mir::BinaryBitwiseOp::Or => self.builder.ins().or(value, bit),
                        mir::BinaryBitwiseOp::Xor => self.builder.ins().xor(value, bit),
                    };
                }
                Ok(value)
            }

            mir::RvalueKind::Assignment {
                lvalue,
                rvalue,
                result,
            } => {
                self.emit_mir_blocking_assign(lvalue, rvalue)?;
                self.emit_mir_rvalue(result)
            }

            mir::RvalueKind::Error => Err(()),
        }
    }

    /// Emit the code for an rvalue converted to a boolean..
    fn emit_rvalue_bool(&mut self, expr_id: NodeId, env: ParamEnv) -> Result<llhd::ir::Value> {
        let mir = self.mir_rvalue(expr_id, env);
        let mir = mir::lower::rvalue::cast_to_bool(self.cx, mir, env);
        self.emit_mir_rvalue(mir)
    }

    /// Emit the code for an lvalue.
    fn emit_lvalue(&mut self, expr_id: NodeId, env: ParamEnv) -> Result<llhd::ir::Value> {
        let mir = self.mir_lvalue(expr_id, env);
        self.emit_mir_lvalue(mir).map(|(x, _)| x)
    }

    /// Emit the code for an MIR lvalue.
    fn emit_mir_lvalue(
        &mut self,
        mir: &mir::Lvalue<'gcx>,
    ) -> Result<(llhd::ir::Value, Option<llhd::ir::Value>)> {
        // if let Some(x) = self.interned_lvalues.get(&mir.id) {
        //     x.clone()
        // } else {
        let x = self.emit_mir_lvalue_uninterned(mir);
        // self.interned_lvalues.insert(mir.id, x.clone());
        x
        // }
    }

    /// Emit the code for an MIR lvalue.
    ///
    /// The first returned value is the actually targeted lvalue. The second is
    /// a potential shadow variable that must be kept up-to-date.
    fn emit_mir_lvalue_uninterned(
        &mut self,
        mir: &mir::Lvalue<'gcx>,
    ) -> Result<(llhd::ir::Value, Option<llhd::ir::Value>)> {
        match mir.kind {
            // Variables and ports trivially return their declaration value.
            // This is either the `var` or `sig` instruction which introduced
            // them.
            mir::LvalueKind::Var(id) | mir::LvalueKind::Port(id) => Ok((
                self.emitted_value(id).clone(),
                self.shadows.get(&id).cloned(),
            )),

            // Member accesses simply look up their inner lvalue and extract the
            // signal or pointer to the respective subfield.
            mir::LvalueKind::Member { value, field } => {
                let target = self.emit_mir_lvalue(value)?;
                let value_real = self.builder.ins().ext_field(target.0, field);
                let value_shadow = target
                    .1
                    .map(|target| self.builder.ins().ext_field(target, field));
                Ok((value_real, value_shadow))
            }

            // Index accesses shift and extract the accessed slice as a signal
            // or pointer.
            mir::LvalueKind::Index {
                value,
                base,
                length,
            } => {
                let (target_real, target_shadow) = self.emit_mir_lvalue(value)?;
                let base = self.emit_mir_rvalue(base)?;
                let shifted_real = {
                    let hidden = self.emit_zero_for_type(&self.llhd_type(target_real));
                    self.builder.ins().shr(target_real, hidden, base)
                };
                let shifted_shadow = target_shadow.map(|target| {
                    let hidden = self.emit_zero_for_type(&self.llhd_type(target));
                    self.builder.ins().shr(target, hidden, base)
                });
                if value.ty.is_array() {
                    if length == 0 {
                        Ok((
                            self.builder.ins().ext_field(shifted_real, 0),
                            shifted_shadow.map(|s| self.builder.ins().ext_field(s, 0)),
                        ))
                    } else {
                        Ok((
                            self.builder.ins().ext_slice(shifted_real, 0, length),
                            shifted_shadow.map(|s| self.builder.ins().ext_slice(s, 0, length)),
                        ))
                    }
                } else {
                    let length = std::cmp::max(1, length);
                    Ok((
                        self.builder.ins().ext_slice(shifted_real, 0, length),
                        shifted_shadow.map(|s| self.builder.ins().ext_slice(s, 0, length)),
                    ))
                }
            }

            // Errors from MIR lowering have already been reported. Just abort.
            mir::LvalueKind::Error => Err(()),

            _ => {
                unimplemented!(
                    "{}",
                    DiagBuilder2::bug("codegen for mir lvalue")
                        .span(mir.span)
                        .add_note(format!("{:#?}", mir))
                );
            }
        }
    }

    /// Emit a binary shift operator.
    fn emit_shift_operator(
        &mut self,
        dir: ShiftDir,
        arith: bool,
        lhs: llhd::ir::Value,
        rhs: llhd::ir::Value,
    ) -> llhd::ir::Value {
        let ty = self.builder.dfg().value_type(lhs);
        let width = if ty.is_signal() {
            ty.unwrap_signal().unwrap_int()
        } else {
            ty.unwrap_int()
        };
        let zeros = self.builder.ins().const_int(width, 0);
        let hidden = if arith && dir == ShiftDir::Right {
            let ones = self.builder.ins().not(zeros);
            let sign = self.builder.ins().ext_slice(lhs, width - 1, 1);
            let array = self.builder.ins().array(vec![zeros, ones]);
            self.builder.ins().mux(array, sign)
        } else {
            zeros
        };
        match dir {
            ShiftDir::Left => self.builder.ins().shl(lhs, hidden, rhs),
            ShiftDir::Right => self.builder.ins().shr(lhs, hidden, rhs),
        }
    }

    /// Add a nameless block.
    fn add_nameless_block(&mut self) -> llhd::ir::Block {
        self.builder.block()
    }

    /// Add a named block.
    fn add_named_block(&mut self, name: impl Into<String>) -> llhd::ir::Block {
        let bb = self.builder.block();
        self.builder.cfg_mut().set_name(bb, name.into());
        bb
    }

    /// Emit the code for a statement.
    fn emit_stmt(&mut self, stmt_id: NodeId, env: ParamEnv) -> Result<()> {
        match self.hir_of(stmt_id)? {
            HirNode::Stmt(x) => self.emit_stmt_regular(stmt_id, x, env),
            HirNode::VarDecl(x) => self.emit_stmt_var_decl(stmt_id, x, env),
            _ => unreachable!(),
        }
    }

    /// Emit the code for a statement, given its HIR.
    fn emit_stmt_regular(
        &mut self,
        _stmt_id: NodeId,
        hir: &hir::Stmt,
        env: ParamEnv,
    ) -> Result<()> {
        #[allow(unreachable_patterns)]
        match hir.kind {
            hir::StmtKind::Null => (),
            hir::StmtKind::Block(ref ids) => {
                for &id in ids {
                    self.emit_stmt(id, env)?;
                }
            }
            hir::StmtKind::Assign { lhs, rhs, kind } => {
                // TODO(fschuiki): The following should happen in a lowering of the
                // assignment to its own MIR.
                let lhs_mir = self.mir_lvalue(lhs, env);
                let rhs_mir = self.mir_rvalue(rhs, env);
                let rhs_mir = mir::lower::rvalue::cast_to_type(self.cx, rhs_mir, env, lhs_mir.ty);
                let lhs_lv = self.emit_mir_lvalue(lhs_mir)?;
                let rhs_rv = self.emit_mir_rvalue(rhs_mir)?;

                match kind {
                    hir::AssignKind::Block(ast::AssignOp::Identity) => {
                        self.emit_blocking_assign_llhd(lhs_lv, rhs_rv)?;
                    }
                    hir::AssignKind::Block(op) => {
                        let lhs_rv = self.emit_rvalue(lhs, env)?;
                        let value = match op {
                            ast::AssignOp::Identity => unreachable!(),
                            ast::AssignOp::Add => self.builder.ins().add(lhs_rv, rhs_rv),
                            ast::AssignOp::Sub => self.builder.ins().sub(lhs_rv, rhs_rv),
                            ast::AssignOp::Mul => self.builder.ins().umul(lhs_rv, rhs_rv),
                            ast::AssignOp::Div => self.builder.ins().udiv(lhs_rv, rhs_rv),
                            ast::AssignOp::Mod => self.builder.ins().umod(lhs_rv, rhs_rv),
                            ast::AssignOp::BitAnd => self.builder.ins().and(lhs_rv, rhs_rv),
                            ast::AssignOp::BitOr => self.builder.ins().or(lhs_rv, rhs_rv),
                            ast::AssignOp::BitXor => self.builder.ins().xor(lhs_rv, rhs_rv),
                            ast::AssignOp::LogicShL => {
                                self.emit_shift_operator(ShiftDir::Left, false, lhs_rv, rhs_rv)
                            }
                            ast::AssignOp::LogicShR => {
                                self.emit_shift_operator(ShiftDir::Right, false, lhs_rv, rhs_rv)
                            }
                            ast::AssignOp::ArithShL => {
                                self.emit_shift_operator(ShiftDir::Left, true, lhs_rv, rhs_rv)
                            }
                            ast::AssignOp::ArithShR => {
                                self.emit_shift_operator(ShiftDir::Right, true, lhs_rv, rhs_rv)
                            }
                        };
                        self.emit_blocking_assign_llhd(lhs_lv, value)?;
                    }
                    hir::AssignKind::Nonblock => {
                        let delay = llhd::ConstTime::new(num::zero(), 1, 0);
                        let delay_const = self.builder.ins().const_time(delay);
                        self.builder.ins().drv(lhs_lv.0, rhs_rv, delay_const);
                    }
                    hir::AssignKind::NonblockDelay(delay) => {
                        let delay = self.emit_rvalue(delay, env)?;
                        self.builder.ins().drv(lhs_lv.0, rhs_rv, delay);
                    }
                    _ => {
                        error!("{:#?}", hir);
                        return self.unimp_msg(
                            format!("code generation for assignment {:?} in", kind),
                            hir,
                        );
                    }
                }
            }
            hir::StmtKind::Timed {
                control: hir::TimingControl::Delay(expr_id),
                stmt,
            } => {
                let resume_blk = self.add_nameless_block();
                let duration = self.emit_rvalue(expr_id, env)?.into();
                self.builder.ins().wait_time(resume_blk, duration, vec![]);
                self.builder.append_to(resume_blk);
                self.emit_shadow_update();
                self.emit_stmt(stmt, env)?;
            }
            hir::StmtKind::Timed {
                control: hir::TimingControl::ExplicitEvent(expr_id),
                stmt,
            } => {
                let expr_hir = match self.hir_of(expr_id)? {
                    HirNode::EventExpr(x) => x,
                    _ => unreachable!(),
                };
                trace!("would now emit event checking code for {:#?}", expr_hir);

                // Store initial values of the expressions the event is
                // sensitive to.
                let init_blk = self.add_named_block("init");
                self.builder.ins().br(init_blk);
                self.builder.append_to(init_blk);
                let mut init_values = vec![];
                for event in &expr_hir.events {
                    init_values.push(self.emit_rvalue(event.expr, env)?);
                }

                // Wait for any of the inputs to those expressions to change.
                let check_blk = self.add_named_block("check");
                let mut trigger_on = vec![];
                for event in &expr_hir.events {
                    let acc = self.accessed_nodes(event.expr)?;
                    for &id in &acc.read {
                        trigger_on.push(self.emitted_value(id).clone());
                    }
                }
                self.builder.ins().wait(check_blk, trigger_on);
                self.builder.append_to(check_blk);
                self.emit_shadow_update();

                // Check if any of the events happened and produce a single bit
                // value that represents this.
                let mut event_cond = None;
                for (event, init_value) in expr_hir.events.iter().zip(init_values.into_iter()) {
                    trace!(
                        "would now emit check if {:?} changed according to {:#?}",
                        init_value,
                        event
                    );
                    let now_value = self.emit_rvalue(event.expr, env)?;
                    let mut trigger = self.emit_event_trigger(event.edge, init_value, now_value)?;
                    for &iff in &event.iff {
                        let iff_value = self.emit_rvalue_bool(iff, env)?;
                        trigger = self.builder.ins().and(trigger, iff_value);
                        self.builder.dfg_mut().set_name(trigger, "iff".to_string());
                    }
                    event_cond = Some(match event_cond {
                        Some(chain) => {
                            let value = self.builder.ins().or(chain, trigger);
                            self.builder
                                .dfg_mut()
                                .set_name(value, "event_or".to_string());
                            value
                        }
                        None => trigger,
                    });
                }

                // If the event happened, branch to a new block which will
                // contain the subsequent statements. Otherwise jump back up to
                // the initial block.
                if let Some(event_cond) = event_cond {
                    let event_blk = self.add_named_block("event");
                    self.builder.ins().br_cond(event_cond, init_blk, event_blk);
                    self.builder.append_to(event_blk);
                }

                // Emit the actual statement.
                self.emit_stmt(stmt, env)?;
            }
            hir::StmtKind::Timed {
                control: hir::TimingControl::ImplicitEvent,
                stmt,
            } => {
                // Wait for any of the inputs to the statement to change.
                let trigger_blk = self.add_named_block("trigger");
                let mut trigger_on = vec![];
                let acc = self.accessed_nodes(stmt)?;
                for &id in &acc.read {
                    trigger_on.push(self.emitted_value(id).clone());
                }
                self.builder.ins().wait(trigger_blk, trigger_on);
                self.builder.append_to(trigger_blk);
                self.emit_shadow_update();

                // Emit the actual statement.
                self.emit_stmt(stmt, env)?;
            }
            hir::StmtKind::Expr(expr_id) => {
                self.emit_rvalue(expr_id, env)?;
            }
            hir::StmtKind::If {
                cond,
                main_stmt,
                else_stmt,
            } => {
                let main_blk = self.add_named_block("if_true");
                let else_blk = self.add_named_block("if_false");
                let cond = self.emit_rvalue_bool(cond, env)?;
                self.builder.ins().br_cond(cond, else_blk, main_blk);
                let final_blk = self.add_named_block("if_exit");
                self.builder.append_to(main_blk);
                self.emit_stmt(main_stmt, env)?;
                self.builder.ins().br(final_blk);
                self.builder.append_to(else_blk);
                if let Some(else_stmt) = else_stmt {
                    self.emit_stmt(else_stmt, env)?;
                };
                self.builder.ins().br(final_blk);
                self.builder.append_to(final_blk);
            }
            hir::StmtKind::Loop { kind, body } => {
                let body_blk = self.add_named_block("loop_body");
                let exit_blk = self.add_named_block("loop_exit");

                // Emit the loop initialization.
                let repeat_var = match kind {
                    hir::LoopKind::Forever => None,
                    hir::LoopKind::Repeat(count) => {
                        let ty = self.type_of(count, env)?;
                        let count = self.emit_rvalue(count, env)?;
                        let var = self.builder.ins().var(count);
                        self.builder
                            .dfg_mut()
                            .set_name(var, "loop_count".to_string());
                        Some((var, ty))
                    }
                    hir::LoopKind::While(_) => None,
                    hir::LoopKind::Do(_) => None,
                    hir::LoopKind::For(init, _, _) => {
                        self.emit_stmt(init, env)?;
                        None
                    }
                };

                // Emit the loop prologue.
                self.builder.ins().br(body_blk);
                self.builder.append_to(body_blk);
                let enter_cond = match kind {
                    hir::LoopKind::Forever => None,
                    hir::LoopKind::Repeat(_) => {
                        let (repeat_var, ty) = repeat_var.clone().unwrap();
                        let lty = self.emit_type(ty, env)?;
                        let value = self.builder.ins().ld(repeat_var);
                        let zero = self.emit_zero_for_type(&lty);
                        Some(self.builder.ins().neq(value, zero))
                    }
                    hir::LoopKind::While(cond) => Some(self.emit_rvalue_bool(cond, env)?),
                    hir::LoopKind::Do(_) => None,
                    hir::LoopKind::For(_, cond, _) => Some(self.emit_rvalue_bool(cond, env)?),
                };
                if let Some(enter_cond) = enter_cond {
                    let entry_blk = self.add_named_block("loop_continue");
                    self.builder.ins().br_cond(enter_cond, exit_blk, entry_blk);
                    self.builder.append_to(entry_blk);
                }

                // Emit the loop body.
                self.emit_stmt(body, env)?;

                // Emit the epilogue.
                let continue_cond = match kind {
                    hir::LoopKind::Forever => None,
                    hir::LoopKind::Repeat(_) => {
                        let (repeat_var, ty) = repeat_var.clone().unwrap();
                        let value = self.builder.ins().ld(repeat_var);
                        let one = self.const_one_for_type(ty, env)?;
                        let value = self.builder.ins().sub(value, one);
                        self.builder.ins().st(repeat_var, value);
                        None
                    }
                    hir::LoopKind::While(_) => None,
                    hir::LoopKind::Do(cond) => Some(self.emit_rvalue_bool(cond, env)?),
                    hir::LoopKind::For(_, _, step) => {
                        self.emit_rvalue_bool(step, env)?;
                        None
                    }
                };
                match continue_cond {
                    Some(cond) => self.builder.ins().br_cond(cond, exit_blk, body_blk),
                    None => self.builder.ins().br(body_blk),
                };
                self.builder.append_to(exit_blk);
            }
            hir::StmtKind::InlineGroup { ref stmts, .. } => {
                for &stmt in stmts {
                    self.emit_stmt(stmt, env)?;
                }
            }

            hir::StmtKind::Case {
                expr,
                ref ways,
                default,
                kind,
            } => {
                let expr = self.emit_rvalue(expr, env)?;
                let final_blk = self.add_named_block("case_exit");
                for &(ref way_exprs, stmt) in ways {
                    let mut last_check = self.builder.ins().const_int(1, 0);
                    for &way_expr in way_exprs {
                        // Determine the constant value of the label.
                        let way_const = self.constant_value_of(way_expr, env)?;
                        let (_, special_bits, x_bits) = match &way_const.kind {
                            ValueKind::Int(v, s, x) => (v, s, x),
                            _ => panic!("case constant evaluates to non-integer"),
                        };
                        let way_expr = self.emit_const(way_const, env)?;
                        let way_width = self.llhd_type(way_expr).unwrap_int();

                        // Generate the comparison mask based on the case kind.
                        let mask = match kind {
                            ast::CaseKind::Normal => None,
                            ast::CaseKind::DontCareZ => {
                                let mut mask = special_bits.clone();
                                mask.difference(x_bits);
                                mask.negate();
                                Some(mask)
                            }
                            ast::CaseKind::DontCareXZ => {
                                let mut mask = special_bits.clone();
                                mask.negate();
                                Some(mask)
                            }
                        };
                        let mask = mask.map(|bits| {
                            let mut mask = BigInt::zero();
                            for b in &bits {
                                mask <<= 1;
                                if b {
                                    mask |= BigInt::one();
                                }
                            }
                            self.builder.ins().const_int(way_width, mask)
                        });

                        // Filter the comparison values through the mask.
                        let (lhs, rhs) = match mask {
                            Some(mask) => (
                                self.builder.ins().and(expr, mask),
                                self.builder.ins().and(way_expr, mask),
                            ),
                            None => (expr, way_expr),
                        };

                        // Perform the comparison and branch.
                        let check = self.builder.ins().eq(lhs, rhs);
                        last_check = self.builder.ins().or(last_check, check);
                    }
                    let taken_blk = self.add_named_block("case_body");
                    let untaken_blk = self.add_nameless_block();
                    self.builder
                        .ins()
                        .br_cond(last_check, untaken_blk, taken_blk);
                    self.builder.append_to(taken_blk);
                    self.emit_stmt(stmt, env)?;
                    self.builder.ins().br(final_blk);
                    self.builder.append_to(untaken_blk);
                }
                if let Some(default) = default {
                    self.emit_stmt(default, env)?;
                }
                self.builder.ins().br(final_blk);
                self.builder.append_to(final_blk);
            }

            _ => {
                error!("{:#?}", hir);
                return self.unimp_msg("code generation for", hir);
            }
        }
        Ok(())
    }

    /// Emit the code for a variable declaration statement, given its HIR.
    fn emit_stmt_var_decl(
        &mut self,
        decl_id: NodeId,
        hir: &hir::VarDecl,
        env: ParamEnv,
    ) -> Result<()> {
        let ty = self.type_of(decl_id, env)?;
        let ty = self.emit_type(ty, env)?;
        let init = match hir.init {
            Some(expr) => {
                let k = self.constant_value_of(expr, env)?;
                self.emit_const(k, env)?
            }
            None => self.emit_zero_for_type(&ty),
        };
        let value = self.builder.ins().var(init);
        self.builder
            .dfg_mut()
            .set_name(value, hir.name.value.to_string());
        self.set_emitted_value(decl_id, value);
        Ok(())
    }

    /// Emit the code to check if a certain edge occurred between two values.
    fn emit_event_trigger(
        &mut self,
        edge: ast::EdgeIdent,
        prev: llhd::ir::Value,
        now: llhd::ir::Value,
    ) -> Result<llhd::ir::Value> {
        let ty = self.llhd_type(now);

        // Check if a posedge happened.
        let posedge = match edge {
            ast::EdgeIdent::Posedge | ast::EdgeIdent::Edge => {
                let zero = self.emit_zero_for_type(&ty);
                let prev_eq_0 = self.builder.ins().eq(prev, zero);
                let now_neq_0 = self.builder.ins().neq(now, zero);
                let value = self.builder.ins().and(prev_eq_0, now_neq_0);
                self.builder
                    .dfg_mut()
                    .set_name(value, "posedge".to_string());
                Some(value)
            }
            _ => None,
        };

        // Check if a negedge happened.
        let negedge = match edge {
            ast::EdgeIdent::Negedge | ast::EdgeIdent::Edge => {
                let zero = self.emit_zero_for_type(&ty);
                let prev_neq_0 = self.builder.ins().neq(prev, zero);
                let now_eq_0 = self.builder.ins().eq(now, zero);
                let value = self.builder.ins().and(now_eq_0, prev_neq_0);
                self.builder
                    .dfg_mut()
                    .set_name(value, "negedge".to_string());
                Some(value)
            }
            _ => None,
        };

        // Combine the two edge triggers, or emit an implicit edge check if none
        // of the above edges was checked.
        Ok(match (posedge, negedge) {
            (Some(a), Some(b)) => {
                let value = self.builder.ins().or(a, b);
                self.builder.dfg_mut().set_name(value, "edge".to_string());
                value
            }
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => {
                let value = self.builder.ins().neq(prev, now);
                self.builder
                    .dfg_mut()
                    .set_name(value, "impledge".to_string());
                value
            }
        })
    }

    /// Emit a blocking assignment on MIR nodes.
    fn emit_mir_blocking_assign(
        &mut self,
        lvalue: &mir::Lvalue<'gcx>,
        rvalue: &mir::Rvalue<'gcx>,
    ) -> Result<()> {
        let lv = self.emit_mir_lvalue(lvalue)?;
        let rv = self.emit_mir_rvalue(rvalue)?;
        self.emit_blocking_assign_llhd(lv, rv)
    }

    /// Emit a blocking assignment to a variable or signal.
    fn emit_blocking_assign_llhd(
        &mut self,
        lvalue: (llhd::ir::Value, Option<llhd::ir::Value>),
        rvalue: llhd::ir::Value,
    ) -> Result<()> {
        let mut assign = |lvalue| {
            let lty = self.llhd_type(lvalue);
            match *lty {
                llhd::SignalType(..) => {
                    let one_epsilon = llhd::ConstTime::new(num::zero(), 0, 1);
                    let one_epsilon = self.builder.ins().const_time(one_epsilon);
                    self.builder.ins().drv(lvalue, rvalue, one_epsilon);
                    // // Emit a wait statement to allow for the assignment to take
                    // // effect.
                    // let blk = self.add_nameless_block();
                    // self.builder.ins().wait_time(blk, one_epsilon, vec![]);
                    // self.builder.append_to(blk);
                }
                llhd::PointerType(..) => {
                    self.builder.ins().st(lvalue, rvalue);
                }
                ref t => panic!("value of type `{}` cannot be driven", t),
            }
        };
        assign(lvalue.0);
        if let Some(lv) = lvalue.1 {
            assign(lv);
        }
        Ok(())
    }

    /// Emit the code to update the shadow variables of signals.
    fn emit_shadow_update(&mut self) {
        for (id, &shadow) in &self.shadows {
            let value = self.builder.ins().prb(self.values[id]);
            self.builder.ins().st(shadow, value);
        }
    }
}

/// An rvalue emission mode.
///
/// Upon code emission, rvalues may be emitted either as direct values,
/// pointers, or signals. This enum is used to communicate the intent to the
/// corresopnding functions.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Mode {
    Value,
    // Pointer,
    Signal,
}

// /// Different binary ops that may be emitted.
// #[derive(Copy, Clone, Debug, PartialEq, Eq)]
// enum BinaryOp {
//     Add,
//     Sub,
//     And,
//     Or,
//     Xor,
// }

/// Directions of the shift operation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ShiftDir {
    Left,
    Right,
}
