use crate::types::{ModeId, Type};
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

#[derive(Debug, Clone, Default)]
pub struct ModeTable {
    pub modes: Vec<StructMode>,
}

impl ModeTable {
    pub fn get(&self, id: ModeId) -> &StructMode {
        &self.modes[id.0]
    }

    pub fn get_mut(&mut self, id: ModeId) -> &mut StructMode {
        &mut self.modes[id.0]
    }

    pub fn field(&self, id: ModeId, name: &str) -> Option<(usize, &FieldInfo)> {
        self.get(id)
            .fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == name)
    }
}
