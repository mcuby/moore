use crate::context::{format_symbols, Context, LlTable, Nonterm, Production, Symbol, Term};
use itertools::Itertools;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    rc::Rc,
};

/// Eliminate epsilon derivations from the grammar.
pub fn remove_epsilon_derivation(ctx: &mut Context) -> bool {
    info!("Removing ε-derivation");
    let mut modified = false;
    while remove_epsilon_derivation_inner(ctx) {
        modified = true;
    }
    modified
}

fn remove_epsilon_derivation_inner(ctx: &mut Context) -> bool {
    // Find all nonterminals which can derive epsilon and cause ambiguity.
    let mut todo = vec![];
    for (&nt, ps) in &ctx.prods {
        for &p in ps {
            if p.is_epsilon() {
                let first = ctx.first_set_of_nonterm(nt);
                let follow = ctx.follow_set(nt);
                if !first.is_disjoint(&follow) {
                    todo.push(p);
                }
            }
        }
    }

    // Process the problematic rules.
    let mut repls = HashMap::new();
    for outer in todo {
        debug!("Eliminating {}", outer);
        ctx.remove_production(outer);
        for (_, ps) in &ctx.prods {
            for &p in ps {
                if let Some(repl) = expand_epsilon(ctx, outer.nt, p) {
                    trace!("Expanding {} to {} productions", p, repl.len());
                    repls.insert(p, repl);
                }
            }
        }
    }

    // Apply the replacements.
    let modified = !repls.is_empty();
    for (p, repl) in repls {
        ctx.remove_production(p);
        for syms in repl {
            ctx.add_production(p.nt, syms);
        }
    }
    modified
}

fn expand_epsilon<'a>(
    ctx: &Context<'a>,
    nt: Nonterm<'a>,
    prod: &'a Production<'a>,
) -> Option<Vec<Vec<Symbol<'a>>>> {
    let mut leads = vec![vec![]];
    for &sym in &prod.syms {
        if sym == Symbol::Nonterm(nt) {
            for mut l in std::mem::replace(&mut leads, vec![]) {
                leads.push(l.clone());
                l.push(sym);
                leads.push(l);
            }
        } else {
            leads.iter_mut().for_each(|l| l.push(sym));
        }
    }
    if leads.len() > 1 {
        Some(leads)
    } else {
        None
    }
}

/// Remove indirect left-recursion from the grammar.
pub fn remove_indirect_left_recursion(ctx: &mut Context) -> bool {
    info!("Removing indirect left-recursion");
    for (&nt, ps) in &ctx.prods {
        find_indirect_left_recursion(
            ctx,
            nt,
            nt,
            &mut Default::default(),
            &mut Default::default(),
        );
    }
    false
}

fn find_indirect_left_recursion<'a>(
    ctx: &Context<'a>,
    root: Nonterm<'a>,
    nt: Nonterm<'a>,
    seen: &mut HashSet<Nonterm<'a>>,
    stack: &mut Vec<&'a Production<'a>>,
) {
    for &p in &ctx.prods[&nt] {
        if let Some(Symbol::Nonterm(first)) = p.syms.iter().cloned().next() {
            if first == root {
                if seen.contains(&first) {
                    error!("Unhandled indirect left-recursion in {}", root);
                    for s in stack.iter() {
                        error!("  {}", s);
                    }
                }
            } else if !seen.contains(&first) {
                seen.insert(first);
                stack.push(p);
                find_indirect_left_recursion(ctx, root, first, seen, stack);
                seen.remove(&first);
                stack.pop();
            }
        }
    }
}

/// Remove left-recursion from the grammar.
pub fn remove_direct_left_recursion(ctx: &mut Context) -> bool {
    info!("Removing direct left-recursion");

    // Find the left-recursive NTs.
    let mut rec = vec![];
    for (&nt, ps) in &ctx.prods {
        let left_rec: HashSet<_> = ps
            .iter()
            .cloned()
            .filter(|p| p.syms.iter().next() == Some(&Symbol::Nonterm(p.nt)))
            .collect();
        if !left_rec.is_empty() {
            rec.push((nt, left_rec));
        }
    }

    // Remove left-recursions.
    let modified = !rec.is_empty();
    for (nt, left_rec) in rec {
        debug!("Removing left-recursion in {}", nt);
        let aux = ctx.anonymous_nonterm();

        // Add the recursive productions to the new nonterminal.
        for p in left_rec {
            let mut new_syms: Vec<_> = p.syms.iter().skip(1).cloned().collect();
            new_syms.push(Symbol::Nonterm(aux));
            ctx.add_production(aux, new_syms);
            ctx.remove_production(p);
        }
        ctx.add_production(aux, vec![]);

        // Update the non-recursive productions in the old non-terminal.
        for p in ctx.prods[&nt].clone() {
            let mut new_syms = p.syms.clone();
            new_syms.push(Symbol::Nonterm(aux));
            ctx.add_production(nt, new_syms);
            ctx.remove_production(p);
        }
    }
    modified
}

/// Perform a simple left-factorization of the grammar.
pub fn left_factorize_simple(ctx: &mut Context) -> bool {
    info!("Left-factoring grammar (simple)");

    // Identify ambiguous rules that require factoring.
    let mut conflicts = vec![];
    for (&nt, ps) in &ctx.prods {
        if has_conflict(ctx, ps) {
            conflicts.push((
                nt,
                ctx.prods[&nt]
                    .iter()
                    .cloned()
                    .filter(|p| !p.is_epsilon())
                    .collect(),
            ));
        }
    }

    // Refactor those rules.
    let mut modified = false;
    for (nt, ps) in conflicts {
        modified |= left_factorize_conflict(ctx, nt, ps);
    }

    // // Find all prefixes across the grammar.
    // let mut todo = vec![];
    // for (&nt, ps) in &ctx.prods {
    //     if ps.len() < 2 {
    //         continue;
    //     }
    //     let mut prefix = vec![];
    //     for i in 0.. {
    //         let firsts: BTreeSet<Option<Symbol>> =
    //             ps.iter().map(|p| p.syms.get(i).cloned()).collect();
    //         if firsts.len() != 1 || firsts.iter().next().unwrap().is_none() {
    //             break;
    //         }
    //         prefix.push(firsts.iter().next().unwrap().unwrap());
    //     }
    //     if !prefix.is_empty() {
    //         todo.push((nt, prefix));
    //     }
    // }

    // // Rewrite the productions.
    // let modified = !todo.is_empty();
    // for (nt, prefix) in todo {
    //     debug!("Factoring {} out of {}", format_symbols(&prefix), nt);
    // }

    false
}

fn has_conflict<'a>(ctx: &Context<'a>, ps: &BTreeSet<&Production<'a>>) -> bool {
    let mut seen = HashSet::new();
    for p in ps {
        for s in ctx.first_set_of_symbols(&p.syms) {
            if !seen.insert(s) {
                return true;
            }
        }
    }
    false
}

fn left_factorize_conflict<'a>(
    ctx: &mut Context<'a>,
    nt: Nonterm<'a>,
    mut todo: BTreeSet<&'a Production<'a>>,
) -> bool {
    let mut firsts: HashMap<&Production, BTreeSet<Term>> = todo
        .iter()
        .map(|&p| (p, ctx.first_set_of_symbols(&p.syms)))
        .collect();
    let mut modified = false;
    while let Some(&init) = todo.iter().next() {
        todo.remove(&init);
        let mut colliders = BTreeSet::new();
        colliders.insert(init);
        let mut seen = HashSet::new();
        seen.extend(firsts[init].iter().cloned());
        for s in std::mem::take(&mut todo) {
            let mut any_hit = false;
            for &f in &firsts[&s] {
                if seen.contains(&f) {
                    any_hit = true;
                    break;
                }
            }
            if any_hit {
                seen.extend(firsts[&s].iter().cloned());
                colliders.insert(s);
            } else {
                todo.insert(s);
            }
        }
        if colliders.len() > 1 {
            modified |= left_factorize_disambiguate(ctx, nt, colliders.into_iter().collect());
        }
    }
    modified
}

fn left_factorize_disambiguate<'a>(
    ctx: &mut Context<'a>,
    nt: Nonterm<'a>,
    prods: Vec<&'a Production<'a>>,
) -> bool {
    // trace!("  Disambiguate:");
    // for p in &prods {
    //     trace!("    {}", p.syms.iter().format(" "));
    // }

    // Find a common prefix, considering balanced tokens to factor out parts of
    // the rules.
    let mut prefix = vec![];
    let mut offsets: Vec<usize> = prods.iter().map(|_| 0).collect();
    loop {
        // Find the set of next symbols in the rules.
        let symbols: HashSet<_> = prods
            .iter()
            .zip(offsets.iter())
            .map(|(p, &offset)| p.syms.get(offset))
            .collect();

        // Check if we have one unique prefix symbol.
        if symbols.len() != 1 {
            break;
        }
        let symbol = match symbols.into_iter().next().unwrap() {
            Some(&p) => p,
            None => break,
        };

        // If the symbol is the left of a balanced pair, advance ahead to its
        // counterpart and gobble up the symbols in between.
        prefix.push(symbol);
        let balanced_end = match symbol.to_string().as_str() {
            "'('" => ctx.lookup_symbol("')'"),
            "'['" => ctx.lookup_symbol("']'"),
            "'{'" => ctx.lookup_symbol("'}'"),
            _ => None,
        };
        if let Some(balanced_end) = balanced_end {
            // trace!("    Factoring-out balanced {} {}", symbol, balanced_end);
            let mut subseqs = vec![];
            for (p, offset) in prods.iter().zip(offsets.iter_mut()) {
                *offset += 1;
                let mut subsyms = vec![];
                while p.syms[*offset] != balanced_end {
                    subsyms.push(p.syms[*offset]);
                    *offset += 1;
                }
                *offset += 1;
                subseqs.push(subsyms);
            }
            // trace!("  Gobbled up subsequences:");
            let aux = ctx.anonymous_nonterm();
            for s in subseqs {
                // trace!("    {}", s.iter().format(" "));
                ctx.add_production(aux, s);
            }
            prefix.push(Symbol::Nonterm(aux));
            prefix.push(balanced_end);
        } else {
            offsets.iter_mut().for_each(|o| *o += 1);
        }
    }
    // trace!("  Prefix {}", format_symbols(&prefix));
    if prefix.is_empty() {
        return false;
    }
    debug!("Factoring {} out from {}", format_symbols(&prefix), nt);

    // Compute the tails that are left over after prefix extraction.
    let tails: BTreeSet<_> = prods
        .iter()
        .zip(offsets.iter())
        .map(|(p, &offset)| &p.syms[offset..])
        .collect();

    // If there is one common tail, add that to the prefix immediately.
    // Otherwise just go ahead and create an auxiliary nonterminal that will
    // contain all of the tails.
    if tails.len() == 1 {
        let tail = tails.into_iter().next().unwrap();
        if !tail.is_empty() {
            // trace!("  Adding unique tail {}", format_symbols(tail));
            prefix.extend(tail);
        }
    } else {
        let aux = ctx.anonymous_nonterm();
        // trace!("  Adding auxiliary tail {}:", aux);
        prefix.push(Symbol::Nonterm(aux));
        for tail in tails {
            let p = ctx.add_production(aux, tail.to_vec());
            // trace!("    {}", p);
        }
    }

    // Actually replace the production.
    // trace!("  Replacing:");
    for &p in &prods {
        // trace!("    {}", p);
        ctx.remove_production(p);
    }
    // trace!("  With:");
    let p = ctx.add_production(nt, prefix);
    // trace!("    {}", p);

    true
}

/// Left-factor the grammar.
pub fn left_factor(ctx: &mut Context) {
    info!("Left-factoring grammar");

    for i in 0..20 {
        debug!("Iteration {}", i);

        // Identify ambiguous rules that require factoring.
        let mut conflicts = vec![];
        for (&nt, ps) in &ctx.prods {
            if has_conflict(ctx, ps) {
                conflicts.push((nt, ctx.prods[&nt].iter().cloned().collect()));
            }
        }

        // Refactor those rules.
        for (nt, ps) in conflicts {
            handle_conflict(ctx, nt, ps, &mut Default::default());
            // std::io::stdin().read_line(&mut String::new());
        }
    }
}

fn handle_conflict<'a>(
    ctx: &mut Context<'a>,
    nt: Nonterm<'a>,
    prods: BTreeSet<&'a Production<'a>>,
    stack: &mut HashSet<Vec<Symbol<'a>>>,
) {
    debug!("Conflict in {}", nt);
    for p in &prods {
        trace!("  {}", p);
    }

    let mut todo: BTreeSet<_> = prods.into_iter().filter(|p| !p.syms.is_empty()).collect();
    let mut firsts: HashMap<&Production, BTreeSet<Term>> = todo
        .iter()
        .map(|&p| (p, ctx.first_set_of_symbols(&p.syms)))
        .collect();
    while let Some(&init) = todo.iter().next() {
        todo.remove(&init);
        let mut colliders = BTreeSet::new();
        colliders.insert(init);
        let mut seen = HashSet::new();
        seen.extend(firsts[init].iter().cloned());
        // trace!("Starting with {} firsts {:?}", init, seen);
        for s in std::mem::take(&mut todo) {
            // trace!("  {} firsts {:?}", s, firsts[&s]);
            let mut any_hit = false;
            for &f in &firsts[&s] {
                if seen.contains(&f) {
                    any_hit = true;
                    break;
                }
            }
            if any_hit {
                seen.extend(firsts[&s].iter().cloned());
                colliders.insert(s);
            } else {
                todo.insert(s);
            }
        }
        // trace!("Colliders {:?}", colliders);
        disambiguate(ctx, nt, colliders, stack);
    }
}

fn disambiguate<'a>(
    ctx: &mut Context<'a>,
    nt: Nonterm<'a>,
    prods: BTreeSet<&'a Production<'a>>,
    stack: &mut HashSet<Vec<Symbol<'a>>>,
) {
    // Handle the trivial case.
    if prods.len() == 1 {
        return;
    }
    trace!("  Disambiguate:");
    for p in &prods {
        trace!("    {}", p.syms.iter().format(" "));
    }

    // Check for the special case of unexpanded nonterminals already matching.
    let firsts: BTreeSet<_> = prods.iter().map(|p| p.syms[0]).collect();
    let done: Vec<_> = if firsts.len() == 1 {
        prods.iter().map(|p| p.syms.to_vec()).collect()
    } else {
        // Fully expand nonterminals in first place.
        #[derive(Debug, Clone)]
        struct Lead<'a> {
            parent: Option<Rc<Lead<'a>>>,
            syms: Vec<Symbol<'a>>,
        }
        let mut done = vec![];
        let mut leads: Vec<Lead<'a>> = prods
            .iter()
            .map(|p| Lead {
                parent: None,
                syms: p.syms.to_vec(),
            })
            .collect();

        while !leads.is_empty() {
            for lead in std::mem::take(&mut leads) {
                if lead.syms.is_empty() {
                    continue;
                }
                match lead.syms[0] {
                    Symbol::Nonterm(nt) => {
                        let parent = Rc::new(lead);
                        for p in &ctx.prods[&nt] {
                            leads.push(Lead {
                                parent: Some(parent.clone()),
                                syms: p
                                    .syms
                                    .iter()
                                    .cloned()
                                    .chain(parent.syms.iter().skip(1).cloned())
                                    .collect(),
                            });
                        }
                    }
                    _ => {
                        done.push(lead);
                    }
                }
            }
        }
        // trace!("  Fully unrolled: {:?}", done);

        // Step-by-step revert the expansion as long as all leads match.
        loop {
            let firsts: BTreeSet<_> = done
                .iter()
                .map(|lead| lead.parent.as_ref().map(|p| p.syms[0]))
                .collect();
            if firsts.len() == 1 && firsts.iter().next().unwrap().is_some() {
                for lead in std::mem::take(&mut done) {
                    done.push((*lead.parent.unwrap()).clone());
                }
            } else {
                break;
            }
        }
        let mut done: Vec<_> = done.into_iter().map(|lead| lead.syms).collect();
        done.sort();
        done.dedup();
        done
    };

    trace!("  Expanded:");
    for d in &done {
        trace!("    {}", format_symbols(&d));
    }

    // No need to further disambiguate if we have only one lead.
    if done.len() <= 1 {
        return;
    }

    // Find a common prefix, considering balanced tokens to factor out parts of
    // the rules.
    let mut prefix = vec![];
    let mut offsets: Vec<usize> = done.iter().map(|_| 0).collect();
    loop {
        // Find the set of next symbols in the rules.
        let symbols: HashSet<_> = done
            .iter()
            .zip(offsets.iter())
            .map(|(syms, &offset)| syms.get(offset))
            .collect();

        // Check if we have one unique prefix symbol.
        if symbols.len() != 1 {
            break;
        }
        let symbol = match symbols.into_iter().next().unwrap() {
            Some(&p) => p,
            None => break,
        };

        // If the symbol is the left of a balanced pair, advance ahead to its
        // counterpart and gobble up the symbols in between.
        prefix.push(symbol);
        let balanced_end = match symbol.to_string().as_str() {
            "'('" => ctx.lookup_symbol("')'"),
            "'['" => ctx.lookup_symbol("']'"),
            "'{'" => ctx.lookup_symbol("'}'"),
            _ => None,
        };
        if let Some(balanced_end) = balanced_end {
            trace!("    Factoring-out balanced {} {}", symbol, balanced_end);
            let mut subseqs = vec![];
            for (syms, offset) in done.iter().zip(offsets.iter_mut()) {
                *offset += 1;
                let mut subsyms = vec![];
                while syms[*offset] != balanced_end {
                    subsyms.push(syms[*offset]);
                    *offset += 1;
                }
                *offset += 1;
                subseqs.push(subsyms);
            }
            // trace!("  Gobbled up subsequences:");
            let aux = ctx.anonymous_nonterm();
            for s in subseqs {
                // trace!("    {}", s.iter().format(" "));
                ctx.add_production(aux, s);
            }
            prefix.push(Symbol::Nonterm(aux));
            prefix.push(balanced_end);
        } else {
            offsets.iter_mut().for_each(|o| *o += 1);
        }
    }
    trace!("  Prefix {}", format_symbols(&prefix));

    // Compute the tails that are left over after prefix extraction.
    let tails: BTreeSet<_> = done
        .iter()
        .zip(offsets.iter())
        .map(|(syms, &offset)| &syms[offset..])
        .collect();

    // If there is one common tail, add that to the prefix immediately.
    // Otherwise just go ahead and create an auxiliary nonterminal that will
    // contain all of the tails.
    if tails.len() == 1 {
        let tail = tails.into_iter().next().unwrap();
        if !tail.is_empty() {
            trace!("  Adding unique tail {}", format_symbols(tail));
            prefix.extend(tail);
        }
    } else {
        let aux = ctx.anonymous_nonterm();
        trace!("  Adding auxiliary tail {}:", aux);
        prefix.push(Symbol::Nonterm(aux));
        for tail in tails {
            let p = ctx.add_production(aux, tail.to_vec());
            trace!("    {}", p);
        }
    }

    // Actually replace the production.
    trace!("  Replacing:");
    for p in &prods {
        trace!("    {}", p);
        ctx.remove_production(p);
    }
    trace!("  With:");
    let p = ctx.add_production(nt, prefix);
    trace!("    {}", p);

    // // Find common prefices and suffices.
    // let mut prefix = vec![];
    // loop {
    //     let set: HashSet<_> = done
    //         .iter()
    //         .map(|syms| syms.iter().skip(prefix.len()).next())
    //         .collect();
    //     if set.len() == 1 {
    //         if let Some(p) = set.into_iter().next().unwrap() {
    //             prefix.push(p);
    //         } else {
    //             break;
    //         }
    //     } else {
    //         break;
    //     }
    // }
    // let mut suffix = vec![];
    // loop {
    //     let set: HashSet<_> = done
    //         .iter()
    //         .map(|syms| syms.iter().rev().skip(suffix.len()).next())
    //         .collect();
    //     if set.len() == 1 {
    //         if let Some(s) = set.into_iter().next().unwrap() {
    //             suffix.push(s);
    //         } else {
    //             break;
    //         }
    //     } else {
    //         break;
    //     }
    // }
    // trace!(
    //     "  [{} ... {}]",
    //     prefix.iter().format(" "),
    //     suffix.iter().format(" ")
    // );

    // // Disambiguate whatever is left.
    // let set: BTreeSet<_> = done
    //     .iter()
    //     .map(|d| &d[prefix.len()..d.len() - suffix.len()])
    //     .collect();
    // if set.iter().any(|&s| stack.contains(s)) {
    //     warn!("Recursion in disambiguation:");
    //     for s in &set {
    //         warn!("  {}", s.iter().format(" "));
    //     }
    //     return;
    // }
    // for s in &set {
    //     stack.insert(s.to_vec());
    // }
    // handle_conflict(ctx, set.clone(), stack);
    // for &s in &set {
    //     stack.remove(s);
    // }

    // // Find the most shallow expansion possible that produces a common prefix
    // // for all sequences.
    // let mut expansions = HashMap::<Symbol, HashMap<usize, usize>>::new();
    // for (seq_idx, seq) in seqs.iter().enumerate() {
    //     let mut leads = vec![seq[0]];
    //     for level in 0.. {
    //         for sym in std::mem::take(&mut leads) {
    //             expansions
    //                 .entry(sym)
    //                 .or_default()
    //                 .entry(seq_idx)
    //                 .or_insert(level);
    //             match sym {
    //                 Symbol::Nonterm(nt) => {
    //                     leads.extend(ctx.prods[&nt].iter().flat_map(|p| p.syms.iter().next()))
    //                 }
    //                 _ => (),
    //             }
    //         }
    //         if leads.is_empty() {
    //             break;
    //         }
    //     }
    // }
    // trace!("Expansions: {:?}", expansions);
}
