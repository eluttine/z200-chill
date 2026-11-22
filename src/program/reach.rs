use crate::ast::{Action, Declaration, Module, Program, VisibilityKind};
use crate::lexer::Span;
use crate::modes::ModeTable;
use crate::types::{ProcType, Type};
use std::collections::{HashMap, HashSet};

use super::{
    apply_forbid, collect_interface, grant_prefixes, resolve_mode_with, signature_from_params_with,
    Error, ModuleInterface,
};

/// Static structure shared by the checker, interpreter, and grant emitter:
/// the interface of every module and the resolved export table.
#[derive(Debug, Clone)]
pub enum ReachExportKind {
    Var(Type),
    Proc(ProcType),
    Mode(Type),
}

#[derive(Debug, Clone)]
pub struct ReachExport {
    pub kind: ReachExportKind,
    pub meta: crate::reach::ExportMeta,
    pub owner: Span,
}

#[derive(Debug, Clone)]
pub struct ReachNode {
    pub span: Span,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// True when the module is directly in a module body (not inside an action block).
    pub hoisted: bool,
    pub interface: ModuleInterface,
    /// Names granted into this reach by directly hoisted child modules.
    pub exports: HashMap<String, Vec<ReachExport>>,
}

/// Shared static reach tree for modules, including nested modules.
#[derive(Debug, Clone, Default)]
pub struct ReachTree {
    pub nodes: Vec<ReachNode>,
    pub by_span: HashMap<Span, usize>,
}

impl ReachTree {
    pub fn get(&self, span: Span) -> Option<&ReachNode> {
        self.by_span.get(&span).map(|index| &self.nodes[*index])
    }
}

fn collect_child_exports(
    module: &Module,
    span_modes: &HashMap<Span, HashMap<String, Type>>,
    modes: &mut ModeTable,
) -> Result<HashMap<String, Vec<ReachExport>>, Error> {
    let mut exports: HashMap<String, Vec<ReachExport>> = HashMap::new();
    for action in &module.actions {
        let Action::Module(child) = action else {
            continue;
        };
        for (name, entries) in collect_reach_exports(child, span_modes, modes)? {
            exports.entry(name).or_default().extend(entries);
        }
    }
    Ok(exports)
}

fn add_block_reach(span: Span, parent: usize, tree: &mut ReachTree) -> usize {
    let id = tree.nodes.len();
    tree.by_span.insert(span, id);
    tree.nodes.push(ReachNode {
        span,
        parent: Some(parent),
        children: Vec::new(),
        hoisted: false,
        interface: ModuleInterface::default(),
        exports: HashMap::new(),
    });
    tree.nodes[parent].children.push(id);
    id
}

fn collect_block_exports(
    actions: &[Action],
    span_modes: &HashMap<Span, HashMap<String, Type>>,
    modes: &mut ModeTable,
) -> Result<HashMap<String, Vec<ReachExport>>, Error> {
    let mut exports: HashMap<String, Vec<ReachExport>> = HashMap::new();
    for action in actions {
        let Action::Module(child) = action else {
            continue;
        };
        for (name, entries) in collect_reach_exports(child, span_modes, modes)? {
            exports.entry(name).or_default().extend(entries);
        }
    }
    Ok(exports)
}

fn collect_reach_exports(
    module: &Module,
    span_modes: &HashMap<Span, HashMap<String, Type>>,
    modes: &mut ModeTable,
) -> Result<HashMap<String, Vec<ReachExport>>, Error> {
    let mode_names = span_modes.get(&module.span).cloned().unwrap_or_default();

    let mut grantable: HashMap<String, ReachExportKind> = HashMap::new();
    for declaration in &module.declarations {
        match declaration {
            Declaration::Var(list) => {
                for var in list {
                    if let Ok(ty) = resolve_mode_with(&var.mode, &mode_names) {
                        grantable.insert(var.name.clone(), ReachExportKind::Var(ty));
                    }
                }
            }
            Declaration::Proc(proc_def) => {
                if let Ok(signature) = signature_from_params_with(
                    &proc_def.params,
                    &proc_def.result,
                    &proc_def.exceptions,
                    &mode_names,
                ) {
                    grantable.insert(proc_def.name.clone(), ReachExportKind::Proc(signature));
                }
            }
            Declaration::Mode(mode_def) => {
                if let Some(Type::Struct(_) | Type::Moreta(_)) = mode_names.get(&mode_def.name) {
                    grantable.insert(
                        mode_def.name.clone(),
                        ReachExportKind::Mode(mode_names[&mode_def.name]),
                    );
                }
            }
            _ => {}
        }
    }

    for action in &module.actions {
        let Action::Module(child) = action else {
            continue;
        };
        for (name, entries) in collect_reach_exports(child, span_modes, modes)? {
            for entry in entries {
                if entry.meta.prefix.is_empty() {
                    grantable.entry(name.clone()).or_insert(entry.kind);
                }
            }
        }
    }

    let mut exports: HashMap<String, Vec<ReachExport>> = HashMap::new();
    let mut granted = HashSet::new();
    for declaration in &module.declarations {
        let Declaration::Visibility(visibility) = declaration else {
            continue;
        };
        if visibility.kind != VisibilityKind::Grant {
            continue;
        }
        let (old, new) = grant_prefixes(visibility, module)?;
        if !old.is_empty() {
            return Err(Error::new(
                visibility.span,
                "non-empty old prefixes in a nested GRANT are not supported yet",
            ));
        }
        let friends = if visibility.friends.is_empty() {
            None
        } else {
            Some(
                visibility
                    .friends
                    .iter()
                    .map(|friend| friend.qualified())
                    .collect::<Vec<_>>(),
            )
        };
        let names: Vec<String> = if visibility.all {
            grantable.keys().cloned().collect()
        } else {
            visibility
                .names
                .iter()
                .map(|name| name.name.clone())
                .collect()
        };
        for name in names {
            if !granted.insert(name.clone()) {
                return Err(Error::new(
                    visibility.span,
                    format!("`{name}` is granted more than once"),
                ));
            }
            let Some(kind) = grantable.get(&name).cloned() else {
                return Err(Error::new(
                    visibility.span,
                    format!(
                        "cannot GRANT `{name}`: not a declaration or child grant in this reach"
                    ),
                ));
            };
            if let Some(forbid) = &visibility.forbid {
                match &kind {
                    ReachExportKind::Mode(Type::Struct(id)) => {
                        apply_forbid(modes.get_mut(*id), &name, forbid, visibility.span)?;
                    }
                    _ => {
                        return Err(Error::new(
                            visibility.span,
                            format!("FORBID requires a structure mode, not `{name}`"),
                        ));
                    }
                }
            }
            exports.entry(name).or_default().push(ReachExport {
                kind,
                meta: crate::reach::ExportMeta {
                    prefix: new.clone(),
                    friends: friends.clone(),
                },
                owner: module.span,
            });
        }
    }
    Ok(exports)
}

pub(super) fn build_reach_tree(
    program: &Program,
    interfaces: &[ModuleInterface],
    span_modes: &HashMap<Span, HashMap<String, Type>>,
    modes: &mut ModeTable,
) -> Result<ReachTree, Error> {
    let mut tree = ReachTree::default();
    for (index, module) in program.modules.iter().enumerate() {
        let id = tree.nodes.len();
        tree.by_span.insert(module.span, id);
        tree.nodes.push(ReachNode {
            span: module.span,
            parent: None,
            children: Vec::new(),
            hoisted: false,
            interface: interfaces[index].clone(),
            exports: HashMap::new(),
        });
        add_nested_reaches(&module.actions, id, false, &mut tree, span_modes, modes)?;
        tree.nodes[id].exports = collect_child_exports(module, span_modes, modes)?;
    }
    Ok(tree)
}

fn add_nested_reaches(
    actions: &[Action],
    parent: usize,
    in_block: bool,
    tree: &mut ReachTree,
    span_modes: &HashMap<Span, HashMap<String, Type>>,
    modes: &mut ModeTable,
) -> Result<(), Error> {
    for action in actions {
        match action {
            Action::Module(module) => {
                let local_modes = span_modes.get(&module.span).cloned().unwrap_or_default();
                let interface = collect_interface(module, &local_modes)?;
                let id = tree.nodes.len();
                tree.by_span.insert(module.span, id);
                tree.nodes.push(ReachNode {
                    span: module.span,
                    parent: Some(parent),
                    children: Vec::new(),
                    hoisted: !in_block,
                    interface,
                    exports: HashMap::new(),
                });
                tree.nodes[parent].children.push(id);
                add_nested_reaches(&module.actions, id, false, tree, span_modes, modes)?;
                tree.nodes[id].exports = collect_child_exports(module, span_modes, modes)?;
            }
            Action::If {
                branches,
                else_actions,
                else_span,
                ..
            } => {
                for branch in branches {
                    let id = add_block_reach(branch.span, parent, tree);
                    add_nested_reaches(&branch.body, id, true, tree, span_modes, modes)?;
                    tree.nodes[id].exports =
                        collect_block_exports(&branch.body, span_modes, modes)?;
                }
                if let (Some(actions), Some(span)) = (else_actions, else_span) {
                    let id = add_block_reach(*span, parent, tree);
                    add_nested_reaches(actions, id, true, tree, span_modes, modes)?;
                    tree.nodes[id].exports = collect_block_exports(actions, span_modes, modes)?;
                }
            }
            Action::Do { body, span, .. } => {
                let id = add_block_reach(*span, parent, tree);
                add_nested_reaches(body, id, true, tree, span_modes, modes)?;
                tree.nodes[id].exports = collect_block_exports(body, span_modes, modes)?;
            }
            Action::Case {
                alternatives,
                else_actions,
                else_span,
                ..
            } => {
                for alternative in alternatives {
                    let id = add_block_reach(alternative.span, parent, tree);
                    add_nested_reaches(&alternative.body, id, true, tree, span_modes, modes)?;
                    tree.nodes[id].exports =
                        collect_block_exports(&alternative.body, span_modes, modes)?;
                }
                if let (Some(actions), Some(span)) = (else_actions, else_span) {
                    let id = add_block_reach(*span, parent, tree);
                    add_nested_reaches(actions, id, true, tree, span_modes, modes)?;
                    tree.nodes[id].exports = collect_block_exports(actions, span_modes, modes)?;
                }
            }
            Action::Labelled { action, .. } => add_nested_reaches(
                std::slice::from_ref(action.as_ref()),
                parent,
                in_block,
                tree,
                span_modes,
                modes,
            )?,
            Action::Begin { actions, span, .. } => {
                let id = add_block_reach(*span, parent, tree);
                add_nested_reaches(actions, id, true, tree, span_modes, modes)?;
                tree.nodes[id].exports = collect_block_exports(actions, span_modes, modes)?;
            }
            Action::Handled { action, .. } => add_nested_reaches(
                std::slice::from_ref(action.as_ref()),
                parent,
                in_block,
                tree,
                span_modes,
                modes,
            )?,
            _ => {}
        }
    }
    Ok(())
}
