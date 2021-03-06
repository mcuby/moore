// Copyright (c) 2016-2019 Fabian Schuiki

//! A port mapping generated by an instantiation.

use crate::{
    ast_map::AstNode,
    crate_prelude::*,
    hir::{HirNode, NamedParam, PosParam},
    ParamEnv,
};
use std::sync::Arc;

/// A port mapping.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct PortMapping(Vec<(NodeId, NodeEnvId)>);

impl PortMapping {
    /// Find the signal assigned to a port.
    pub fn find(&self, node_id: NodeId) -> Option<NodeEnvId> {
        self.0
            .iter()
            .find(|&&(id, _)| id == node_id)
            .map(|&(_, id)| id)
    }
}

/// A location that implies a port mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortMappingSource<'hir> {
    ModuleInst {
        module: NodeId,
        inst: NodeId,
        env: ParamEnv,
        pos: &'hir [PosParam],
        named: &'hir [NamedParam],
    },
}

pub(crate) fn compute<'gcx>(
    cx: &impl Context<'gcx>,
    src: PortMappingSource<'gcx>,
) -> Result<Arc<PortMapping>> {
    match src {
        PortMappingSource::ModuleInst {
            module,
            inst: _,
            env,
            pos,
            named,
        } => {
            let module = match cx.hir_of(module)? {
                HirNode::Module(m) => m,
                _ => panic!("expected module"),
            };

            // Associate the positional and named assignments with the actual
            // ports of the module.
            let port_iter = pos
                .iter()
                .enumerate()
                .map(
                    |(index, &(span, assign_id))| match module.ports.get(index) {
                        Some(&port_id) => Ok((port_id, (assign_id, env))),
                        None => {
                            cx.emit(
                                DiagBuilder2::error(format!(
                                    "{} only has {} ports(s)",
                                    module.desc_full(),
                                    module.ports.len()
                                ))
                                .span(span),
                            );
                            Err(())
                        }
                    },
                )
                .chain(named.iter().map(|&(_span, name, assign_id)| {
                    let names: Vec<_> = module
                        .ports
                        .iter()
                        .flat_map(|&id| match cx.ast_of(id) {
                            Ok(AstNode::Port(&ast::Port::Named { name, .. })) => {
                                Some((name.name, id))
                            }
                            Ok(_) => unreachable!(),
                            Err(()) => None,
                        })
                        .collect();
                    match names
                        .iter()
                        .find(|&(port_name, _)| *port_name == name.value)
                    {
                        Some(&(_, port_id)) => Ok((port_id, (assign_id, env))),
                        None => {
                            cx.emit(
                                DiagBuilder2::error(format!(
                                    "no port `{}` in {}",
                                    name,
                                    module.desc_full(),
                                ))
                                .span(name.span)
                                .add_note(format!(
                                    "declared ports are {}",
                                    names
                                        .iter()
                                        .map(|&(n, _)| format!("`{}`", n))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )),
                            );
                            Err(())
                        }
                    }
                }));
            let ports = port_iter
                .filter_map(|err| match err {
                    Ok((port_id, (Some(assign_id), env))) => Some(Ok((port_id, (assign_id, env)))),
                    Ok(_) => None,
                    Err(()) => Some(Err(())),
                })
                .collect::<Result<Vec<_>>>()?;

            Ok(Arc::new(PortMapping(ports)))
        }
    }
}
