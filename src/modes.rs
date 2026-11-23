use crate::types::{ModeId, MoretaId, ProcType, SetId, Type};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct StructMode {
    pub name: String,
    pub package: usize,
    pub module: usize,
    pub package_name: Option<String>,
    pub module_name: Option<String>,
    pub fields: Vec<FieldInfo>,
    /// Fields that may only be selected inside the defining module.
    pub forbidden: HashSet<String>,
}

impl StructMode {
    /// Stable symbolic identity used by grant files.
    pub fn symbol(&self) -> String {
        mode_symbol(
            self.package_name.as_deref(),
            self.module_name.as_deref(),
            &self.name,
        )
    }
}

/// Build the symbolic name `package!Module!Mode` for a structure mode.
pub fn mode_symbol(package: Option<&str>, module: Option<&str>, name: &str) -> String {
    match (package, module) {
        (Some(package), Some(module)) => format!("{package}!{module}!{name}"),
        (None, Some(module)) => format!("{module}!{name}"),
        _ => name.to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentVisibility {
    Public,
    Internal,
    Private,
}

#[derive(Debug, Clone)]
pub enum MoretaComponent {
    Var {
        name: String,
        ty: Type,
        visibility: ComponentVisibility,
    },
    Proc {
        name: String,
        signature: ProcType,
        span: crate::lexer::Span,
        constructor: bool,
        destructor: bool,
        visibility: ComponentVisibility,
        implemented: bool,
    },
    Mode {
        name: String,
        ty: Type,
        visibility: ComponentVisibility,
    },
}

impl MoretaComponent {
    pub fn name(&self) -> &str {
        match self {
            Self::Var { name, .. } | Self::Proc { name, .. } | Self::Mode { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MoretaMode {
    pub name: String,
    pub kind: crate::ast::MoretaKind,
    pub package: usize,
    pub module: usize,
    pub package_name: Option<String>,
    pub module_name: Option<String>,
    pub bases: Vec<MoretaId>,
    pub implements: Vec<MoretaId>,
    pub abstract_mode: bool,
    pub final_mode: bool,
    pub assignable: bool,
    pub components: Vec<MoretaComponent>,
}

impl MoretaMode {
    /// Stable symbolic identity used by grant files.
    pub fn symbol(&self) -> String {
        mode_symbol(
            self.package_name.as_deref(),
            self.module_name.as_deref(),
            &self.name,
        )
    }
}

#[derive(Debug, Clone)]
pub struct SetMode {
    pub name: String,
    pub package: usize,
    pub module: usize,
    pub package_name: Option<String>,
    pub module_name: Option<String>,
    pub elements: Vec<(String, i128)>,
}

#[derive(Debug, Clone, Default)]
pub struct ModeTable {
    pub modes: Vec<StructMode>,
    pub sets: Vec<SetMode>,
    pub moretas: Vec<MoretaMode>,
}

impl ModeTable {
    pub fn get(&self, id: ModeId) -> &StructMode {
        &self.modes[id.0]
    }

    pub fn get_mut(&mut self, id: ModeId) -> &mut StructMode {
        &mut self.modes[id.0]
    }

    pub fn get_moreta(&self, id: MoretaId) -> &MoretaMode {
        &self.moretas[id.0]
    }

    pub fn moreta_component(&self, id: MoretaId, name: &str) -> Option<&MoretaComponent> {
        self.get_moreta(id)
            .components
            .iter()
            .find(|component| component.name() == name)
    }

    pub fn moreta_var(&self, id: MoretaId, name: &str) -> Option<Type> {
        match self.moreta_component(id, name)? {
            MoretaComponent::Var { ty, .. } => Some(*ty),
            MoretaComponent::Proc { .. } | MoretaComponent::Mode { .. } => None,
        }
    }

    pub fn moreta_visibility(&self, id: MoretaId, name: &str) -> Option<ComponentVisibility> {
        Some(match self.moreta_component(id, name)? {
            MoretaComponent::Var { visibility, .. }
            | MoretaComponent::Proc { visibility, .. }
            | MoretaComponent::Mode { visibility, .. } => *visibility,
        })
    }

    pub fn moreta_proc(&self, id: MoretaId, name: &str) -> Option<(ProcType, crate::lexer::Span)> {
        match self.moreta_component(id, name)? {
            MoretaComponent::Proc {
                signature, span, ..
            } => Some((signature.clone(), *span)),
            MoretaComponent::Var { .. } | MoretaComponent::Mode { .. } => None,
        }
    }

    pub fn moreta_constructor(&self, id: MoretaId) -> Option<(ProcType, crate::lexer::Span)> {
        self.get_moreta(id)
            .components
            .iter()
            .find_map(|component| match component {
                MoretaComponent::Proc {
                    signature,
                    span,
                    constructor: true,
                    ..
                } => Some((signature.clone(), *span)),
                _ => None,
            })
    }

    pub fn moreta_destructor(&self, id: MoretaId) -> Option<(ProcType, crate::lexer::Span)> {
        self.get_moreta(id)
            .components
            .iter()
            .find_map(|component| match component {
                MoretaComponent::Proc {
                    signature,
                    span,
                    destructor: true,
                    ..
                } => Some((signature.clone(), *span)),
                _ => None,
            })
    }

    pub fn get_set(&self, id: SetId) -> &SetMode {
        &self.sets[id.0]
    }

    pub fn set_element(&self, id: SetId, name: &str) -> Option<(usize, i128)> {
        self.get_set(id)
            .elements
            .iter()
            .enumerate()
            .find(|(_, (element, _))| element == name)
            .map(|(index, (_, value))| (index, *value))
    }

    pub fn field(&self, id: ModeId, name: &str) -> Option<(usize, &FieldInfo)> {
        self.get(id)
            .fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == name)
    }
}
