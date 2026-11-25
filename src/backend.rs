//! Minimal C backend for the initial native-code milestone.
//!
//! This backend intentionally covers a small, deterministic subset of the typed
//! program. Unsupported constructs produce a diagnostic instead of falling back
//! silently. The interpreter remains the reference implementation.

use crate::ast::{
    Action, AssignOp, BinaryOp, CaseLabel, Declaration, Expr, Handler, ModeRef, Module, Passing,
    ProcDef, Program, UnaryOp,
};
use crate::builtins::{ClauseWidth, ConversionCode, EditingCode, FormatOp, IoControl, TextIo};
use crate::manifest::WidthProfile;
use crate::modes::ModeTable;
use crate::sema::{ApplyResolution, CheckResult, ImportedKind, ImportedSymbol};
use crate::types::{self, IntKind, ModeId, Type};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

pub const RUNTIME_HEADER: &str = include_str!("../runtime/z200rt.h");
pub const RUNTIME_SOURCE: &str = include_str!("../runtime/z200rt.c");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Int(IntKind),
    Float,
    Bool,
    Char,
    String,
    Array {
        element: ScalarKind,
        dims: [Dim; 4],
        rank: usize,
        total: usize,
    },
    Ref(ScalarKind),
    RefArray {
        element: ScalarKind,
        dims: [Dim; 4],
        rank: usize,
        total: usize,
    },
    DynamicArray {
        element: ScalarKind,
        low: i128,
    },
    Struct(ModeId),
    PowerSet(types::PowerSetMember),
    Text {
        length: usize,
        indexed: bool,
        low: i128,
        high: i128,
    },
    Association,
    Access {
        indexed: bool,
        low: i128,
        high: i128,
    },
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dim {
    low: i128,
    high: i128,
}

impl Dim {
    fn length(self) -> i128 {
        (self.high - self.low + 1).max(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarKind {
    Int(IntKind),
    Float,
    Bool,
    Char,
    String,
    Struct(ModeId),
    PowerSet(types::PowerSetMember),
    Association,
    Access {
        indexed: bool,
        low: i128,
        high: i128,
    },
    Text {
        length: usize,
        indexed: bool,
        low: i128,
        high: i128,
    },
}

impl ScalarKind {
    fn c_type(self) -> String {
        match self {
            ScalarKind::Int(_) => "int64_t".to_owned(),
            ScalarKind::Float => "double".to_owned(),
            ScalarKind::Bool => "int".to_owned(),
            ScalarKind::Char => "int".to_owned(),
            ScalarKind::String => "const char *".to_owned(),
            ScalarKind::Struct(id) => format!("z_struct_{}", id.0),
            ScalarKind::PowerSet(_) => "ZPowerSet".to_owned(),
            ScalarKind::Association => "ZAssociation".to_owned(),
            ScalarKind::Access { .. } => "ZAccess".to_owned(),
            ScalarKind::Text { .. } => "ZText".to_owned(),
        }
    }

    fn kind(self) -> Kind {
        match self {
            ScalarKind::Int(kind) => Kind::Int(kind),
            ScalarKind::Float => Kind::Float,
            ScalarKind::Bool => Kind::Bool,
            ScalarKind::Char => Kind::Char,
            ScalarKind::String => Kind::String,
            ScalarKind::Struct(id) => Kind::Struct(id),
            ScalarKind::PowerSet(member) => Kind::PowerSet(member),
            ScalarKind::Association => Kind::Association,
            ScalarKind::Access { indexed, low, high } => Kind::Access { indexed, low, high },
            ScalarKind::Text {
                length,
                indexed,
                low,
                high,
            } => Kind::Text {
                length,
                indexed,
                low,
                high,
            },
        }
    }
}

enum TextTarget {
    Text {
        code: String,
        length: usize,
        indexed: bool,
        low: i128,
        high: i128,
    },
    String {
        code: String,
        limit: String,
    },
}

#[derive(Debug, Clone)]
struct VarInfo {
    c_name: String,
    kind: Kind,
}

impl Kind {
    fn c_type(self) -> String {
        match self {
            Kind::Int(_) => "int64_t".to_owned(),
            Kind::Float => "double".to_owned(),
            Kind::Bool => "int".to_owned(),
            Kind::Char => "int".to_owned(),
            Kind::String => "const char *".to_owned(),
            Kind::Array { .. } => {
                unreachable!("array kinds are emitted through c_declaration")
            }
            Kind::Ref(element) => match element {
                ScalarKind::Int(_) => "int64_t *".to_owned(),
                ScalarKind::Float => "double *".to_owned(),
                ScalarKind::Bool => "int *".to_owned(),
                ScalarKind::Char => "int *".to_owned(),
                ScalarKind::String => "const char **".to_owned(),
                ScalarKind::Struct(id) => format!("z_struct_{} *", id.0),
                ScalarKind::PowerSet(_) => "ZPowerSet *".to_owned(),
                ScalarKind::Association => "ZAssociation *".to_owned(),
                ScalarKind::Access { .. } => "ZAccess *".to_owned(),
                ScalarKind::Text { .. } => "ZText *".to_owned(),
            },
            Kind::RefArray { element, .. } => format!("{} *", element.c_type()),
            Kind::Struct(id) => format!("z_struct_{}", id.0),
            Kind::PowerSet(_) => "ZPowerSet".to_owned(),
            Kind::Text { .. } => "ZText".to_owned(),
            Kind::Association => "ZAssociation".to_owned(),
            Kind::Access { .. } => "ZAccess".to_owned(),
            Kind::DynamicArray { .. } => "ZSlice".to_owned(),
            Kind::Null => {
                unreachable!("NULL is never used as a declared variable type")
            }
        }
    }

    fn default_value(self) -> &'static str {
        match self {
            Kind::Int(_) | Kind::Bool | Kind::Char => "0",
            Kind::Float => "0.0",
            Kind::String => "\"\"",
            Kind::Array { .. } => "{0}",
            Kind::Ref(_) | Kind::RefArray { .. } | Kind::Null => "NULL",
            Kind::Struct(_) | Kind::PowerSet(_) | Kind::Text { .. } => "{0}",
            Kind::Association | Kind::Access { .. } => "{0}",
            Kind::DynamicArray { .. } => "{0}",
        }
    }

    fn c_declaration(self, name: &str) -> String {
        match self {
            Kind::Array { element, total, .. } => {
                format!("{} {}[{}]", element.c_type(), name, total)
            }
            Kind::Ref(element) | Kind::RefArray { element, .. } => {
                format!("{} *{}", element.c_type(), name)
            }
            Kind::Struct(id) => format!("z_struct_{} {name}", id.0),
            Kind::Null => unreachable!("NULL is never used as a declared variable type"),
            other => format!("{} {}", other.c_type(), name),
        }
    }
}

struct Generator<'a> {
    check: &'a CheckResult,
    profile: WidthProfile,
    modes: &'a ModeTable,
    module_modes: &'a [HashMap<String, Type>],
    module_index: usize,
    vars: HashMap<String, VarInfo>,
    indent: usize,
    result: Option<(String, Kind)>,
    result_wrapper: Option<String>,
    temp_counter: usize,
    loop_depth: usize,
    labels: Vec<(String, String, String)>,
    loop_marks: Vec<String>,
    prelude: Vec<String>,
    temporaries: Vec<(String, Kind)>,
    scopes: Vec<Vec<(String, Kind)>>,
    scope_marks: Vec<String>,
    function_mark: Option<String>,
    cleanup_defs: Rc<RefCell<String>>,
    cleanup_counter: Rc<Cell<usize>>,
}

pub fn generate(
    program: &Program,
    check: &CheckResult,
    package: &str,
    profile: WidthProfile,
    modes: &ModeTable,
    module_modes: &[HashMap<String, Type>],
) -> Result<String, String> {
    let modules: Vec<(usize, &Module)> = program
        .modules
        .iter()
        .enumerate()
        .filter(|(_, module)| !module.kind.is_spec())
        .collect();

    let mut module_modes = module_modes.to_vec();
    for ((module, local), symbol) in &check.imports {
        if let ImportedKind::Mode(ty) = &symbol.kind {
            if let Some(modes) = module_modes.get_mut(*module) {
                modes.insert(local.clone(), **ty);
            }
        }
    }
    let module_modes = module_modes.as_slice();

    let cleanup_defs = Rc::new(RefCell::new(String::new()));
    let cleanup_counter = Rc::new(Cell::new(0));
    let mut out = String::new();
    out.push_str("/* generated by z200c */\n");
    out.push_str("/* generated by z200c */\n");
    out.push_str("#include \"z200rt.h\"\n\n");

    emit_struct_typedefs(&mut out, modes)?;
    emit_array_result_typedefs(&mut out, &modules, module_modes)?;
    out.push_str("/*__Z200C_CLEANUP_DEFS__*/\n");
    out.push('\n');

    // Module globals.
    for &(index, module) in &modules {
        for declaration in &module.declarations {
            let Declaration::Var(vars) = declaration else {
                continue;
            };
            for var in vars {
                let kind = mode_kind(&var.mode, module_modes, index)?;
                out.push_str(&format!(
                    "static {};\n",
                    kind.c_declaration(&global_name(index, &var.name))
                ));
            }
        }
    }
    out.push('\n');

    // Procedure prototypes.
    for &(index, module) in &modules {
        for declaration in &module.declarations {
            if let Declaration::Proc(proc_def) = declaration {
                emit_proc_signature(&mut out, index, proc_def, module_modes)?;
                out.push_str(";\n");
            }
        }
    }
    out.push('\n');

    // Procedure definitions.
    for &(index, module) in &modules {
        for declaration in &module.declarations {
            if let Declaration::Proc(proc_def) = declaration {
                emit_proc(
                    &mut out,
                    check,
                    profile,
                    modes,
                    module_modes,
                    cleanup_defs.clone(),
                    cleanup_counter.clone(),
                    index,
                    module,
                    proc_def,
                )?;
            }
        }
    }

    // Module run functions.
    for &(index, module) in &modules {
        out.push_str(&format!("static void run_module_{index}(void) {{\n"));
        let mut generator = Generator {
            check,
            profile,
            module_index: index,
            modes,
            module_modes,
            vars: module_vars(module, index, module_modes, &check.imports),
            indent: 1,
            result: None,
            result_wrapper: None,
            temp_counter: 0,
            loop_depth: 0,
            labels: Vec::new(),
            loop_marks: Vec::new(),
            prelude: Vec::new(),
            temporaries: Vec::new(),
            scopes: vec![Vec::new()],
            scope_marks: Vec::new(),
            function_mark: None,
            cleanup_defs: cleanup_defs.clone(),
            cleanup_counter: cleanup_counter.clone(),
        };
        generator.line(
            &mut out,
            "ZHandler *__z_entry_handlers = z_handler_current();",
        );
        let function_mark = generator.push_scope_marker(&mut out)?;
        generator.function_mark = Some(function_mark);
        generator.emit_initializers(&mut out, &module.declarations)?;
        if let Some(handler) = &module.handler {
            generator.emit_handled(&mut out, &module.actions, handler)?;
        } else {
            generator.emit_actions(&mut out, &module.actions)?;
        }
        generator.drop_temporaries(&mut out)?;
        generator.drop_live_scopes(&mut out)?;
        out.push_str("}\n\n");
    }

    out.push_str("int main(void) {\n");
    for &(index, _) in &modules {
        out.push_str(&format!("    run_module_{index}();\n"));
    }
    {
        let mut cleanup = Generator {
            check,
            profile,
            module_index: 0,
            modes,
            module_modes,
            vars: HashMap::new(),
            indent: 1,
            result: None,
            result_wrapper: None,
            temp_counter: 0,
            loop_depth: 0,
            labels: Vec::new(),
            loop_marks: Vec::new(),
            prelude: Vec::new(),
            temporaries: Vec::new(),
            scopes: vec![Vec::new()],
            scope_marks: Vec::new(),
            function_mark: None,
            cleanup_defs: cleanup_defs.clone(),
            cleanup_counter: cleanup_counter.clone(),
        };
        for &(index, module) in &modules {
            for declaration in &module.declarations {
                let Declaration::Var(vars) = declaration else {
                    continue;
                };
                for var in vars {
                    let kind = mode_kind(&var.mode, module_modes, index)?;
                    if cleanup.kind_owns(kind) {
                        cleanup.drop_kind(&mut out, &global_name(index, &var.name), kind)?;
                    }
                }
            }
        }
    }
    out.push_str(&format!("    (void)\"{}\";\n", c_escape(package)));
    out.push_str("    if (getenv(\"Z200C_RC_STATS\") != NULL) {\n");
    out.push_str("        printf(\"live-payloads: %zu\\n\", z_rc_live_count());\n");
    out.push_str("    }\n");
    out.push_str("    return 0;\n}\n");
    let definitions = cleanup_defs.borrow().clone();
    out = out.replace("/*__Z200C_CLEANUP_DEFS__*/", &definitions);
    Ok(out)
}

fn emit_struct_typedefs(out: &mut String, modes: &ModeTable) -> Result<(), String> {
    fn emit_one(
        id: ModeId,
        modes: &ModeTable,
        done: &mut std::collections::HashSet<usize>,
        visiting: &mut std::collections::HashSet<usize>,
        out: &mut String,
    ) -> Result<(), String> {
        if done.contains(&id.0) {
            return Ok(());
        }
        if !visiting.insert(id.0) {
            return Err(format!(
                "native backend: recursive structure mode `{}` is not supported",
                modes.get(id).name
            ));
        }
        let mode = modes.get(id);
        for field in &mode.fields {
            if let Type::Struct(dependency) = field.ty {
                emit_one(dependency, modes, done, visiting, out)?;
            }
        }
        out.push_str(&format!("typedef struct z_struct_{} {{\n", id.0));
        for field in &mode.fields {
            let kind = kind_from_type(field.ty)?;
            out.push_str(&format!(
                "    {};\n",
                kind.c_declaration(&c_ident(&field.name))
            ));
        }
        out.push_str(&format!("}} z_struct_{};\n\n", id.0));
        visiting.remove(&id.0);
        done.insert(id.0);
        Ok(())
    }

    let mut done = std::collections::HashSet::new();
    let mut visiting = std::collections::HashSet::new();
    for id in 0..modes.modes.len() {
        emit_one(ModeId(id), modes, &mut done, &mut visiting, out)?;
    }
    Ok(())
}

fn module_vars(
    module: &Module,
    index: usize,
    module_modes: &[HashMap<String, Type>],
    imports: &HashMap<(usize, String), ImportedSymbol>,
) -> HashMap<String, VarInfo> {
    let mut vars = HashMap::new();
    for declaration in &module.declarations {
        let Declaration::Var(list) = declaration else {
            continue;
        };
        for var in list {
            if let Ok(kind) = mode_kind(&var.mode, module_modes, index) {
                vars.insert(
                    var.name.clone(),
                    VarInfo {
                        c_name: global_name(index, &var.name),
                        kind,
                    },
                );
            }
        }
    }
    for ((owner, local), symbol) in imports {
        if *owner != index {
            continue;
        }
        if let ImportedKind::Var(ty) = &symbol.kind {
            if let Ok(kind) = kind_from_type(**ty) {
                vars.insert(
                    local.clone(),
                    VarInfo {
                        c_name: global_name(symbol.module, &symbol.name),
                        kind,
                    },
                );
            }
        }
    }
    vars
}

fn global_name(module: usize, name: &str) -> String {
    format!("m{module}_{}", c_ident(name))
}

fn proc_name(module: usize, name: &str) -> String {
    format!("m{module}_p_{}", c_ident(name))
}

fn array_result_wrapper(module: usize, name: &str) -> String {
    format!("z_ret_{}", proc_name(module, name))
}

fn emit_array_result_typedefs(
    out: &mut String,
    modules: &[(usize, &Module)],
    module_modes: &[HashMap<String, Type>],
) -> Result<(), String> {
    for &(index, module) in modules {
        for declaration in &module.declarations {
            let Declaration::Proc(proc_def) = declaration else {
                continue;
            };
            let Some(mode) = &proc_def.result else {
                continue;
            };
            let kind = mode_kind(mode, module_modes, index)?;
            if let Kind::Array { element, total, .. } = kind {
                out.push_str(&format!(
                    "typedef struct {{ {} items[{}]; }} {};\n",
                    element.c_type(),
                    total,
                    array_result_wrapper(index, &proc_def.name)
                ));
            }
        }
    }
    Ok(())
}

fn emit_proc_signature(
    out: &mut String,
    module: usize,
    proc_def: &ProcDef,
    module_modes: &[HashMap<String, Type>],
) -> Result<(), String> {
    let result_kind = proc_def
        .result
        .as_ref()
        .map(|mode| mode_kind(mode, module_modes, module))
        .transpose()?;
    let result = match result_kind {
        Some(Kind::Array { .. }) => array_result_wrapper(module, &proc_def.name),
        Some(kind) => kind.c_type(),
        None => "void".to_owned(),
    };
    out.push_str(&format!(
        "static {result} {}",
        proc_name(module, &proc_def.name)
    ));
    out.push('(');
    for (index, param) in proc_def.params.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        let kind = mode_kind(&param.mode, module_modes, module)?;
        let name = c_ident(&format!("p_{}", param.name));
        if let Kind::Array { element, .. } = kind {
            let element = element.c_type();
            match param.passing {
                Passing::In => out.push_str(&format!("const {element} *{name}")),
                Passing::Out | Passing::InOut | Passing::Loc => {
                    out.push_str(&format!("{element} *{name}"))
                }
            }
            continue;
        }
        match param.passing {
            Passing::In => out.push_str(&format!("{} {name}", kind.c_type())),
            Passing::Out | Passing::InOut | Passing::Loc => {
                out.push_str(&format!("{} *{name}", kind.c_type()))
            }
        }
    }
    if proc_def.params.is_empty() {
        out.push_str("void");
    }
    out.push(')');
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_proc(
    out: &mut String,
    check: &CheckResult,
    profile: WidthProfile,
    modes: &ModeTable,
    module_modes: &[HashMap<String, Type>],
    cleanup_defs: Rc<RefCell<String>>,
    cleanup_counter: Rc<Cell<usize>>,
    module: usize,
    owner: &Module,
    proc_def: &ProcDef,
) -> Result<(), String> {
    emit_proc_signature(out, module, proc_def, module_modes)?;
    out.push_str(" {\n");
    let result_kind = proc_def
        .result
        .as_ref()
        .map(|mode| mode_kind(mode, module_modes, module))
        .transpose()?;
    let result_wrapper = match result_kind {
        Some(Kind::Array { .. }) => Some(array_result_wrapper(module, &proc_def.name)),
        _ => None,
    };
    let mut generator = Generator {
        check,
        profile,
        module_index: module,
        modes,
        module_modes,
        vars: module_vars(owner, module, module_modes, &check.imports),
        indent: 1,
        result: result_kind.map(|kind| (format!("__result_{}", c_ident(&proc_def.name)), kind)),
        result_wrapper,
        temp_counter: 0,
        loop_depth: 0,
        labels: Vec::new(),
        loop_marks: Vec::new(),
        prelude: Vec::new(),
        temporaries: Vec::new(),
        scopes: vec![Vec::new()],
        scope_marks: Vec::new(),
        function_mark: None,
        cleanup_defs,
        cleanup_counter,
    };
    generator.line(out, "ZHandler *__z_entry_handlers = z_handler_current();");
    let function_mark = generator.push_scope_marker(out)?;
    generator.function_mark = Some(function_mark);
    if let Some((name, kind)) = &generator.result {
        if let Kind::Array { element, total, .. } = kind {
            generator.line(
                out,
                &format!("{} {name}[{total}] = {{0}};", element.c_type()),
            );
        } else {
            generator.line(
                out,
                &format!("{} {} = {};", kind.c_type(), name, kind.default_value()),
            );
        }
    }
    for param in &proc_def.params {
        let kind = mode_kind(&param.mode, module_modes, module)?;
        let p_name = c_ident(&format!("p_{}", param.name));
        if let Kind::Array { .. } = kind {
            let c_name = match param.passing {
                Passing::In => {
                    let local = c_ident(&format!("l_{}", param.name));
                    generator.line(out, &format!("{} = {{0}};", kind.c_declaration(&local)));
                    generator.copy_kind(out, &local, &p_name, kind)?;
                    generator.register_owned_local(out, &local, kind)?;
                    local
                }
                Passing::Out | Passing::InOut | Passing::Loc => p_name,
            };
            generator
                .vars
                .insert(param.name.clone(), VarInfo { c_name, kind });
            continue;
        }
        if param.passing == Passing::In && generator.kind_owns(kind) {
            let local = c_ident(&format!("l_{}", param.name));
            generator.line(
                out,
                &format!("{} = {};", kind.c_declaration(&local), kind.default_value()),
            );
            generator.copy_kind(out, &local, &p_name, kind)?;
            generator.register_owned_local(out, &local, kind)?;
            generator.vars.insert(
                param.name.clone(),
                VarInfo {
                    c_name: local,
                    kind,
                },
            );
            continue;
        }
        let c_name = match param.passing {
            Passing::In => p_name,
            Passing::Out | Passing::InOut | Passing::Loc => format!("(*{p_name})"),
        };
        generator
            .vars
            .insert(param.name.clone(), VarInfo { c_name, kind });
    }
    generator.emit_local_declarations(out, &proc_def.declarations)?;
    if let Some(handler) = &proc_def.handler {
        generator.emit_handled(out, &proc_def.actions, handler)?;
    } else {
        generator.emit_actions(out, &proc_def.actions)?;
    }
    generator.drop_temporaries(out)?;
    generator.drop_live_scopes(out)?;
    if let Some(wrapper) = generator.result_wrapper.clone() {
        let Some((name, _)) = generator.result.clone() else {
            return Err("native backend: internal array result without storage".to_owned());
        };
        generator.emit_array_result_return(out, &name, &wrapper)?;
    } else {
        generator.line(out, "z_handler_set(__z_entry_handlers);");
        if let Some((name, _)) = generator.result.clone() {
            generator.line(out, &format!("return {name};"));
        }
    }
    out.push_str("}\n\n");
    Ok(())
}

impl Generator<'_> {
    fn line(&mut self, out: &mut String, text: &str) {
        self.flush_prelude(out);
        for _ in 0..self.indent {
            out.push_str("    ");
        }
        out.push_str(text);
        out.push('\n');
    }

    fn flush_prelude(&mut self, out: &mut String) {
        if self.prelude.is_empty() {
            return;
        }
        let lines = std::mem::take(&mut self.prelude);
        for text in lines {
            for _ in 0..self.indent {
                out.push_str("    ");
            }
            out.push_str(&text);
            out.push('\n');
        }
    }

    fn cleanup_cast_type(kind: Kind) -> Result<String, String> {
        match kind {
            Kind::String => Ok("const char *".to_owned()),
            Kind::PowerSet(_) => Ok("ZPowerSet".to_owned()),
            Kind::Text { .. } => Ok("ZText".to_owned()),
            Kind::Association => Ok("ZAssociation".to_owned()),
            Kind::Access { .. } => Ok("ZAccess".to_owned()),
            Kind::Struct(id) => Ok(format!("z_struct_{}", id.0)),
            Kind::Array { element, total, .. } => {
                Ok(format!("({}) (*)[{}]", element.c_type(), total))
            }
            _ => Err("native backend: cleanup for this kind is not supported".to_owned()),
        }
    }

    fn drop_kind_text(
        &self,
        out: &mut String,
        indent: usize,
        value: &str,
        kind: Kind,
        counter: &mut usize,
    ) -> Result<(), String> {
        let prefix = "    ".repeat(indent);
        match kind {
            Kind::String => out.push_str(&format!("{prefix}z_string_drop({value});\n")),
            Kind::PowerSet(_) => out.push_str(&format!("{prefix}z_ps_drop(&{value});\n")),
            Kind::Text { .. } => out.push_str(&format!("{prefix}z_text_drop(&{value});\n")),
            Kind::Association => out.push_str(&format!("{prefix}z_association_drop(&{value});\n")),
            Kind::Access { .. } => out.push_str(&format!("{prefix}z_access_drop(&{value});\n")),
            Kind::Struct(id) => {
                let fields = self
                    .modes
                    .get(id)
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty))
                    .collect::<Vec<_>>();
                for (name, ty) in fields {
                    let field_kind = kind_from_type(ty)?;
                    if self.kind_owns(field_kind) {
                        self.drop_kind_text(
                            out,
                            indent,
                            &format!("{value}.{}", c_ident(&name)),
                            field_kind,
                            counter,
                        )?;
                    }
                }
            }
            Kind::Array { element, total, .. } => {
                if !self.kind_owns(element.kind()) {
                    return Ok(());
                }
                let index = format!("__drop_i_{}", *counter);
                *counter += 1;
                out.push_str(&format!("{prefix}{{\n"));
                out.push_str(&format!(
                    "{prefix}    for (size_t {index} = 0; {index} < {total}; {index}++) {{\n"
                ));
                self.drop_kind_text(
                    out,
                    indent + 2,
                    &format!("{value}[{index}]"),
                    element.kind(),
                    counter,
                )?;
                out.push_str(&format!("{prefix}    }}\n"));
                out.push_str(&format!("{prefix}}}\n"));
            }
            _ => {}
        }
        Ok(())
    }

    fn cleanup_push_line(&mut self, value: &str, kind: Kind) -> Result<String, String> {
        if !self.kind_owns(kind) {
            return Ok(String::new());
        }
        let id = self.cleanup_counter.get();
        self.cleanup_counter.set(id + 1);
        let name = format!("__drop_value_{id}");
        let mut body = String::new();
        let mut counter = 0usize;
        if let Kind::Array { element, total, .. } = kind {
            let element_type = element.c_type();
            body.push_str(&format!(
                "    {element_type} *items = ({element_type} *)value;\n"
            ));
            let index = format!("__drop_i_{}", counter);
            counter += 1;
            body.push_str(&format!(
                "    for (size_t {index} = 0; {index} < {total}; {index}++) {{\n"
            ));
            self.drop_kind_text(
                &mut body,
                2,
                &format!("items[{index}]"),
                element.kind(),
                &mut counter,
            )?;
            body.push_str("    }\n");
        } else {
            let ctype = Self::cleanup_cast_type(kind)?;
            let value_expr = format!("(*(({ctype} *)value))");
            self.drop_kind_text(&mut body, 1, &value_expr, kind, &mut counter)?;
        }
        self.cleanup_defs
            .borrow_mut()
            .push_str(&format!("static void {name}(void *value) {{\n{body}}}\n\n"));
        Ok(format!("z_cleanup_push({name}, (void *)&({value}));"))
    }

    fn register_owned_local(
        &mut self,
        out: &mut String,
        name: &str,
        kind: Kind,
    ) -> Result<(), String> {
        let push = self.cleanup_push_line(name, kind)?;
        if !push.is_empty() {
            self.line(out, &push);
        }
        Ok(())
    }

    fn kind_owns(&self, kind: Kind) -> bool {
        match kind {
            Kind::String
            | Kind::PowerSet(_)
            | Kind::Text { .. }
            | Kind::Association
            | Kind::Access { .. } => true,
            Kind::Array { element, .. } => self.kind_owns(element.kind()),
            Kind::Struct(id) => self
                .modes
                .get(id)
                .fields
                .iter()
                .any(|field| kind_from_type(field.ty).is_ok_and(|kind| self.kind_owns(kind))),
            _ => false,
        }
    }

    fn drop_kind(&mut self, out: &mut String, value: &str, kind: Kind) -> Result<(), String> {
        match kind {
            Kind::String => self.line(out, &format!("z_string_drop({value});")),
            Kind::PowerSet(_) => self.line(out, &format!("z_ps_drop(&{value});")),
            Kind::Text { .. } => self.line(out, &format!("z_text_drop(&{value});")),
            Kind::Association => self.line(out, &format!("z_association_drop(&{value});")),
            Kind::Access { .. } => self.line(out, &format!("z_access_drop(&{value});")),
            Kind::Struct(id) => {
                let fields = self
                    .modes
                    .get(id)
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty))
                    .collect::<Vec<_>>();
                for (name, ty) in fields {
                    let field_kind = kind_from_type(ty)?;
                    if self.kind_owns(field_kind) {
                        self.drop_kind(out, &format!("{value}.{}", c_ident(&name)), field_kind)?;
                    }
                }
            }
            Kind::Array { element, total, .. } => {
                if !self.kind_owns(element.kind()) {
                    return Ok(());
                }
                let unique = self.temp_counter;
                self.temp_counter += 1;
                let index = format!("__drop_i_{unique}");
                self.line(out, "{");
                self.indent += 1;
                self.line(
                    out,
                    &format!("for (size_t {index} = 0; {index} < {total}; {index}++) {{"),
                );
                self.indent += 1;
                self.drop_kind(out, &format!("{value}[{index}]"), element.kind())?;
                self.indent -= 1;
                self.line(out, "}");
                self.indent -= 1;
                self.line(out, "}");
            }
            _ => {}
        }
        Ok(())
    }

    fn copy_kind(
        &mut self,
        out: &mut String,
        target: &str,
        source: &str,
        kind: Kind,
    ) -> Result<(), String> {
        if target == source {
            return Ok(());
        }
        match kind {
            Kind::String => self.line(out, &format!("z_string_assign(&{target}, {source});")),
            Kind::PowerSet(_) => self.line(out, &format!("z_ps_assign(&{target}, {source});")),
            Kind::Text { .. } => {
                self.line(out, &format!("z_text_assign_value(&{target}, {source});"))
            }
            Kind::Association => {
                self.line(out, &format!("z_association_assign(&{target}, {source});"))
            }
            Kind::Access { .. } => {
                self.line(out, &format!("z_access_assign(&{target}, {source});"))
            }
            Kind::Struct(id) => {
                let fields = self
                    .modes
                    .get(id)
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty))
                    .collect::<Vec<_>>();
                for (name, ty) in fields {
                    let field_kind = kind_from_type(ty)?;
                    self.copy_kind(
                        out,
                        &format!("{target}.{}", c_ident(&name)),
                        &format!("{source}.{}", c_ident(&name)),
                        field_kind,
                    )?;
                }
            }
            Kind::Array { element, total, .. } => {
                let unique = self.temp_counter;
                self.temp_counter += 1;
                let index = format!("__copy_i_{unique}");
                self.line(out, "{");
                self.indent += 1;
                self.line(
                    out,
                    &format!("for (size_t {index} = 0; {index} < {total}; {index}++) {{"),
                );
                self.indent += 1;
                self.copy_kind(
                    out,
                    &format!("{target}[{index}]"),
                    &format!("{source}[{index}]"),
                    element.kind(),
                )?;
                self.indent -= 1;
                self.line(out, "}");
                self.indent -= 1;
                self.line(out, "}");
            }
            _ => self.line(out, &format!("{target} = {source};")),
        }
        Ok(())
    }

    fn temporary(&mut self, expression: &str, kind: Kind) -> Result<String, String> {
        let unique = self.temp_counter;
        self.temp_counter += 1;
        let name = format!("__own_{unique}");
        self.prelude
            .push(format!("{} = {expression};", kind.c_declaration(&name)));
        let push = self.cleanup_push_line(&name, kind)?;
        if !push.is_empty() {
            self.prelude.push(push);
        }
        self.temporaries.push((name.clone(), kind));
        Ok(name)
    }

    fn register_temporary(&mut self, name: String, kind: Kind) {
        self.temporaries.push((name, kind));
    }

    fn drop_temporaries(&mut self, out: &mut String) -> Result<(), String> {
        self.flush_prelude(out);
        self.temporaries.clear();
        Ok(())
    }

    fn push_scope_marker(&mut self, out: &mut String) -> Result<String, String> {
        let unique = self.temp_counter;
        self.temp_counter += 1;
        let mark = format!("__scope_mark_{unique}");
        self.line(out, &format!("ZCleanup *{mark} = z_cleanup_mark();"));
        self.scope_marks.push(mark.clone());
        Ok(mark)
    }

    fn drop_scope(&mut self, out: &mut String) -> Result<(), String> {
        if let Some(mark) = self.scope_marks.pop() {
            self.line(out, &format!("z_cleanup_run({mark});"));
        }
        self.scopes.pop();
        Ok(())
    }

    fn drop_live_scopes(&mut self, out: &mut String) -> Result<(), String> {
        while !self.scope_marks.is_empty() {
            self.drop_scope(out)?;
        }
        Ok(())
    }

    fn emit_initializers(
        &mut self,
        out: &mut String,
        declarations: &[Declaration],
    ) -> Result<(), String> {
        for declaration in declarations {
            let Declaration::Var(vars) = declaration else {
                continue;
            };
            for var in vars {
                let Some(init) = &var.init else {
                    continue;
                };
                let info =
                    self.vars.get(&var.name).cloned().ok_or_else(|| {
                        format!("native backend: unknown variable `{}`", var.name)
                    })?;
                if matches!(info.kind, Kind::Array { .. }) {
                    self.emit_array_literal(out, &info, &init.value, var.span)?;
                    self.drop_temporaries(out)?;
                    continue;
                }
                let (code, value_kind) = self.emit_expr(&init.value)?;
                if let Kind::Int(kind) = info.kind {
                    let code = self.checked_int(&code, kind)?;
                    self.line(out, &format!("{} = {code};", info.c_name));
                } else if let Kind::Text { length, .. } = info.kind {
                    self.emit_text_store(out, &info.c_name, length, &code, value_kind)?;
                } else if self.kind_owns(info.kind) {
                    self.copy_kind(out, &info.c_name, &code, info.kind)?;
                } else {
                    self.line(out, &format!("{} = {};", info.c_name, code));
                }
                self.drop_temporaries(out)?;
            }
        }
        Ok(())
    }

    fn emit_array_literal(
        &mut self,
        out: &mut String,
        info: &VarInfo,
        value: &Expr,
        span: crate::lexer::Span,
    ) -> Result<(), String> {
        let Kind::Array {
            element,
            dims,
            rank,
            total,
        } = info.kind
        else {
            return Err("native backend: internal: not an array initializer".to_owned());
        };
        let Expr::ArrayTuple { elements, .. } = value else {
            return Err("native backend: array initializers must be array literals".to_owned());
        };
        let mut values = Vec::new();
        self.collect_array_literal(elements, &dims, rank, 0, &mut values)?;
        if values.len() != total {
            return Err(format!(
                "native backend: array literal has {} element(s), expected {total}",
                values.len()
            ));
        }
        let values = if let ScalarKind::Int(kind) = element {
            values
                .into_iter()
                .map(|code| self.checked_int(&code, kind))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            values
        };
        for (index, code) in values.into_iter().enumerate() {
            self.copy_kind(
                out,
                &format!("{}[{}]", info.c_name, index),
                &code,
                element.kind(),
            )?;
        }
        let _ = span;
        Ok(())
    }

    fn collect_array_literal(
        &mut self,
        elements: &[crate::ast::ArrayTupleElement],
        dims: &[Dim; 4],
        rank: usize,
        level: usize,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        let expected = dims[level].length().max(0) as usize;
        if elements.len() != expected {
            return Err(format!(
                "native backend: array literal expects {expected} element(s) at dimension {}, found {}",
                level + 1,
                elements.len()
            ));
        }
        for item in elements {
            if item.labels.is_some() || item.range_end.is_some() {
                return Err(
                    "native backend: labelled or ranged array literals are not supported yet"
                        .to_owned(),
                );
            }
            if level + 1 < rank {
                let Expr::ArrayTuple { elements, .. } = &item.value else {
                    return Err("native backend: nested array literal expected".to_owned());
                };
                self.collect_array_literal(elements, dims, rank, level + 1, out)?;
            } else {
                out.push(self.emit_expr(&item.value)?.0);
            }
        }
        Ok(())
    }

    fn array_index_expr(
        &mut self,
        info: &VarInfo,
        args: &[Expr],
        span: crate::lexer::Span,
    ) -> Result<(String, Kind), String> {
        let Kind::Array {
            element,
            dims,
            rank,
            ..
        } = info.kind
        else {
            return Err(format!("native backend: `{}` is not an array", info.c_name));
        };
        if args.len() != rank {
            return Err(format!(
                "native backend: array index requires {rank} index expression(s)"
            ));
        }
        let mut terms = Vec::with_capacity(rank);
        for (index, argument) in args.iter().enumerate() {
            let (code, _) = self.emit_expr(argument)?;
            let checked = format!(
                "z_check_index({code}, {}, {})",
                dims[index].low, dims[index].high
            );
            let mut stride = 1i128;
            for dim in dims.iter().take(rank).skip(index + 1) {
                stride *= dim.length();
            }
            terms.push(format!(
                "((({checked}) - ({})) * {stride})",
                dims[index].low
            ));
        }
        let _ = span;
        Ok((
            format!("{}[{}]", info.c_name, terms.join(" + ")),
            element.kind(),
        ))
    }

    fn deref_index_expr(
        &mut self,
        base: &Expr,
        indices: &[Expr],
        span: crate::lexer::Span,
    ) -> Result<(String, Kind), String> {
        let (array_code, kind) = self.emit_expr(base)?;
        let Kind::Array {
            element,
            dims,
            rank,
            ..
        } = kind
        else {
            return Err(
                "native backend: indexed dereference requires an array reference".to_owned(),
            );
        };
        if indices.len() != rank {
            return Err(format!(
                "native backend: array index requires {rank} index expression(s)"
            ));
        }
        let mut terms = Vec::with_capacity(rank);
        for (index, argument) in indices.iter().enumerate() {
            let (code, _) = self.emit_expr(argument)?;
            let checked = format!(
                "z_check_index({code}, {}, {})",
                dims[index].low, dims[index].high
            );
            let mut stride = 1i128;
            for dim in dims.iter().take(rank).skip(index + 1) {
                stride *= dim.length();
            }
            terms.push(format!(
                "((({checked}) - ({})) * {stride})",
                dims[index].low
            ));
        }
        let _ = span;
        Ok((
            format!("({array_code})[{}]", terms.join(" + ")),
            element.kind(),
        ))
    }

    fn emit_local_declarations(
        &mut self,
        out: &mut String,
        declarations: &[Declaration],
    ) -> Result<(), String> {
        for declaration in declarations {
            match declaration {
                Declaration::Var(vars) => {
                    for var in vars {
                        let kind = mode_kind(&var.mode, self.module_modes, self.module_index)?;
                        let c_name = c_ident(&format!("l_{}", var.name));
                        if matches!(kind, Kind::Array { .. }) {
                            self.line(out, &format!("{} = {{0}};", kind.c_declaration(&c_name)));
                            let info = VarInfo {
                                c_name: c_name.clone(),
                                kind,
                            };
                            if let Some(init) = &var.init {
                                self.emit_array_literal(out, &info, &init.value, var.span)?;
                                self.drop_temporaries(out)?;
                            }
                            self.register_owned_local(out, &c_name, info.kind)?;
                            self.vars.insert(var.name.clone(), info);
                            continue;
                        }
                        if let Kind::Text { length, .. } = kind {
                            self.line(out, &format!("{} = {{0}};", kind.c_declaration(&c_name)));
                            if let Some(init) = &var.init {
                                let (code, value_kind) = self.emit_expr(&init.value)?;
                                self.emit_text_store(out, &c_name, length, &code, value_kind)?;
                                self.drop_temporaries(out)?;
                            }
                            self.register_owned_local(out, &c_name, kind)?;
                            self.vars.insert(var.name.clone(), VarInfo { c_name, kind });
                            continue;
                        }
                        if self.kind_owns(kind) {
                            self.line(
                                out,
                                &format!(
                                    "{} = {};",
                                    kind.c_declaration(&c_name),
                                    kind.default_value()
                                ),
                            );
                            if let Some(init) = &var.init {
                                let (code, value_kind) = self.emit_expr(&init.value)?;
                                if value_kind != kind {
                                    return Err(format!(
                                        "native backend: initializer for `{}` has incompatible kind",
                                        var.name
                                    ));
                                }
                                self.copy_kind(out, &c_name, &code, kind)?;
                                self.drop_temporaries(out)?;
                            }
                            self.register_owned_local(out, &c_name, kind)?;
                            self.vars.insert(var.name.clone(), VarInfo { c_name, kind });
                            continue;
                        }
                        let init = match &var.init {
                            Some(init) => {
                                let (code, value_kind) = self.emit_expr(&init.value)?;
                                let compatible_ints = matches!(value_kind, Kind::Int(_))
                                    && matches!(kind, Kind::Int(_));
                                if value_kind != kind
                                    && !compatible_ints
                                    && !(matches!(kind, Kind::Ref(_)) && value_kind == Kind::Null)
                                {
                                    return Err(format!(
                                        "native backend: initializer for `{}` has incompatible kind",
                                        var.name
                                    ));
                                }
                                if let Kind::Int(int_kind) = kind {
                                    self.checked_int(&code, int_kind)?
                                } else {
                                    code
                                }
                            }
                            None => kind.default_value().to_owned(),
                        };
                        self.line(out, &format!("{} {} = {};", kind.c_type(), c_name, init));
                        self.vars.insert(var.name.clone(), VarInfo { c_name, kind });
                    }
                }
                Declaration::Visibility(_)
                | Declaration::Mode(_)
                | Declaration::ProcSignature(_) => {}
                Declaration::Proc(_) => {
                    return Err(
                        "native backend: nested procedure definitions are not supported".to_owned(),
                    )
                }
                _ => {
                    return Err(
                        "native backend: this declaration is not supported in native code yet"
                            .to_owned(),
                    )
                }
            }
        }
        Ok(())
    }

    fn emit_actions(&mut self, out: &mut String, actions: &[Action]) -> Result<(), String> {
        for action in actions {
            let is_return = matches!(action, Action::Return { .. });
            let unique = self.temp_counter;
            self.temp_counter += 1;
            let action_mark = format!("__action_mark_{unique}");
            self.line(out, &format!("ZCleanup *{action_mark} = z_cleanup_mark();"));
            match action {
                Action::Assign {
                    target,
                    op,
                    value,
                    span: _,
                } => self.emit_assignment(out, target, *op, value)?,
                Action::Assert { condition, .. } => {
                    let (code, _) = self.emit_expr(condition)?;
                    self.line(
                        out,
                        &format!("if (!({code})) {{ z_throw(\"ASSERTFAIL\"); }}"),
                    );
                }
                Action::Call { callee, .. } => self.emit_call(out, callee)?,
                Action::Return { value, .. } => {
                    if let Some(wrapper) = self.result_wrapper.clone() {
                        let Some((name, kind)) = self.result.clone() else {
                            return Err(
                                "native backend: internal array result without storage".to_owned()
                            );
                        };
                        if let Some(value) = value {
                            let target = VarInfo {
                                c_name: name.clone(),
                                kind,
                            };
                            self.emit_array_value_into(out, &target, value)?;
                        }
                        self.drop_temporaries(out)?;
                        self.drop_live_scopes(out)?;
                        self.emit_array_result_return(out, &name, &wrapper)?;
                    } else if let Some((name, kind)) = self.result.clone() {
                        if let Some(value) = value {
                            let (code, value_kind) = self.emit_expr(value)?;
                            if let Kind::Int(int_kind) = kind {
                                let code = self.checked_int(&code, int_kind)?;
                                self.line(out, &format!("{name} = {code};"));
                            } else if let Kind::Text { length, .. } = kind {
                                self.emit_text_store(out, &name, length, &code, value_kind)?;
                            } else if self.kind_owns(kind) {
                                self.copy_kind(out, &name, &code, kind)?;
                            } else {
                                self.line(out, &format!("{name} = {code};"));
                            }
                        }
                        self.drop_temporaries(out)?;
                        self.drop_live_scopes(out)?;
                        self.line(out, "z_handler_set(__z_entry_handlers);");
                        self.line(out, &format!("return {name};"));
                    } else {
                        self.drop_temporaries(out)?;
                        self.drop_live_scopes(out)?;
                        self.line(out, "z_handler_set(__z_entry_handlers);");
                        self.line(out, "return;");
                    }
                }
                Action::Result { value, .. } => {
                    let Some((name, kind)) = self.result.clone() else {
                        return Err("native backend: RESULT outside a value procedure".to_owned());
                    };
                    if self.result_wrapper.is_some() {
                        let target = VarInfo { c_name: name, kind };
                        self.emit_array_value_into(out, &target, value)?;
                    } else {
                        let (code, value_kind) = self.emit_expr(value)?;
                        if let Kind::Int(int_kind) = kind {
                            let code = self.checked_int(&code, int_kind)?;
                            self.line(out, &format!("{name} = {code};"));
                        } else if let Kind::Text { length, .. } = kind {
                            self.emit_text_store(out, &name, length, &code, value_kind)?;
                        } else if self.kind_owns(kind) {
                            self.copy_kind(out, &name, &code, kind)?;
                        } else {
                            self.line(out, &format!("{name} = {code};"));
                        }
                    }
                }
                Action::If {
                    branches,
                    else_actions,
                    ..
                } => {
                    for (index, branch) in branches.iter().enumerate() {
                        let (condition, _) = self.emit_expr(&branch.condition)?;
                        let keyword = if index == 0 { "if" } else { "else if" };
                        self.line(out, &format!("{keyword} ({condition}) {{"));
                        self.indent += 1;
                        self.emit_actions(out, &branch.body)?;
                        self.indent -= 1;
                        self.line(out, "}");
                    }
                    if let Some(actions) = else_actions {
                        self.line(out, "else {");
                        self.indent += 1;
                        self.emit_actions(out, actions)?;
                        self.indent -= 1;
                        self.line(out, "}");
                    }
                }
                Action::Cause { name, .. } => {
                    self.line(out, &format!("z_throw(\"{}\");", c_escape(name)));
                }
                Action::Handled {
                    action, handler, ..
                } => {
                    self.emit_handled(out, std::slice::from_ref(action.as_ref()), handler)?;
                }
                Action::Do { control, body, .. } => {
                    self.emit_do(out, control, body)?;
                }
                Action::Exit { label: None, .. } => {
                    if self.loop_depth == 0 {
                        return Err(
                            "native backend: EXIT outside a loop is not supported yet".to_owned()
                        );
                    }
                    if let Some(mark) = self.loop_marks.last().cloned() {
                        self.line(out, &format!("z_cleanup_run({mark});"));
                    }
                    self.line(out, "break;");
                }
                Action::Exit {
                    label: Some(label), ..
                } => {
                    let Some((_, target, mark)) = self
                        .labels
                        .iter()
                        .rev()
                        .find(|(name, _, _)| name == label)
                        .cloned()
                    else {
                        return Err(format!(
                            "native backend: unknown labelled EXIT target `{label}`"
                        ));
                    };
                    self.line(out, &format!("z_cleanup_run({mark});"));
                    self.line(out, &format!("goto {target};"));
                }
                Action::Labelled { label, action, .. } => {
                    let unique = self.temp_counter;
                    self.temp_counter += 1;
                    let c_label = format!("__label_{unique}_{}", c_ident(label));
                    let mark = format!("__label_mark_{unique}");
                    self.line(out, "{");
                    self.indent += 1;
                    self.line(out, &format!("ZCleanup *{mark} = z_cleanup_mark();"));
                    self.labels
                        .push((label.clone(), c_label.clone(), mark.clone()));
                    self.emit_actions(out, std::slice::from_ref(action.as_ref()))?;
                    self.line(out, &format!("z_cleanup_run({mark});"));
                    self.indent -= 1;
                    self.line(out, "}");
                    self.line(out, &format!("{c_label}: ;"));
                    self.labels.pop();
                }
                Action::Begin {
                    declarations,
                    actions,
                    ..
                } => {
                    let previous = self.vars.clone();
                    self.line(out, "{");
                    self.indent += 1;
                    self.scopes.push(Vec::new());
                    self.push_scope_marker(out)?;
                    self.emit_local_declarations(out, declarations)?;
                    self.emit_actions(out, actions)?;
                    self.drop_temporaries(out)?;
                    self.drop_scope(out)?;
                    self.indent -= 1;
                    self.line(out, "}");
                    self.vars = previous;
                }
                Action::Case {
                    selectors,
                    alternatives,
                    else_actions,
                    ..
                } => self.emit_case(out, selectors, alternatives, else_actions.as_deref())?,
                Action::Empty { .. } => {}
                _ => {
                    return Err(
                        "native backend: this action is not supported in native code yet"
                            .to_owned(),
                    )
                }
            }
            if !is_return {
                self.temporaries.clear();
                self.line(out, &format!("z_cleanup_run({action_mark});"));
            }
        }
        Ok(())
    }

    fn emit_handled(
        &mut self,
        out: &mut String,
        body: &[Action],
        handler: &Handler,
    ) -> Result<(), String> {
        let unique = self.temp_counter;
        self.temp_counter += 1;
        let state = format!("__handler_{unique}");
        let exception = format!("__exception_{unique}");
        self.line(out, "{");
        self.indent += 1;
        self.line(out, &format!("ZHandler {state};"));
        self.line(out, &format!("{state}.cleanup = z_cleanup_mark();"));
        self.line(out, &format!("{state}.prev = z_handler_current();"));
        self.line(out, &format!("z_handler_set(&{state});"));
        self.line(out, &format!("if (setjmp({state}.env) == 0) {{"));
        self.indent += 1;
        self.emit_actions(out, body)?;
        self.indent -= 1;
        self.line(out, "} else {");
        self.indent += 1;
        self.line(
            out,
            &format!("const char *{exception} = z_exception_current();"),
        );
        self.line(out, &format!("z_handler_set({state}.prev);"));
        let catch_all = handler
            .alternatives
            .iter()
            .find(|alternative| alternative.exceptions.is_empty());
        if let Some(alternative) = catch_all {
            self.emit_actions(out, &alternative.actions)?;
        } else {
            let mut wrote_condition = false;
            for alternative in &handler.alternatives {
                let condition = alternative
                    .exceptions
                    .iter()
                    .map(|name| format!("(strcmp({exception}, \"{}\") == 0)", c_escape(name)))
                    .collect::<Vec<_>>()
                    .join(" || ");
                self.line(
                    out,
                    &format!(
                        "{} ({condition}) {{",
                        if wrote_condition { "else if" } else { "if" }
                    ),
                );
                wrote_condition = true;
                self.indent += 1;
                self.emit_actions(out, &alternative.actions)?;
                self.indent -= 1;
                self.line(out, "}");
            }
            if let Some(actions) = &handler.else_actions {
                if wrote_condition {
                    self.line(out, "else {");
                    self.indent += 1;
                    self.emit_actions(out, actions)?;
                    self.indent -= 1;
                    self.line(out, "}");
                } else {
                    self.emit_actions(out, actions)?;
                }
            } else {
                if wrote_condition {
                    self.line(out, "else {");
                    self.indent += 1;
                }
                self.line(out, &format!("z_throw({exception});"));
                if wrote_condition {
                    self.indent -= 1;
                    self.line(out, "}");
                }
            }
        }
        self.indent -= 1;
        self.line(out, "}");
        self.line(out, &format!("z_handler_set({state}.prev);"));
        self.indent -= 1;
        self.line(out, "}");
        Ok(())
    }

    fn emit_case(
        &mut self,
        out: &mut String,
        selectors: &[Expr],
        alternatives: &[crate::ast::CaseAlternative],
        else_actions: Option<&[Action]>,
    ) -> Result<(), String> {
        let unique = self.temp_counter;
        self.temp_counter += 1;
        let explicit = format!("__case_explicit_{unique}");
        let mut selector_names = Vec::with_capacity(selectors.len());
        for (index, selector) in selectors.iter().enumerate() {
            let name = format!("__case_{unique}_{index}");
            let (code, _) = self.emit_expr(selector)?;
            self.line(out, &format!("const int64_t {name} = {code};"));
            selector_names.push(name);
        }

        let mut explicit_conditions = Vec::new();
        for alternative in alternatives {
            if alternative.labels.len() != selectors.len() {
                return Err(format!(
                    "native backend: CASE alternative has {} label list(s) for {} selector(s)",
                    alternative.labels.len(),
                    selectors.len()
                ));
            }
            let mut groups = Vec::new();
            let mut complete = true;
            for (labels, selector) in alternative.labels.iter().zip(&selector_names) {
                let conditions = case_labels_condition(labels, selector, None);
                if conditions.is_empty() {
                    complete = false;
                    break;
                }
                groups.push(format!("({})", conditions.join(" || ")));
            }
            if complete {
                explicit_conditions.push(format!("({})", groups.join(" && ")));
            }
        }
        if explicit_conditions.is_empty() {
            self.line(out, &format!("const int {explicit} = 0;"));
        } else {
            self.line(
                out,
                &format!(
                    "const int {explicit} = {};",
                    explicit_conditions.join(" || ")
                ),
            );
        }

        let mut first = true;
        for alternative in alternatives {
            let mut groups = Vec::new();
            for (labels, selector) in alternative.labels.iter().zip(&selector_names) {
                let conditions = case_labels_condition(labels, selector, Some(&explicit));
                groups.push(format!("({})", conditions.join(" || ")));
            }
            let condition = groups.join(" && ");
            self.line(
                out,
                &format!("{} ({condition}) {{", if first { "if" } else { "else if" }),
            );
            first = false;
            self.indent += 1;
            self.emit_actions(out, &alternative.body)?;
            self.indent -= 1;
            self.line(out, "}");
        }
        if let Some(actions) = else_actions {
            if first {
                self.emit_actions(out, actions)?;
            } else {
                self.line(out, "else {");
                self.indent += 1;
                self.emit_actions(out, actions)?;
                self.indent -= 1;
                self.line(out, "}");
            }
        }
        Ok(())
    }

    fn push_loop_marker(&mut self, out: &mut String) -> Result<String, String> {
        let unique = self.temp_counter;
        self.temp_counter += 1;
        let mark = format!("__loop_mark_{unique}");
        self.line(out, &format!("ZCleanup *{mark} = z_cleanup_mark();"));
        self.loop_marks.push(mark.clone());
        Ok(mark)
    }

    fn emit_do(
        &mut self,
        out: &mut String,
        control: &crate::ast::DoControl,
        body: &[Action],
    ) -> Result<(), String> {
        use crate::ast::DoControl;
        match control {
            DoControl::While(condition) => {
                let (condition, _) = self.emit_expr(condition)?;
                self.line(out, &format!("while ({condition}) {{"));
                self.indent += 1;
                let mark = self.push_loop_marker(out)?;
                self.loop_depth += 1;
                self.emit_actions(out, body)?;
                self.line(out, &format!("z_cleanup_run({mark});"));
                self.loop_marks.pop();
                self.loop_depth -= 1;
                self.indent -= 1;
                self.line(out, "}");
            }
            DoControl::ForEver | DoControl::None => {
                self.line(out, "for (;;) {");
                self.indent += 1;
                let mark = self.push_loop_marker(out)?;
                self.loop_depth += 1;
                self.emit_actions(out, body)?;
                self.line(out, &format!("z_cleanup_run({mark});"));
                self.loop_marks.pop();
                self.loop_depth -= 1;
                self.indent -= 1;
                self.line(out, "}");
            }
            DoControl::For {
                counter,
                start,
                end,
                step,
                down,
            } => {
                let (start_code, _) = self.emit_expr(start)?;
                let (end_code, _) = self.emit_expr(end)?;
                let step_code = match step {
                    Some(step) => self.emit_expr(step)?.0,
                    None => "1".to_owned(),
                };
                let unique = self.temp_counter;
                self.temp_counter += 1;
                let counter_c = c_ident(&format!("loop_{counter}_{unique}"));
                let end_c = format!("__loop_end_{unique}");
                let step_c = format!("__loop_step_{unique}");
                let comparison = if *down { ">=" } else { "<=" };
                let increment = if *down { "-=" } else { "+=" };

                self.line(out, "{");
                self.indent += 1;
                self.line(out, &format!("int64_t {counter_c} = {start_code};"));
                self.line(out, &format!("const int64_t {end_c} = {end_code};"));
                self.line(out, &format!("const int64_t {step_c} = {step_code};"));
                let previous = self.vars.insert(
                    counter.clone(),
                    VarInfo {
                        c_name: counter_c.clone(),
                        kind: Kind::Int(IntKind::Int),
                    },
                );
                self.line(
                    out,
                    &format!(
                        "for (; {counter_c} {comparison} {end_c}; {counter_c} {increment} {step_c}) {{"
                    ),
                );
                self.indent += 1;
                let mark = self.push_loop_marker(out)?;
                self.loop_depth += 1;
                self.emit_actions(out, body)?;
                self.line(out, &format!("z_cleanup_run({mark});"));
                self.loop_marks.pop();
                self.loop_depth -= 1;
                self.indent -= 1;
                self.line(out, "}");
                if let Some(previous) = previous {
                    self.vars.insert(counter.clone(), previous);
                } else {
                    self.vars.remove(counter);
                }
                self.indent -= 1;
                self.line(out, "}");
            }
        }
        Ok(())
    }

    fn emit_text_store(
        &mut self,
        out: &mut String,
        target: &str,
        length: usize,
        code: &str,
        kind: Kind,
    ) -> Result<(), String> {
        match kind {
            Kind::String => {
                self.line(out, &format!("z_text_assign(&{target}, {code}, {length});"));
            }
            Kind::Text { .. } => {
                self.line(out, &format!("z_text_assign_value(&{target}, {code});"));
            }
            _ => {
                return Err(
                    "native backend: text assignment requires a character string or TEXT value"
                        .to_owned(),
                )
            }
        }
        Ok(())
    }

    fn emit_array_value_into(
        &mut self,
        out: &mut String,
        target: &VarInfo,
        value: &Expr,
    ) -> Result<(), String> {
        if matches!(value, Expr::ArrayTuple { .. }) {
            return self.emit_array_literal(out, target, value, value.span());
        }
        if let Expr::Name { name, .. } = value {
            if let Some(info) = self.vars.get(name).cloned() {
                if matches!(info.kind, Kind::Array { .. }) {
                    if info.kind != target.kind {
                        return Err(
                            "native backend: array assignment requires matching modes".to_owned()
                        );
                    }
                    self.copy_kind(out, &target.c_name, &info.c_name, target.kind)?;
                    return Ok(());
                }
            }
        }
        let (code, kind) = self.emit_expr(value)?;
        let Kind::Array { element, total, .. } = target.kind else {
            return Err("native backend: array assignment target is not an array".to_owned());
        };
        match kind {
            Kind::Array {
                element: value_element,
                total: value_total,
                ..
            } => {
                if element != value_element || total != value_total {
                    return Err("native backend: array assignment mode mismatch".to_owned());
                }
                self.copy_kind(out, &target.c_name, &code, target.kind)?;
            }
            Kind::DynamicArray {
                element: value_element,
                ..
            } => {
                if element != value_element {
                    return Err("native backend: array assignment mode mismatch".to_owned());
                }
                let unique = self.temp_counter;
                self.temp_counter += 1;
                let slice = format!("__slice_assign_{unique}");
                self.line(out, &format!("ZSlice {slice} = {code};"));
                self.line(
                    out,
                    &format!(
                        "if ({slice}.len != {total}) {{ z_fail(\"RANGEFAIL\", \"array slice length mismatch\"); }}"
                    ),
                );
                let unique = self.temp_counter;
                self.temp_counter += 1;
                let index = format!("__slice_copy_i_{unique}");
                let element_kind = element.kind();
                self.line(out, "{");
                self.indent += 1;
                self.line(
                    out,
                    &format!("for (size_t {index} = 0; {index} < {total}; {index}++) {{"),
                );
                self.indent += 1;
                self.copy_kind(
                    out,
                    &format!("{}[{index}]", target.c_name),
                    &format!(
                        "(({element} *){slice}.data)[{index}]",
                        element = element.c_type()
                    ),
                    element_kind,
                )?;
                self.indent -= 1;
                self.line(out, "}");
                self.indent -= 1;
                self.line(out, "}");
            }
            _ => return Err("native backend: array assignment requires an array value".to_owned()),
        }
        Ok(())
    }

    fn emit_array_result_return(
        &mut self,
        out: &mut String,
        result_name: &str,
        wrapper: &str,
    ) -> Result<(), String> {
        self.line(out, "z_handler_set(__z_entry_handlers);");
        self.line(out, "{");
        self.indent += 1;
        self.line(out, &format!("{wrapper} __out;"));
        self.line(
            out,
            &format!("memcpy(__out.items, {result_name}, sizeof(__out.items));"),
        );
        self.line(out, "return __out;");
        self.indent -= 1;
        self.line(out, "}");
        Ok(())
    }

    fn int_bounds(&self, kind: IntKind) -> Result<(i128, i128), String> {
        types::int_bounds(kind, self.profile).ok_or_else(|| {
            format!(
                "native backend: integer width for {} is not supported",
                kind.name()
            )
        })
    }

    fn checked_int(&self, code: &str, kind: IntKind) -> Result<String, String> {
        let (low, high) = self.int_bounds(kind)?;
        if low == i64::MIN as i128 && high == i64::MAX as i128 {
            return Ok(code.to_owned());
        }
        Ok(format!("z_check_int({code}, {low}, {high})"))
    }

    fn binary_int_kind(left: Kind, right: Kind) -> Option<IntKind> {
        match (left, right) {
            (Kind::Int(left), Kind::Int(right)) if left == right => Some(left),
            (Kind::Int(left), Kind::Int(right)) => {
                if left == IntKind::Long {
                    Some(right)
                } else if right == IntKind::Long {
                    Some(left)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn array_element_of(kind: Kind) -> Result<ScalarKind, String> {
        match kind {
            Kind::Array { element, .. } | Kind::DynamicArray { element, .. } => Ok(element),
            _ => Err("native backend: array value expected".to_owned()),
        }
    }

    fn array_parts(code: &str, kind: Kind) -> Result<(String, String), String> {
        match kind {
            Kind::Array { total, .. } => Ok((code.to_owned(), total.to_string())),
            Kind::DynamicArray { .. } => Ok((format!("({code}).data"), format!("({code}).len"))),
            _ => Err("native backend: array value expected".to_owned()),
        }
    }

    fn array_equality_code(
        &self,
        left_code: &str,
        left_kind: Kind,
        right_code: &str,
        right_kind: Kind,
    ) -> Result<String, String> {
        let element = Self::array_element_of(left_kind)?;
        if element != Self::array_element_of(right_kind)? {
            return Err("native backend: array comparison requires matching modes".to_owned());
        }
        let (left_data, left_len) = Self::array_parts(left_code, left_kind)?;
        let (right_data, right_len) = Self::array_parts(right_code, right_kind)?;
        Ok(match element {
            ScalarKind::Int(_)
            | ScalarKind::Float
            | ScalarKind::Bool
            | ScalarKind::Char
            | ScalarKind::Struct(_) => {
                format!(
                    "z_slice_equal_bytes((const void *)({left_data}), {left_len}, (const void *)({right_data}), {right_len}, sizeof({}))",
                    element.c_type()
                )
            }
            ScalarKind::String => format!(
                "z_slice_equal_strings((const char **)({left_data}), {left_len}, (const char **)({right_data}), {right_len})"
            ),
            ScalarKind::PowerSet(_) => format!(
                "z_slice_equal_powersets((ZPowerSet *)({left_data}), {left_len}, (ZPowerSet *)({right_data}), {right_len})"
            ),
            ScalarKind::Text { .. } => format!(
                "z_slice_equal_text((ZText *)({left_data}), {left_len}, (ZText *)({right_data}), {right_len})"
            ),
            ScalarKind::Association | ScalarKind::Access { .. } => {
                return Err(
                    "native backend: association/access array comparison is not supported yet"
                        .to_owned(),
                )
            }
        })
    }

    fn emit_multislice_assignment(
        &mut self,
        out: &mut String,
        name: &str,
        span: crate::lexer::Span,
        op: AssignOp,
        value: &Expr,
    ) -> Result<(), String> {
        if op != AssignOp::Assign {
            return Err(
                "native backend: compound multidimensional slice assignment is not supported yet"
                    .to_owned(),
            );
        }
        let Some(ApplyResolution::MultiSlice { source: _, result }) =
            self.check.resolutions.get(&span)
        else {
            return Err("native backend: multidimensional slice has no resolution".to_owned());
        };
        let result_dims = result.dimensions();
        let Some(info) = self.vars.get(name).cloned() else {
            return Err(format!("native backend: unknown array `{name}`"));
        };
        let Kind::Array {
            element,
            dims,
            rank,
            ..
        } = info.kind
        else {
            return Err(format!("native backend: `{name}` is not an array"));
        };
        if rank != result_dims.len() {
            return Err("native backend: multidimensional slice rank mismatch".to_owned());
        }
        let mut source_strides = [1i128; 4];
        let mut result_strides = [1i128; 4];
        for index in (0..rank).rev() {
            if index + 1 < rank {
                source_strides[index] = source_strides[index + 1] * dims[index + 1].length();
                result_strides[index] = result_strides[index + 1] * result_dims[index + 1].length();
            }
        }
        let total: i128 = (0..rank).map(|index| result_dims[index].length()).product();
        let mut result_kind_dims = [Dim { low: 0, high: 0 }; 4];
        for index in 0..rank {
            result_kind_dims[index] = Dim {
                low: result_dims[index].low,
                high: result_dims[index].high,
            };
        }
        let expected = Kind::Array {
            element,
            dims: result_kind_dims,
            rank,
            total: total.max(0) as usize,
        };
        let (value_code, value_kind) = if matches!(value, Expr::ArrayTuple { .. }) {
            let code = self.emit_array_argument(value, expected)?;
            (code, expected)
        } else {
            self.emit_array_value_expr(value, element)?
        };
        let Kind::Array {
            total: value_total, ..
        } = value_kind
        else {
            return Err(
                "native backend: multidimensional slice assignment requires a fixed array value"
                    .to_owned(),
            );
        };
        if value_total != expected_total(expected) {
            return Err("native backend: array slice length mismatch".to_owned());
        }
        let unique = self.temp_counter;
        self.temp_counter += 1;
        let copy = format!("__multi_value_{unique}");
        self.line(
            out,
            &format!("{} {copy}[{}] = {{0}};", element.c_type(), total.max(0)),
        );
        for index in 0..total {
            self.copy_kind(
                out,
                &format!("{copy}[{index}]"),
                &format!("({value_code})[{index}]"),
                element.kind(),
            )?;
        }
        for flat in 0..total {
            let mut offset = 0i128;
            for index in 0..rank {
                let local = if result_strides[index] == 0 {
                    0
                } else {
                    (flat / result_strides[index]) % result_dims[index].length().max(1)
                };
                let source_index = result_dims[index].low + local;
                offset += (source_index - dims[index].low) * source_strides[index];
            }
            self.copy_kind(
                out,
                &format!("{}[{offset}]", info.c_name),
                &format!("{copy}[{flat}]"),
                element.kind(),
            )?;
        }
        self.drop_kind(out, &copy, expected)?;
        Ok(())
    }

    fn emit_array_value_expr(
        &mut self,
        expression: &Expr,
        expected_element: ScalarKind,
    ) -> Result<(String, Kind), String> {
        if let Expr::Name { name, .. } = expression {
            let Some(info) = self.vars.get(name).cloned() else {
                return Err(format!("native backend: unknown array `{name}`"));
            };
            if let Kind::Array { element, total, .. } = info.kind {
                let _ = total;
                if element != expected_element {
                    return Err("native backend: array element mode mismatch".to_owned());
                }
                return Ok((info.c_name, info.kind));
            }
        }
        if let Expr::ArrayTuple { elements, .. } = expression {
            let total = elements.len();
            let mut dims = [Dim { low: 0, high: 0 }; 4];
            dims[0] = Dim {
                low: 1,
                high: total as i128,
            };
            let expected = Kind::Array {
                element: expected_element,
                dims,
                rank: 1,
                total,
            };
            let code = self.emit_array_argument(expression, expected)?;
            return Ok((code, expected));
        }
        let (code, kind) = self.emit_expr(expression)?;
        match kind {
            Kind::Array { element, .. } | Kind::DynamicArray { element, .. } => {
                if element != expected_element {
                    return Err("native backend: array element mode mismatch".to_owned());
                }
                Ok((code, kind))
            }
            _ => Err("native backend: array value expected".to_owned()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_slice_assignment(
        &mut self,
        out: &mut String,
        name: &str,
        first: &Expr,
        last: &Expr,
        up: bool,
        span: crate::lexer::Span,
        op: AssignOp,
        value: &Expr,
    ) -> Result<(), String> {
        if op != AssignOp::Assign {
            return Err(
                "native backend: compound slice assignment is not supported yet".to_owned(),
            );
        }
        let Some(info) = self.vars.get(name).cloned() else {
            return Err(format!("native backend: unknown array `{name}`"));
        };
        let Kind::Array {
            element,
            dims,
            rank,
            ..
        } = info.kind
        else {
            return Err(format!("native backend: `{name}` is not an array"));
        };
        if rank != 1 {
            return Err(
                "native backend: multidimensional slice assignment is not supported yet".to_owned(),
            );
        }
        let low = dims[0].low;
        let high = dims[0].high;
        let (dest, target_len, target_is_fixed) = match self.check.types.get(&span) {
            Some(Type::Array(_)) => {
                let Some(first) = crate::sema::constant_int(first) else {
                    return Err("native backend: constant slice bounds expected".to_owned());
                };
                let Some(last) = crate::sema::constant_int(last) else {
                    return Err("native backend: constant slice bounds expected".to_owned());
                };
                let upper = if up {
                    first
                        .checked_add(last - 1)
                        .ok_or_else(|| "array slice bounds overflow".to_owned())?
                } else {
                    last
                };
                let count = (upper - first + 1).max(0);
                let start = first - low;
                (
                    format!("({} + {start})", info.c_name),
                    count.to_string(),
                    true,
                )
            }
            Some(Type::DynamicArray { .. }) => {
                let (first_code, _) = self.emit_expr(first)?;
                let (last_code, _) = self.emit_expr(last)?;
                let unique = self.temp_counter;
                self.temp_counter += 1;
                let slice = format!("__slice_target_{unique}");
                self.line(
                    out,
                    &format!(
                        "ZSlice {slice} = z_slice_make((void *)({}), {}, {}, {}, {}, {}, sizeof({}));",
                        info.c_name,
                        low,
                        high,
                        first_code,
                        last_code,
                        if up { 1 } else { 0 },
                        element.c_type()
                    ),
                );
                (
                    format!("(({element} *){slice}.data)", element = element.c_type()),
                    format!("{slice}.len"),
                    false,
                )
            }
            _ => return Err("native backend: slice target has no array mode".to_owned()),
        };
        let (value_code, value_kind) = self.emit_array_value_expr(value, element)?;
        let (value_ptr, value_len, value_is_fixed) = match value_kind {
            Kind::Array { total, .. } => (value_code, total.to_string(), true),
            Kind::DynamicArray { .. } => {
                let unique = self.temp_counter;
                self.temp_counter += 1;
                let slice = format!("__slice_value_{unique}");
                self.line(out, &format!("ZSlice {slice} = {value_code};"));
                (format!("{slice}.data"), format!("{slice}.len"), false)
            }
            _ => return Err("native backend: slice assignment requires an array value".to_owned()),
        };
        if value_is_fixed && target_is_fixed && value_len != target_len {
            return Err("native backend: array slice length mismatch".to_owned());
        }
        if !(value_is_fixed && target_is_fixed) {
            self.line(
                out,
                &format!(
                    "if ({value_len} != {target_len}) {{ z_fail(\"RANGEFAIL\", \"array slice length mismatch\"); }}"
                ),
            );
        }
        let unique = self.temp_counter;
        self.temp_counter += 1;
        let index = format!("__slice_assign_i_{unique}");
        let element_kind = element.kind();
        self.line(out, "{");
        self.indent += 1;
        self.line(
            out,
            &format!("for (size_t {index} = 0; {index} < (size_t)({target_len}); {index}++) {{"),
        );
        self.indent += 1;
        self.copy_kind(
            out,
            &format!("({dest})[{index}]"),
            &format!("(({} *){value_ptr})[{index}]", element.c_type()),
            element_kind,
        )?;
        self.indent -= 1;
        self.line(out, "}");
        self.indent -= 1;
        self.line(out, "}");
        Ok(())
    }

    fn emit_assignment(
        &mut self,
        out: &mut String,
        target: &Expr,
        op: AssignOp,
        value: &Expr,
    ) -> Result<(), String> {
        if let Expr::ArraySlice {
            name,
            first,
            last,
            up,
            span,
        } = target
        {
            return self.emit_slice_assignment(out, name, first, last, *up, *span, op, value);
        }
        if let Expr::MultiSlice { name, span, .. } = target {
            return self.emit_multislice_assignment(out, name, *span, op, value);
        }
        let (target_code, target_kind) = match target {
            Expr::Name { name, .. } => {
                let Some(info) = self.vars.get(name).cloned() else {
                    return Err(format!("native backend: unknown variable `{name}`"));
                };
                if matches!(info.kind, Kind::Array { .. }) {
                    if op != AssignOp::Assign {
                        return Err(
                            "native backend: compound whole-array assignment is not supported yet"
                                .to_owned(),
                        );
                    }
                    if let Expr::Name { name: source, .. } = value {
                        let Some(source_info) = self.vars.get(source).cloned() else {
                            return Err(format!("native backend: unknown array `{source}`"));
                        };
                        if source_info.kind != info.kind {
                            return Err("native backend: array assignment requires matching modes"
                                .to_owned());
                        }
                        self.copy_kind(out, &info.c_name, &source_info.c_name, info.kind)?;
                        return Ok(());
                    }
                    self.emit_array_value_into(out, &info, value)?;
                    return Ok(());
                }
                (info.c_name, info.kind)
            }
            Expr::Apply {
                name, args, span, ..
            } => {
                let Some(info) = self.vars.get(name).cloned() else {
                    return Err(format!("native backend: unknown array `{name}`"));
                };
                if !matches!(
                    self.check.resolutions.get(span),
                    Some(ApplyResolution::ArrayIndex(Type::Array(_)))
                        | Some(ApplyResolution::ArrayIndex(Type::MultiArray(_)))
                ) {
                    return Err(format!("native backend: `{name}` is not an indexed array"));
                }
                self.array_index_expr(&info, args, *span)?
            }
            Expr::DerefIndex {
                base,
                indices,
                span,
                ..
            } => self.deref_index_expr(base, indices, *span)?,
            Expr::Field { .. } => self.emit_field_location(target)?,
            Expr::Deref { base, .. } => {
                let (code, kind) = self.emit_expr(base)?;
                let Kind::Ref(element) = kind else {
                    return Err("native backend: dereference target is not a reference".to_owned());
                };
                (
                    format!("Z_DEREF({}, {code})", element.c_type()),
                    element.kind(),
                )
            }
            _ => {
                return Err(
                    "native backend: this assignment target is not supported in native code yet"
                        .to_owned(),
                )
            }
        };
        let (code, value_kind) = self.emit_expr(value)?;
        if let Kind::Int(kind) = target_kind {
            let result = match op {
                AssignOp::Assign => self.checked_int(&code, kind)?,
                AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div => {
                    let function = match op {
                        AssignOp::Add => "z_add",
                        AssignOp::Sub => "z_sub",
                        AssignOp::Mul => "z_mul",
                        AssignOp::Div => "z_div",
                        _ => unreachable!(),
                    };
                    self.checked_int(&format!("{function}({target_code}, {code})"), kind)?
                }
                AssignOp::Concat => {
                    return Err(
                        "native backend: `//=` requires a character-string target".to_owned()
                    )
                }
            };
            self.line(out, &format!("{target_code} = {result};"));
            return Ok(());
        }
        if let Kind::Text { length, .. } = target_kind {
            if op != AssignOp::Assign {
                return Err(
                    "native backend: compound text assignment is not supported yet".to_owned(),
                );
            }
            self.emit_text_store(out, &target_code, length, &code, value_kind)?;
            return Ok(());
        }
        if target_kind == Kind::String {
            match op {
                AssignOp::Assign => {
                    self.copy_kind(out, &target_code, &code, Kind::String)?;
                }
                AssignOp::Concat => {
                    let value =
                        self.temporary(&format!("z_concat({target_code}, {code})"), Kind::String)?;
                    self.copy_kind(out, &target_code, &value, Kind::String)?;
                }
                _ => {
                    return Err(
                        "native backend: this compound string assignment is not supported"
                            .to_owned(),
                    )
                }
            }
            return Ok(());
        }
        if matches!(target_kind, Kind::Struct(_) | Kind::Array { .. }) {
            if op != AssignOp::Assign {
                return Err(
                    "native backend: compound aggregate assignment is not supported yet".to_owned(),
                );
            }
            self.copy_kind(out, &target_code, &code, target_kind)?;
            return Ok(());
        }
        if let Kind::PowerSet(_) = target_kind {
            let value = match op {
                AssignOp::Assign => code.clone(),
                AssignOp::Add | AssignOp::Sub | AssignOp::Mul => {
                    let function = match op {
                        AssignOp::Add => "z_ps_union",
                        AssignOp::Sub => "z_ps_difference",
                        AssignOp::Mul => "z_ps_intersection",
                        _ => unreachable!(),
                    };
                    self.temporary(&format!("{function}({target_code}, {code})"), target_kind)?
                }
                AssignOp::Div | AssignOp::Concat => {
                    return Err(
                        "native backend: this powerset compound assignment is not supported"
                            .to_owned(),
                    )
                }
            };
            self.copy_kind(out, &target_code, &value, target_kind)?;
            return Ok(());
        }
        let operator = match op {
            AssignOp::Assign => "=",
            AssignOp::Add => "+=",
            AssignOp::Sub => "-=",
            AssignOp::Mul => "*=",
            AssignOp::Div => "/=",
            AssignOp::Concat => {
                let string_target = match target {
                    Expr::Name { name, .. } => self
                        .vars
                        .get(name)
                        .is_some_and(|info| info.kind == Kind::String),
                    Expr::Apply { name, .. } => self.vars.get(name).is_some_and(|info| {
                        matches!(
                            info.kind,
                            Kind::Array {
                                element: ScalarKind::String,
                                ..
                            }
                        )
                    }),
                    _ => false,
                };
                if !string_target {
                    return Err(
                        "native backend: `//=` requires a character-string target".to_owned()
                    );
                }
                self.line(
                    out,
                    &format!("{} = z_concat({}, {code});", target_code, target_code),
                );
                return Ok(());
            }
        };
        self.line(out, &format!("{target_code} {operator} {code};"));
        Ok(())
    }

    fn association_target(&self, expression: &Expr) -> Result<(String, Kind), String> {
        let Expr::Name { name, .. } = expression else {
            return Err(
                "native backend: association/access operations require simple variables".to_owned(),
            );
        };
        let info = self
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| format!("native backend: unknown variable `{name}`"))?;
        if matches!(info.kind, Kind::Association | Kind::Access { .. }) {
            Ok((info.c_name, info.kind))
        } else {
            Err(format!(
                "native backend: `{name}` is not an ASSOCIATION or ACCESS variable"
            ))
        }
    }

    fn transfer_target(&mut self, expression: &Expr) -> Result<(String, Kind), String> {
        if let Expr::Deref { .. } = expression {
            let (code, kind) = self.emit_expr(expression)?;
            if matches!(kind, Kind::Access { .. } | Kind::Text { .. }) {
                return Ok((code, kind));
            }
            return Err(
                "native backend: dereferenced transfer target must be ACCESS or TEXT".to_owned(),
            );
        }
        let Expr::Name { name, .. } = expression else {
            return Err(
                "native backend: transfer operations require variables or dereferenced references"
                    .to_owned(),
            );
        };
        let info = self
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| format!("native backend: unknown variable `{name}`"))?;
        if matches!(info.kind, Kind::Access { .. } | Kind::Text { .. }) {
            Ok((info.c_name, info.kind))
        } else {
            Err(format!(
                "native backend: `{name}` is not an ACCESS or TEXT location"
            ))
        }
    }

    fn emit_association_expr(
        &mut self,
        op: crate::builtins::AssociationOp,
        has_index: bool,
        args: &[Expr],
    ) -> Result<(String, Kind), String> {
        use crate::builtins::AssociationOp;
        match op {
            AssociationOp::IsAssociated => {
                let (target, _) = self.association_target(&args[0])?;
                Ok((
                    format!("z_association_is_associated(&{target})"),
                    Kind::Bool,
                ))
            }
            AssociationOp::Existing
            | AssociationOp::Readable
            | AssociationOp::Writeable
            | AssociationOp::Indexable
            | AssociationOp::Sequencible
            | AssociationOp::Variable => {
                let (target, _) = self.association_target(&args[0])?;
                let attribute = match op {
                    AssociationOp::Existing => 0,
                    AssociationOp::Readable => 1,
                    AssociationOp::Writeable => 2,
                    AssociationOp::Indexable => 3,
                    AssociationOp::Sequencible => 4,
                    AssociationOp::Variable => 5,
                    _ => unreachable!(),
                };
                Ok((
                    format!("z_association_attribute(&{target}, {attribute})"),
                    Kind::Bool,
                ))
            }
            AssociationOp::GetAssociation => {
                let (target, _) = self.association_target(&args[0])?;
                Ok((
                    format!("z_get_association(&{target})"),
                    Kind::Ref(ScalarKind::Association),
                ))
            }
            AssociationOp::GetUsage => {
                let (target, kind) = self.transfer_target(&args[0])?;
                let call = if matches!(kind, Kind::Text { .. }) {
                    format!("z_text_usage(&{target})")
                } else {
                    format!("z_access_usage(&{target})")
                };
                Ok((call, Kind::Int(IntKind::Int)))
            }
            AssociationOp::OutOfFile => {
                let (target, kind) = self.transfer_target(&args[0])?;
                let call = if matches!(kind, Kind::Text { .. }) {
                    format!("z_text_out_of_file(&{target})")
                } else {
                    format!("z_access_out_of_file(&{target})")
                };
                Ok((call, Kind::Bool))
            }
            AssociationOp::ReadRecord => {
                let (target, kind) = self.association_target(&args[0])?;
                let Kind::Access { low, high, .. } = kind else {
                    return Err("native backend: READRECORD requires an ACCESS location".to_owned());
                };
                let index_code = if has_index {
                    let Some(index) = args.get(1) else {
                        return Err("native backend: READRECORD is missing its index".to_owned());
                    };
                    self.emit_expr(index)?.0
                } else {
                    "0".to_owned()
                };
                let value = self.temporary(
                    &format!(
                        "z_read_record(&{target}, {}, {index_code}, {low}, {high})",
                        if has_index { 1 } else { 0 }
                    ),
                    Kind::String,
                )?;
                Ok((value, Kind::String))
            }
            _ => Err(
                "native backend: this association operation is not supported as a value".to_owned(),
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_association_call(
        &mut self,
        out: &mut String,
        op: crate::builtins::AssociationOp,
        has_index: bool,
        has_target: bool,
        has_usage: bool,
        has_where: bool,
        args: &[Expr],
    ) -> Result<(), String> {
        use crate::builtins::AssociationOp;
        match op {
            AssociationOp::Associate => {
                let (target, _) = self.association_target(&args[0])?;
                let (path, _) = self.emit_expr(&args[1])?;
                self.line(out, &format!("z_associate(&{target}, {path});"));
            }
            AssociationOp::Dissociate => {
                let (target, _) = self.association_target(&args[0])?;
                self.line(out, &format!("z_dissociate(&{target});"));
            }
            AssociationOp::Create => {
                let (target, _) = self.association_target(&args[0])?;
                self.line(out, &format!("z_create_association(&{target});"));
            }
            AssociationOp::Delete => {
                let (target, _) = self.association_target(&args[0])?;
                self.line(out, &format!("z_delete_association(&{target});"));
            }
            AssociationOp::Modify => {}
            AssociationOp::Connect => {
                let (target, kind) = self.transfer_target(&args[0])?;
                let (association, _) = self.association_target(&args[1])?;
                let (indexed, low, high, is_text) = match kind {
                    Kind::Access { indexed, low, high } => (indexed, low, high, false),
                    Kind::Text {
                        indexed, low, high, ..
                    } => (indexed, low, high, true),
                    _ => unreachable!(),
                };
                let mut offset = 2;
                let usage = if has_usage {
                    let code = self.emit_expr(&args[offset])?.0;
                    offset += 1;
                    code
                } else {
                    "2".to_owned()
                };
                let where_value = if has_where {
                    let code = self.emit_expr(&args[offset])?.0;
                    offset += 1;
                    code
                } else {
                    "0".to_owned()
                };
                let index = if has_index {
                    self.emit_expr(&args[offset])?.0
                } else {
                    "0".to_owned()
                };
                let function = if is_text {
                    "z_connect_text"
                } else {
                    "z_connect_access"
                };
                self.line(
                    out,
                    &format!(
                        "{function}(&{target}, &{association}, {usage}, {where_value}, {}, {}, {index}, {low}, {high});",
                        if indexed { 1 } else { 0 },
                        if has_index { 1 } else { 0 }
                    ),
                );
            }
            AssociationOp::Disconnect => {
                let (target, kind) = self.transfer_target(&args[0])?;
                if matches!(kind, Kind::Access { .. }) {
                    self.line(out, &format!("z_disconnect_access(&{target});"));
                } else {
                    self.line(out, &format!("z_disconnect_text(&{target});"));
                }
            }
            AssociationOp::ReadRecord => {
                let (target, kind) = self.association_target(&args[0])?;
                let Kind::Access { low, high, .. } = kind else {
                    return Err("native backend: READRECORD requires an ACCESS location".to_owned());
                };
                let index_code = if has_index {
                    self.emit_expr(&args[1])?.0
                } else {
                    "0".to_owned()
                };
                let value = self.temporary(
                    &format!(
                        "z_read_record(&{target}, {}, {index_code}, {low}, {high})",
                        if has_index { 1 } else { 0 }
                    ),
                    Kind::String,
                )?;
                if has_target {
                    let target_index = if has_index { 2 } else { 1 };
                    let target_expr = &args[target_index];
                    let (code, kind) = self.association_target(target_expr).or_else(|_| {
                        let Expr::Name { name, .. } = target_expr else {
                            return Err(
                                "native backend: READRECORD target must be a variable".to_owned()
                            );
                        };
                        let info = self.vars.get(name).cloned().ok_or_else(|| {
                            format!("native backend: unknown READRECORD target `{name}`")
                        })?;
                        Ok((info.c_name, info.kind))
                    })?;
                    self.copy_kind(out, &code, &value, kind)?;
                }
            }
            AssociationOp::WriteRecord => {
                let (target, kind) = self.association_target(&args[0])?;
                let Kind::Access { low, high, .. } = kind else {
                    return Err(
                        "native backend: WRITERECORD requires an ACCESS location".to_owned()
                    );
                };
                let (index_code, value_index) = if has_index {
                    (self.emit_expr(&args[1])?.0, 2)
                } else {
                    ("0".to_owned(), 1)
                };
                let (value, _) = self.emit_expr(&args[value_index])?;
                self.line(
                    out,
                    &format!(
                        "z_write_record(&{target}, {}, {index_code}, {value}, {low}, {high});",
                        if has_index { 1 } else { 0 }
                    ),
                );
            }
            _ => return Err(
                "native backend: this association operation is not supported in native code yet"
                    .to_owned(),
            ),
        }
        Ok(())
    }

    fn text_io_target(&self, expression: &Expr) -> Result<TextTarget, String> {
        let Expr::Name { name, span } = expression else {
            return Err(
                "native backend: text operations require a simple text variable".to_owned(),
            );
        };
        let info = self
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| format!("native backend: unknown text variable `{name}`"))?;
        match info.kind {
            Kind::Text {
                length,
                indexed,
                low,
                high,
            } => Ok(TextTarget::Text {
                code: info.c_name,
                length,
                indexed,
                low,
                high,
            }),
            Kind::String => {
                let limit = match self.check.types.get(span) {
                    Some(Type::BoundedString {
                        length,
                        varying: true,
                    }) => length.to_string(),
                    Some(Type::String) => "((size_t)-1)".to_owned(),
                    Some(Type::WString) | Some(Type::WBoundedString { .. }) => {
                        return Err(
                            "native backend: wide text transfer targets are not supported yet"
                                .to_owned(),
                        )
                    }
                    _ => "((size_t)-1)".to_owned(),
                };
                Ok(TextTarget::String {
                    code: info.c_name,
                    limit,
                })
            }
            _ => Err(format!(
                "native backend: `{name}` is not a supported text transfer target"
            )),
        }
    }

    fn text_location(&self, expression: &Expr) -> Result<(String, usize), String> {
        let Expr::Name { name, .. } = expression else {
            return Err(
                "native backend: text operations require a simple text variable".to_owned(),
            );
        };
        let info = self
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| format!("native backend: unknown text variable `{name}`"))?;
        let Kind::Text { length, .. } = info.kind else {
            return Err(format!(
                "native backend: `{name}` is not a narrow sequential TEXT location"
            ));
        };
        Ok((info.c_name, length))
    }

    fn emit_text_io(
        &mut self,
        out: &mut String,
        args: &[Expr],
        kind: TextIo,
        format: &[FormatOp],
        indexed: bool,
    ) -> Result<(), String> {
        let Some(target_expr) = args.first() else {
            return Err("native backend: text transfer requires a target".to_owned());
        };
        let text_target = self.text_io_target(target_expr)?;
        let (target, length, is_text, string_target, text_indexed, text_low, text_high) =
            match text_target {
                TextTarget::Text {
                    code,
                    length,
                    indexed,
                    low,
                    high,
                } => (code, length.to_string(), true, None, indexed, low, high),
                TextTarget::String { code, limit } => {
                    let unique = self.temp_counter;
                    self.temp_counter += 1;
                    let temp = format!("__text_string_{unique}");
                    self.line(out, &format!("ZText {temp} = z_text_from_string({code});"));
                    (temp, limit, false, Some(code), false, 0, 0)
                }
            };
        let mut offset = 1;
        let index_code = if indexed {
            let Some(index) = args.get(1) else {
                return Err("native backend: text transfer is missing its index".to_owned());
            };
            let (value, _) = self.emit_expr(index)?;
            offset = 2;
            Some(value)
        } else {
            None
        };
        let has_index = if indexed { 1 } else { 0 };
        let index_value = index_code.as_deref().unwrap_or("0");
        let mut read_guard = None;
        if is_text {
            match kind {
                TextIo::Write => self.line(
                    out,
                    &format!(
                        "z_text_begin_write(&{target}, {has_index}, {index_value}, {text_low}, {text_high}, {length});"
                    ),
                ),
                TextIo::Read => {
                    let unique = self.temp_counter;
                    self.temp_counter += 1;
                    let guard = format!("__text_connected_{unique}");
                    self.line(
                        out,
                        &format!(
                            "int {guard} = z_text_begin_read(&{target}, {has_index}, {index_value}, {text_low}, {text_high}, {length});"
                        ),
                    );
                    self.line(out, &format!("if ({guard} != 0) {{"));
                    self.indent += 1;
                    read_guard = Some(guard);
                }
            }
        } else if let Some(index) = &index_code {
            self.line(
                out,
                &format!("z_text_set_index(&{target}, {index}, {length});"),
            );
        }
        let _ = text_indexed;
        let mut io_index = offset + 1;
        for operation in format {
            match operation {
                FormatOp::Text(text) => match kind {
                    TextIo::Write => self.line(
                        out,
                        &format!("z_text_write(&{target}, \"{}\", {length});", c_escape(text)),
                    ),
                    TextIo::Read => self.line(
                        out,
                        &format!(
                            "z_text_read_literal(&{target}, \"{}\", {length});",
                            c_escape(text)
                        ),
                    ),
                },
                FormatOp::Convert(conversion) => {
                    text_conversion_supported(conversion, kind)?;
                    let width_code = match conversion.width {
                        ClauseWidth::Fixed(width) => width.to_string(),
                        ClauseWidth::Variable => {
                            let Some(width_arg) = args.get(io_index) else {
                                return Err(
                                    "native backend: text transfer is missing a variable width argument"
                                        .to_owned(),
                                );
                            };
                            io_index += 1;
                            self.emit_expr(width_arg)?.0
                        }
                    };
                    let Some(argument) = args.get(io_index) else {
                        return Err(
                            "native backend: text transfer is missing an io argument".to_owned()
                        );
                    };
                    io_index += 1;
                    let left = if conversion.left { 1 } else { 0 };
                    let overflow = if conversion.overflow { 1 } else { 0 };
                    let pad = conversion.pad as u32;
                    let base = match conversion.code {
                        ConversionCode::Binary => 2,
                        ConversionCode::Octal => 8,
                        ConversionCode::Hex => 16,
                        ConversionCode::Default | ConversionCode::Scientific => 10,
                    };
                    let fractional = conversion
                        .fractional_width
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-1".to_owned());
                    let exponent = conversion
                        .exponent_width
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-1".to_owned());
                    let scientific = if matches!(conversion.code, ConversionCode::Scientific) {
                        1
                    } else {
                        0
                    };
                    match kind {
                        TextIo::Write => {
                            let (code, value_kind) = self.emit_expr(argument)?;
                            let formats = format!("{width_code}, {left}, (char){pad}, {overflow}");
                            match value_kind {
                                Kind::Int(_) => self.line(
                                    out,
                                    &format!(
                                        "z_text_write_int_format(&{target}, {code}, {length}, {base}, {formats});"
                                    ),
                                ),
                                Kind::Bool => self.line(
                                    out,
                                    &format!(
                                        "z_text_write_bool_format(&{target}, {code}, {length}, {formats});"
                                    ),
                                ),
                                Kind::Char => self.line(
                                    out,
                                    &format!(
                                        "z_text_write_char_format(&{target}, {code}, {length}, {formats});"
                                    ),
                                ),
                                Kind::String => self.line(
                                    out,
                                    &format!(
                                        "z_text_write_string_format(&{target}, {code}, {length}, {formats});"
                                    ),
                                ),
                                Kind::Float => self.line(
                                    out,
                                    &format!(
                                        "z_text_write_float_format(&{target}, {code}, {length}, {scientific}, {formats}, {fractional}, {exponent});"
                                    ),
                                ),
                                _ => return Err(
                                    "native backend: this value cannot be written with a text conversion yet"
                                        .to_owned(),
                                ),
                            }
                        }
                        TextIo::Read => {
                            let Expr::Name { name, .. } = argument else {
                                return Err(
                                    "native backend: READTEXT conversion targets must be simple variables"
                                        .to_owned(),
                                );
                            };
                            let Some(info) = self.vars.get(name).cloned() else {
                                return Err(format!(
                                    "native backend: unknown READTEXT target `{name}`"
                                ));
                            };
                            let read = match info.kind {
                                Kind::Int(_) => format!(
                                    "z_text_read_int_format(&{target}, {length}, {base}, {width_code}, {left}, (char){pad})"
                                ),
                                Kind::Char => format!(
                                    "z_text_read_char_format(&{target}, {length}, {width_code}, {left}, (char){pad})"
                                ),
                                Kind::Bool => format!(
                                    "z_text_read_bool_format(&{target}, {length}, {width_code}, {left}, (char){pad})"
                                ),
                                Kind::Float => format!(
                                    "z_text_read_float_format(&{target}, {length}, {width_code}, {left}, (char){pad})"
                                ),
                                Kind::String => {
                                    let value = self.temporary(
                                        &format!(
                                            "z_text_read_string_format(&{target}, {length}, {width_code})"
                                        ),
                                        Kind::String,
                                    )?;
                                    self.copy_kind(
                                        out,
                                        &info.c_name,
                                        &value,
                                        Kind::String,
                                    )?;
                                    continue;
                                }
                                _ => {
                                    return Err(
                                        "native backend: this READTEXT target mode is not supported yet"
                                            .to_owned(),
                                    )
                                }
                            };
                            let read = if let Kind::Int(kind) = info.kind {
                                self.checked_int(&read, kind)?
                            } else {
                                read
                            };
                            self.line(out, &format!("{} = {read};", info.c_name));
                        }
                    }
                }
                FormatOp::Edit(editing) => {
                    let width_code = match editing.width {
                        ClauseWidth::Fixed(width) => width.to_string(),
                        ClauseWidth::Variable => {
                            let Some(width_arg) = args.get(io_index) else {
                                return Err(
                                    "native backend: text transfer is missing a variable width argument"
                                        .to_owned(),
                                );
                            };
                            io_index += 1;
                            self.emit_expr(width_arg)?.0
                        }
                    };
                    let code = match editing.code {
                        EditingCode::Space => 0,
                        EditingCode::SkipRight => 1,
                        EditingCode::SkipLeft => 2,
                        EditingCode::Tab => 3,
                    };
                    let function = match kind {
                        TextIo::Write => "z_text_edit_write",
                        TextIo::Read => "z_text_edit_read",
                    };
                    self.line(
                        out,
                        &format!("{function}(&{target}, {code}, {width_code}, {length});"),
                    );
                }
                FormatOp::Io(control) => {
                    let code = match control {
                        IoControl::NextRecord => 0,
                        IoControl::NextPage => 1,
                        IoControl::CurrentLine => 2,
                        IoControl::Prompt => 3,
                        IoControl::Emit => 4,
                        IoControl::EndPage => 5,
                    };
                    let function = match kind {
                        TextIo::Write => "z_text_io_write",
                        TextIo::Read => "z_text_io_read",
                    };
                    self.line(out, &format!("{function}(&{target}, {code}, {length});"));
                }
            }
        }
        if io_index != args.len() {
            return Err("native backend: text io argument count mismatch".to_owned());
        }
        if let Some(_guard) = read_guard {
            self.drop_temporaries(out)?;
            self.indent -= 1;
            self.line(out, "}");
        }
        if is_text {
            if matches!(kind, TextIo::Write) {
                self.line(out, &format!("z_text_end_write(&{target});"));
            }
            self.line(out, &format!("z_text_reset_index(&{target});"));
        } else {
            if matches!(kind, TextIo::Write) {
                if let Some(code) = &string_target {
                    self.line(
                        out,
                        &format!("z_string_assign(&{code}, z_text_view(&{target}));"),
                    );
                }
            }
            self.line(out, &format!("z_text_drop(&{target});"));
        }
        Ok(())
    }

    fn emit_call(&mut self, out: &mut String, callee: &Expr) -> Result<(), String> {
        let Expr::Apply { name, args, .. } = callee else {
            return Err("native backend: only direct procedure calls are supported".to_owned());
        };
        match name.as_str() {
            "WRITELINE" | "WRITE" => {
                if args.len() > 1 {
                    return Err(format!(
                        "native backend: `{name}` currently supports at most one argument"
                    ));
                }
                let newline = name == "WRITELINE";
                if let Some(argument) = args.first() {
                    let (code, kind) = self.emit_expr(argument)?;
                    let format =
                        match kind {
                            Kind::String => {
                                if newline {
                                    format!("puts({code});")
                                } else {
                                    format!("fputs({code}, stdout);")
                                }
                            }
                            Kind::Char => {
                                if newline {
                                    format!("printf(\"%c\\n\", {code});")
                                } else {
                                    format!("printf(\"%c\", {code});")
                                }
                            }
                            Kind::Bool => {
                                if newline {
                                    format!("printf(\"%d\\n\", {code});")
                                } else {
                                    format!("printf(\"%d\", {code});")
                                }
                            }
                            Kind::Int(_) => {
                                if newline {
                                    format!("printf(\"%lld\\n\", (long long)({code}));")
                                } else {
                                    format!("printf(\"%lld\", (long long)({code}));")
                                }
                            }
                            Kind::Array { .. }
                            | Kind::Ref(_)
                            | Kind::RefArray { .. }
                            | Kind::Struct(_)
                            | Kind::PowerSet(_)
                            | Kind::Text { .. }
                            | Kind::Association
                            | Kind::Access { .. }
                            | Kind::Float
                            | Kind::DynamicArray { .. }
                            | Kind::Null => return Err(
                                "native backend: arrays, references, structures, powersets, and NULL cannot be passed to WRITE/WRITELINE yet"
                                    .to_owned(),
                            ),
                        };
                    self.line(out, &format);
                } else if newline {
                    self.line(out, "putchar('\\n');");
                }
                Ok(())
            }
            _ => {
                if let Expr::Apply {
                    name, args, span, ..
                } = callee
                {
                    match self.check.resolutions.get(span) {
                        Some(ApplyResolution::TextSub {
                            op: crate::builtins::TextSubOp::SetAccess,
                        }) => {
                            let (text, _) = self.text_location(&args[0])?;
                            let (access, _) = self.association_target(&args[1])?;
                            self.line(out, &format!("z_text_set_access(&{text}, &{access});"));
                            return Ok(());
                        }
                        Some(ApplyResolution::TextSub {
                            op: crate::builtins::TextSubOp::SetRecord,
                        }) => {
                            let (text, _) = self.text_location(&args[0])?;
                            let (source, kind) = self.emit_expr(&args[1])?;
                            if kind != Kind::String {
                                return Err(
                                    "native backend: SETTEXTRECORD source must be CHARS".to_owned()
                                );
                            }
                            self.line(out, &format!("z_text_set_record(&{text}, &({source}));"));
                            return Ok(());
                        }
                        Some(ApplyResolution::Association {
                            op,
                            index,
                            target,
                            usage,
                            where_expr,
                        }) => {
                            let op = *op;
                            let has_index = *index;
                            let has_target = *target;
                            let has_usage = *usage;
                            let has_where = *where_expr;
                            self.emit_association_call(
                                out, op, has_index, has_target, has_usage, has_where, args,
                            )?;
                            return Ok(());
                        }
                        Some(ApplyResolution::TextIndex { get: false }) => {
                            if args.len() != 2 {
                                return Err(
                                    "native backend: malformed SETTEXTINDEX call".to_owned()
                                );
                            }
                            let (target, length) = self.text_location(&args[0])?;
                            let (value, _) = self.emit_expr(&args[1])?;
                            self.line(
                                out,
                                &format!("z_text_set_index(&{target}, {value}, {length});"),
                            );
                            return Ok(());
                        }
                        Some(ApplyResolution::TextIo {
                            kind,
                            format,
                            indexed,
                        }) => {
                            let kind = *kind;
                            let format = format.clone();
                            let indexed = *indexed;
                            self.emit_text_io(out, args, kind, &format, indexed)?;
                            return Ok(());
                        }
                        _ => {}
                    }
                    if let Some(ApplyResolution::Procedure(signature)) =
                        self.check.resolutions.get(span)
                    {
                        if let Some(result_ty) = signature.result {
                            let result_kind = kind_from_type(result_ty)?;
                            if self.kind_owns(result_kind) {
                                let _ = self.emit_procedure_call(name, args, *span)?;
                                return Ok(());
                            }
                        }
                        let lowered = self.lower_call_args(args, signature)?;
                        let (target_module, target_name) = self.resolved_proc_name(name);
                        let name = proc_name(target_module, &target_name);
                        self.line(out, &format!("{name}({});", lowered.join(", ")));
                        return Ok(());
                    }
                }
                let (code, _) = self.emit_expr(callee)?;
                self.line(out, &format!("{code};"));
                Ok(())
            }
        }
    }

    fn struct_equality(&self, id: ModeId, left: &str, right: &str) -> Result<String, String> {
        let mode = self.modes.get(id);
        let mut comparisons = Vec::new();
        for field in &mode.fields {
            let name = c_ident(&field.name);
            let left_field = format!("{left}.{name}");
            let right_field = format!("{right}.{name}");
            match kind_from_type(field.ty)? {
                Kind::Struct(inner) => {
                    comparisons.push(self.struct_equality(inner, &left_field, &right_field)?);
                }
                Kind::String => {
                    comparisons.push(format!("(strcmp({left_field}, {right_field}) == 0)"))
                }
                Kind::Array { .. } => comparisons.push(format!(
                    "(memcmp(&{left_field}, &{right_field}, sizeof({left_field})) == 0)"
                )),
                Kind::PowerSet(_) => {
                    comparisons.push(format!("(z_ps_equal({left_field}, {right_field}))"))
                }
                Kind::Text { .. } => comparisons.push(format!(
                    "(z_text_equal_text(&{left_field}, &{right_field}))"
                )),
                Kind::Association | Kind::Access { .. } => {
                    return Err(
                        "native backend: association/access fields cannot be compared yet"
                            .to_owned(),
                    )
                }
                Kind::DynamicArray { .. } => {
                    return Err(
                        "native backend: dynamic slices cannot be structure fields".to_owned()
                    )
                }
                Kind::Ref(_) | Kind::RefArray { .. } | Kind::Null => {
                    comparisons.push(format!("({left_field} == {right_field})"))
                }
                Kind::Int(_) | Kind::Float | Kind::Bool | Kind::Char => {
                    comparisons.push(format!("({left_field} == {right_field})"))
                }
            }
        }
        if comparisons.is_empty() {
            Ok("1".to_owned())
        } else {
            Ok(format!("({})", comparisons.join(" && ")))
        }
    }

    fn emit_field_location(&mut self, expr: &Expr) -> Result<(String, Kind), String> {
        match expr {
            Expr::Name { name, .. } => {
                let info = self
                    .vars
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("native backend: unknown variable `{name}`"))?;
                Ok((info.c_name, info.kind))
            }
            Expr::Apply { name, args, span } => {
                let Some(info) = self.vars.get(name).cloned() else {
                    return Err(format!("native backend: unknown array `{name}`"));
                };
                self.array_index_expr(&info, args, *span)
            }
            Expr::Deref { .. } => {
                let (code, kind) = self.emit_expr(expr)?;
                if !matches!(kind, Kind::Struct(_)) {
                    return Err(
                        "native backend: field access requires a structure value".to_owned()
                    );
                }
                Ok((code, kind))
            }
            Expr::Field { base, field, .. } => {
                let (base_code, base_kind) = self.emit_field_location(base)?;
                let Kind::Struct(id) = base_kind else {
                    return Err(
                        "native backend: field selection requires a structure value".to_owned()
                    );
                };
                let mode = self.modes.get(id);
                let Some(info) = mode.fields.iter().find(|info| &info.name == field) else {
                    return Err(format!(
                        "native backend: unknown field `{field}` in structure `{}`",
                        mode.name
                    ));
                };
                let kind = kind_from_type(info.ty)?;
                Ok((format!("{base_code}.{}", c_ident(field)), kind))
            }
            _ => Err("native backend: field access requires a simple structure value".to_owned()),
        }
    }

    fn emit_powerset_literal(
        &mut self,
        elements: &[crate::ast::ArrayTupleElement],
        member: types::PowerSetMember,
    ) -> Result<(String, Kind), String> {
        if elements.is_empty() {
            let value = self.temporary("z_ps_empty()", Kind::PowerSet(member))?;
            return Ok((value, Kind::PowerSet(member)));
        }
        let mut ranges = Vec::with_capacity(elements.len());
        for element in elements {
            if element.labels.is_some() {
                return Err(
                    "native backend: labelled powerset literals are not supported yet".to_owned(),
                );
            }
            let (low, _) = self.emit_expr(&element.value)?;
            let high = match &element.range_end {
                Some(end) => self.emit_expr(end)?.0,
                None => low.clone(),
            };
            ranges.push(format!("{{{low}, {high}}}"));
        }
        let value = self.temporary(
            &format!(
                "z_ps_literal({}, (ZPSRange[]){{{}}})",
                ranges.len(),
                ranges.join(", ")
            ),
            Kind::PowerSet(member),
        )?;
        Ok((value, Kind::PowerSet(member)))
    }

    fn emit_expr(&mut self, expr: &Expr) -> Result<(String, Kind), String> {
        match expr {
            Expr::Integer { value, .. } => {
                Ok((format!("INT64_C({value})"), Kind::Int(IntKind::Long)))
            }
            Expr::Float { value, .. } => Ok((c_float_literal(*value), Kind::Float)),
            Expr::Bool { value, .. } => {
                Ok(((if *value { "1" } else { "0" }).to_owned(), Kind::Bool))
            }
            Expr::Char { value, .. } => Ok(((*value as u32).to_string(), Kind::Char)),
            Expr::String { value, .. } => Ok((format!("\"{}\"", c_escape(value)), Kind::String)),
            Expr::Null { .. } => Ok(("NULL".to_owned(), Kind::Null)),
            Expr::DerefIndex {
                base,
                indices,
                span,
                ..
            } => self.deref_index_expr(base, indices, *span),
            Expr::MultiSlice { name, span, .. } => {
                let Some(ApplyResolution::MultiSlice { source: _, result }) =
                    self.check.resolutions.get(span)
                else {
                    return Err(
                        "native backend: multidimensional slice has no resolution".to_owned()
                    );
                };
                let Some(info) = self.vars.get(name).cloned() else {
                    return Err(format!("native backend: unknown array `{name}`"));
                };
                let Kind::Array {
                    element,
                    dims,
                    rank,
                    ..
                } = info.kind
                else {
                    return Err(format!("native backend: `{name}` is not an array"));
                };
                let result_dims = result.dimensions();
                if result_dims.len() != rank {
                    return Err("native backend: multidimensional slice rank mismatch".to_owned());
                }
                let mut source_strides = [1i128; 4];
                let mut result_strides = [1i128; 4];
                for index in (0..rank).rev() {
                    if index + 1 < rank {
                        source_strides[index] =
                            source_strides[index + 1] * dims[index + 1].length();
                        result_strides[index] =
                            result_strides[index + 1] * result_dims[index + 1].length();
                    }
                }
                let total: i128 = (0..rank).map(|index| result_dims[index].length()).product();
                let mut values = Vec::with_capacity(total.max(0) as usize);
                for flat in 0..total {
                    let mut offset = 0i128;
                    for index in 0..rank {
                        let local = if result_strides[index] == 0 {
                            0
                        } else {
                            (flat / result_strides[index]) % result_dims[index].length().max(1)
                        };
                        let source_index = result_dims[index].low + local;
                        offset += (source_index - dims[index].low) * source_strides[index];
                    }
                    values.push(format!("{}[{offset}]", info.c_name));
                }
                let mut result_kind_dims = [Dim { low: 0, high: 0 }; 4];
                for index in 0..rank {
                    result_kind_dims[index] = Dim {
                        low: result_dims[index].low,
                        high: result_dims[index].high,
                    };
                }
                let element_name = element.c_type();
                let code = if values.is_empty() {
                    format!("({element_name}[]){{0}}")
                } else {
                    format!("({element_name}[]){{{}}}", values.join(", "))
                };
                Ok((
                    code,
                    Kind::Array {
                        element,
                        dims: result_kind_dims,
                        rank,
                        total: total.max(0) as usize,
                    },
                ))
            }
            Expr::ArraySlice {
                name,
                first,
                last,
                up,
                span,
            } => {
                let Some(info) = self.vars.get(name).cloned() else {
                    return Err(format!("native backend: unknown array `{name}`"));
                };
                let Kind::Array {
                    element,
                    dims,
                    rank,
                    ..
                } = info.kind
                else {
                    return Err(format!("native backend: `{name}` is not an array"));
                };
                if rank != 1 {
                    return Err(
                        "native backend: multidimensional slices are not supported yet".to_owned(),
                    );
                }
                let low = dims[0].low;
                let high = dims[0].high;
                match self.check.types.get(span) {
                    Some(Type::Array(array)) => {
                        let Some(first) = crate::sema::constant_int(first) else {
                            return Err("native backend: constant slice bounds expected".to_owned());
                        };
                        let Some(last) = crate::sema::constant_int(last) else {
                            return Err("native backend: constant slice bounds expected".to_owned());
                        };
                        let upper = if *up {
                            first
                                .checked_add(last - 1)
                                .ok_or_else(|| "array slice bounds overflow".to_owned())?
                        } else {
                            last
                        };
                        let total = (upper - first + 1).max(0) as usize;
                        let start = first - low;
                        let mut new_dims = [Dim { low: 0, high: 0 }; 4];
                        new_dims[0] = Dim {
                            low: array.low,
                            high: array.high,
                        };
                        Ok((
                            format!("({} + {start})", info.c_name),
                            Kind::Array {
                                element,
                                dims: new_dims,
                                rank: 1,
                                total,
                            },
                        ))
                    }
                    Some(Type::DynamicArray {
                        low: result_low, ..
                    }) => {
                        let (first_code, _) = self.emit_expr(first)?;
                        let (last_code, _) = self.emit_expr(last)?;
                        let code = format!(
                            "z_slice_make((void *)({}), {}, {}, {}, {}, {}, sizeof({}))",
                            info.c_name,
                            low,
                            high,
                            first_code,
                            last_code,
                            if *up { 1 } else { 0 },
                            element.c_type()
                        );
                        Ok((
                            code,
                            Kind::DynamicArray {
                                element,
                                low: *result_low,
                            },
                        ))
                    }
                    _ => Err("native backend: slice has no array mode".to_owned()),
                }
            }
            Expr::ArrayTuple { elements, span } => {
                let Some(Type::PowerSet(set)) = self.check.types.get(span) else {
                    return Err(
                        "native backend: array literals are only supported in array assignments yet"
                            .to_owned(),
                    );
                };
                self.emit_powerset_literal(elements, set.member)
            }
            Expr::Field { .. } => self.emit_field_location(expr),
            Expr::AddressOf { target, .. } => match target.as_ref() {
                Expr::Name { name, .. } => {
                    let Some(info) = self.vars.get(name).cloned() else {
                        return Err(format!("native backend: unknown variable `{name}`"));
                    };
                    if let Kind::Array {
                        element,
                        dims,
                        rank,
                        total,
                    } = info.kind
                    {
                        return Ok((
                            info.c_name,
                            Kind::RefArray {
                                element,
                                dims,
                                rank,
                                total,
                            },
                        ));
                    }
                    let element = scalar_kind(info.kind).ok_or_else(|| {
                        "native backend: only scalar references are supported yet".to_owned()
                    })?;
                    Ok((format!("&{}", info.c_name), Kind::Ref(element)))
                }
                Expr::Apply { name, args, span } => {
                    let Some(info) = self.vars.get(name).cloned() else {
                        return Err(format!("native backend: unknown array `{name}`"));
                    };
                    let (code, kind) = self.array_index_expr(&info, args, *span)?;
                    let element = scalar_kind(kind).ok_or_else(|| {
                        "native backend: only scalar array-element references are supported yet"
                            .to_owned()
                    })?;
                    Ok((format!("&{code}"), Kind::Ref(element)))
                }
                Expr::Field { .. } => {
                    let (code, kind) = self.emit_field_location(target.as_ref())?;
                    let element = scalar_kind(kind).ok_or_else(|| {
                        "native backend: only scalar structure-field references are supported yet"
                            .to_owned()
                    })?;
                    Ok((format!("&{code}"), Kind::Ref(element)))
                }
                _ => Err(
                    "native backend: references to composite locations are not supported yet"
                        .to_owned(),
                ),
            },
            Expr::Deref { base, .. } => {
                let (code, kind) = self.emit_expr(base)?;
                match kind {
                    Kind::Ref(element) => Ok((
                        format!("Z_DEREF({}, {code})", element.c_type()),
                        element.kind(),
                    )),
                    Kind::RefArray {
                        element,
                        dims,
                        rank,
                        total,
                    } => Ok((
                        format!("({code})"),
                        Kind::Array {
                            element,
                            dims,
                            rank,
                            total,
                        },
                    )),
                    _ => Err("native backend: dereference requires a reference".to_owned()),
                }
            }
            Expr::Name { name, span } => {
                if let Some(ApplyResolution::SetElement { value, .. }) =
                    self.check.resolutions.get(span)
                {
                    return Ok((value.to_string(), Kind::Int(IntKind::Long)));
                }
                let info = self
                    .vars
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("native backend: unknown variable `{name}`"))?;
                if matches!(info.kind, Kind::Array { .. }) {
                    return Err(format!(
                        "native backend: array `{name}` cannot be used as a scalar value"
                    ));
                }
                Ok((info.c_name, info.kind))
            }
            Expr::Unary { op, operand, .. } => {
                let (code, kind) = self.emit_expr(operand)?;
                match op {
                    UnaryOp::Neg => match kind {
                        Kind::Int(int_kind) => {
                            let result = format!("z_neg({code})");
                            Ok((self.checked_int(&result, int_kind)?, Kind::Int(int_kind)))
                        }
                        Kind::Float => Ok((format!("(-({code}))"), Kind::Float)),
                        _ => Ok((format!("z_neg({code})"), Kind::Int(IntKind::Long))),
                    },
                    UnaryOp::Not => Ok((format!("!({code})"), Kind::Bool)),
                }
            }
            Expr::Binary {
                op, left, right, ..
            } => {
                if matches!(op, BinaryOp::Eq | BinaryOp::Ne)
                    && (matches!(
                        left.as_ref(),
                        Expr::ArraySlice { .. } | Expr::MultiSlice { .. }
                    ) || matches!(
                        right.as_ref(),
                        Expr::ArraySlice { .. } | Expr::MultiSlice { .. }
                    ))
                {
                    let (left_code, left_kind, right_code, right_kind) = if matches!(
                        left.as_ref(),
                        Expr::ArraySlice { .. } | Expr::MultiSlice { .. }
                    ) {
                        let (left_code, left_kind) = self.emit_expr(left)?;
                        let element = Self::array_element_of(left_kind)?;
                        let (right_code, right_kind) =
                            self.emit_array_value_expr(right, element)?;
                        (left_code, left_kind, right_code, right_kind)
                    } else {
                        let (right_code, right_kind) = self.emit_expr(right)?;
                        let element = Self::array_element_of(right_kind)?;
                        let (left_code, left_kind) = self.emit_array_value_expr(left, element)?;
                        (left_code, left_kind, right_code, right_kind)
                    };
                    let equality =
                        self.array_equality_code(&left_code, left_kind, &right_code, right_kind)?;
                    let code = if *op == BinaryOp::Eq {
                        equality
                    } else {
                        format!("!({equality})")
                    };
                    return Ok((code, Kind::Bool));
                }
                if matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
                    if let (
                        Expr::Name {
                            name: left_name, ..
                        },
                        Expr::Name {
                            name: right_name, ..
                        },
                    ) = (left.as_ref(), right.as_ref())
                    {
                        if let (Some(left_info), Some(right_info)) = (
                            self.vars.get(left_name).cloned(),
                            self.vars.get(right_name).cloned(),
                        ) {
                            if matches!(left_info.kind, Kind::Array { .. }) {
                                if left_info.kind != right_info.kind {
                                    return Err(
                                        "native backend: array comparison requires matching modes"
                                            .to_owned(),
                                    );
                                }
                                let equality = match left_info.kind {
                                    Kind::Array {
                                        element: ScalarKind::PowerSet(_),
                                        total,
                                        ..
                                    } => format!(
                                        "z_ps_array_equal({}, {}, {total})",
                                        left_info.c_name, right_info.c_name
                                    ),
                                    Kind::Array {
                                        element: ScalarKind::String,
                                        total,
                                        ..
                                    } => format!(
                                        "z_string_array_equal({}, {}, {total})",
                                        left_info.c_name, right_info.c_name
                                    ),
                                    _ => {
                                        let size = array_size_expression(left_info.kind)?;
                                        format!(
                                            "(memcmp({}, {}, {size}) == 0)",
                                            left_info.c_name, right_info.c_name
                                        )
                                    }
                                };
                                let code = if *op == BinaryOp::Eq {
                                    equality
                                } else {
                                    format!("!({equality})")
                                };
                                return Ok((code, Kind::Bool));
                            }
                        }
                    }
                }
                let (left, left_kind) = self.emit_expr(left)?;
                let (right, right_kind) = self.emit_expr(right)?;
                if matches!(
                    op,
                    BinaryOp::Eq
                        | BinaryOp::Ne
                        | BinaryOp::Lt
                        | BinaryOp::Le
                        | BinaryOp::Gt
                        | BinaryOp::Ge
                ) && !matches!(left_kind, Kind::Text { .. })
                    && !matches!(right_kind, Kind::Text { .. })
                    && (left_kind == Kind::String || right_kind == Kind::String)
                {
                    if left_kind != Kind::String || right_kind != Kind::String {
                        return Err(
                            "native backend: string comparison requires two strings".to_owned()
                        );
                    }
                    let comparison = match op {
                        BinaryOp::Eq => "== 0",
                        BinaryOp::Ne => "!= 0",
                        BinaryOp::Lt => "< 0",
                        BinaryOp::Le => "<= 0",
                        BinaryOp::Gt => "> 0",
                        BinaryOp::Ge => ">= 0",
                        _ => unreachable!(),
                    };
                    return Ok((
                        format!("(strcmp({left}, {right}) {comparison})"),
                        Kind::Bool,
                    ));
                }
                if matches!(op, BinaryOp::Eq | BinaryOp::Ne)
                    && (matches!(left_kind, Kind::Text { .. })
                        || matches!(right_kind, Kind::Text { .. }))
                {
                    let equality =
                        match (left_kind, right_kind) {
                            (Kind::Text { .. }, Kind::Text { .. }) => {
                                format!("z_text_equal_text(&{left}, &{right})")
                            }
                            (Kind::Text { .. }, Kind::String) => {
                                format!("z_text_equal_string(&{left}, {right})")
                            }
                            (Kind::String, Kind::Text { .. }) => {
                                format!("z_text_equal_string(&{right}, {left})")
                            }
                            _ => return Err(
                                "native backend: TEXT comparison requires TEXT or CHARS operands"
                                    .to_owned(),
                            ),
                        };
                    let code = if *op == BinaryOp::Eq {
                        equality
                    } else {
                        format!("!({equality})")
                    };
                    return Ok((code, Kind::Bool));
                }
                if matches!(op, BinaryOp::Eq | BinaryOp::Ne) && matches!(left_kind, Kind::Struct(_))
                {
                    if left_kind != right_kind {
                        return Err(
                            "native backend: structure comparison requires matching modes"
                                .to_owned(),
                        );
                    }
                    let Kind::Struct(id) = left_kind else {
                        unreachable!();
                    };
                    let equality = self.struct_equality(id, &left, &right)?;
                    if *op == BinaryOp::Eq {
                        return Ok((equality, Kind::Bool));
                    }
                    return Ok((format!("!({equality})"), Kind::Bool));
                }
                if *op == BinaryOp::In {
                    if !matches!(right_kind, Kind::PowerSet(_)) {
                        return Err(
                            "native backend: the right operand of IN must be a powerset".to_owned()
                        );
                    }
                    return Ok((format!("z_ps_contains({right}, {left})"), Kind::Bool));
                }
                if matches!(op, BinaryOp::Eq | BinaryOp::Ne)
                    && matches!(left_kind, Kind::PowerSet(_))
                {
                    if !matches!(right_kind, Kind::PowerSet(_)) {
                        return Err(
                            "native backend: powerset comparison requires matching modes"
                                .to_owned(),
                        );
                    }
                    let equality = format!("z_ps_equal({left}, {right})");
                    let code = if *op == BinaryOp::Eq {
                        equality
                    } else {
                        format!("!({equality})")
                    };
                    return Ok((code, Kind::Bool));
                }
                if matches!(
                    op,
                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
                ) && matches!(left_kind, Kind::PowerSet(_))
                {
                    if !matches!(right_kind, Kind::PowerSet(_)) {
                        return Err(
                            "native backend: powerset comparison requires matching modes"
                                .to_owned(),
                        );
                    }
                    let function = match op {
                        BinaryOp::Lt => "z_ps_lt",
                        BinaryOp::Le => "z_ps_le",
                        BinaryOp::Gt => "z_ps_gt",
                        BinaryOp::Ge => "z_ps_ge",
                        _ => unreachable!(),
                    };
                    return Ok((format!("{function}({left}, {right})"), Kind::Bool));
                }
                if matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul)
                    && matches!(left_kind, Kind::PowerSet(_))
                {
                    if !matches!(right_kind, Kind::PowerSet(_)) {
                        return Err(
                            "native backend: powerset operation requires matching modes".to_owned()
                        );
                    }
                    let function = match op {
                        BinaryOp::Add => "z_ps_union",
                        BinaryOp::Sub => "z_ps_difference",
                        BinaryOp::Mul => "z_ps_intersection",
                        _ => unreachable!(),
                    };
                    let value =
                        self.temporary(&format!("{function}({left}, {right})"), left_kind)?;
                    return Ok((value, left_kind));
                }
                if matches!(
                    op,
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div
                ) && matches!(left_kind, Kind::Int(_))
                    && matches!(right_kind, Kind::Int(_))
                {
                    let function = match op {
                        BinaryOp::Add => "z_add",
                        BinaryOp::Sub => "z_sub",
                        BinaryOp::Mul => "z_mul",
                        BinaryOp::Div => "z_div",
                        _ => unreachable!(),
                    };
                    let kind =
                        Self::binary_int_kind(left_kind, right_kind).unwrap_or(IntKind::Long);
                    let result = format!("{function}({left}, {right})");
                    return Ok((self.checked_int(&result, kind)?, Kind::Int(kind)));
                }
                if matches!(
                    op,
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div
                ) && left_kind == Kind::Float
                    && right_kind == Kind::Float
                {
                    let result = match op {
                        BinaryOp::Add => format!("(({left}) + ({right}))"),
                        BinaryOp::Sub => format!("(({left}) - ({right}))"),
                        BinaryOp::Mul => format!("(({left}) * ({right}))"),
                        BinaryOp::Div => format!("z_fdiv({left}, {right})"),
                        _ => unreachable!(),
                    };
                    return Ok((result, Kind::Float));
                }
                if matches!(op, BinaryOp::Mod | BinaryOp::Rem) {
                    let kind =
                        Self::binary_int_kind(left_kind, right_kind).unwrap_or(IntKind::Long);
                    let result = format!("z_mod({left}, {right})");
                    return Ok((self.checked_int(&result, kind)?, Kind::Int(kind)));
                }
                if *op == BinaryOp::Xor {
                    let kind =
                        Self::binary_int_kind(left_kind, right_kind).unwrap_or(IntKind::Long);
                    let result = format!("(({left}) ^ ({right}))");
                    return Ok((self.checked_int(&result, kind)?, Kind::Int(kind)));
                }
                if *op == BinaryOp::Concat {
                    if left_kind != Kind::String || right_kind != Kind::String {
                        return Err("native backend: `//` requires character strings".to_owned());
                    }
                    let value =
                        self.temporary(&format!("z_concat({left}, {right})"), Kind::String)?;
                    return Ok((value, Kind::String));
                }
                let (operator, kind) = match op {
                    BinaryOp::Or | BinaryOp::OrIf => ("||", Kind::Bool),
                    BinaryOp::And | BinaryOp::AndIf => ("&&", Kind::Bool),
                    BinaryOp::Xor => unreachable!(),
                    BinaryOp::Eq => ("==", Kind::Bool),
                    BinaryOp::Ne => ("!=", Kind::Bool),
                    BinaryOp::Lt => ("<", Kind::Bool),
                    BinaryOp::Le => ("<=", Kind::Bool),
                    BinaryOp::Gt => (">", Kind::Bool),
                    BinaryOp::Ge => (">=", Kind::Bool),
                    BinaryOp::Add => ("+", Kind::Int(IntKind::Long)),
                    BinaryOp::Sub => ("-", Kind::Int(IntKind::Long)),
                    BinaryOp::Mul => ("*", Kind::Int(IntKind::Long)),
                    BinaryOp::Div | BinaryOp::Mod | BinaryOp::Rem => unreachable!(),
                    BinaryOp::Concat => unreachable!(),
                    BinaryOp::Pow | BinaryOp::In => {
                        return Err(
                            "native backend: this binary operator is not supported yet".to_owned()
                        )
                    }
                };
                Ok((format!("(({left}) {operator} ({right}))"), kind))
            }
            Expr::Apply { name, args, span } => {
                match self.check.resolutions.get(span) {
                    Some(ApplyResolution::ArrayIndex(Type::Array(_)))
                    | Some(ApplyResolution::ArrayIndex(Type::MultiArray(_))) => {
                        let Some(info) = self.vars.get(name).cloned() else {
                            return Err(format!("native backend: unknown array `{name}`"));
                        };
                        self.array_index_expr(&info, args, *span)
                    }
                    Some(ApplyResolution::TextSub {
                        op: crate::builtins::TextSubOp::GetRecord,
                    }) => {
                        let Expr::Name { name, .. } = &args[0] else {
                            return Err(
                                "native backend: GETTEXTRECORD requires a TEXT variable".to_owned()
                            );
                        };
                        let info = self.vars.get(name).cloned().ok_or_else(|| {
                            format!("native backend: unknown text variable `{name}`")
                        })?;
                        if !matches!(info.kind, Kind::Text { .. }) {
                            return Err("native backend: GETTEXTRECORD requires TEXT".to_owned());
                        }
                        Ok((
                            format!("z_text_get_record(&{})", info.c_name),
                            Kind::Ref(ScalarKind::String),
                        ))
                    }
                    Some(ApplyResolution::TextSub {
                        op: crate::builtins::TextSubOp::GetAccess,
                    }) => {
                        let Expr::Name { name, .. } = &args[0] else {
                            return Err(
                                "native backend: GETTEXTACCESS requires a TEXT variable".to_owned()
                            );
                        };
                        let info = self.vars.get(name).cloned().ok_or_else(|| {
                            format!("native backend: unknown text variable `{name}`")
                        })?;
                        if !matches!(info.kind, Kind::Text { .. }) {
                            return Err("native backend: GETTEXTACCESS requires TEXT".to_owned());
                        }
                        Ok((
                            format!("z_text_get_access(&{})", info.c_name),
                            Kind::Ref(ScalarKind::Access {
                                indexed: false,
                                low: 0,
                                high: 0,
                            }),
                        ))
                    }
                    Some(ApplyResolution::Association {
                        op, index, target, ..
                    }) => {
                        let op = *op;
                        let has_index = *index;
                        if *target {
                            return Err(
                                "native backend: this association operation produces no value"
                                    .to_owned(),
                            );
                        }
                        self.emit_association_expr(op, has_index, args)
                    }
                    Some(ApplyResolution::TextIndex { get: true }) => {
                        if args.len() != 1 {
                            return Err("native backend: malformed GETTEXTINDEX call".to_owned());
                        }
                        let (target, _) = self.text_location(&args[0])?;
                        Ok((
                            format!("z_text_get_index({target})"),
                            Kind::Int(IntKind::Int),
                        ))
                    }
                    Some(ApplyResolution::TextIndex { get: false }) => {
                        Err("native backend: SETTEXTINDEX cannot be used as a value".to_owned())
                    }
                    Some(ApplyResolution::Builtin(crate::builtins::Builtin::ReadLine)) => {
                        let value = self.temporary("z_readline()", Kind::String)?;
                        Ok((value, Kind::String))
                    }
                    Some(ApplyResolution::FloatConversion) => {
                        if args.len() != 1 {
                            return Err("native backend: malformed FLOAT conversion".to_owned());
                        }
                        let (code, _) = self.emit_expr(&args[0])?;
                        Ok((format!("((double)({code}))"), Kind::Float))
                    }
                    Some(ApplyResolution::Procedure(_)) => {
                        let (code, kind) = self.emit_procedure_call(name, args, *span)?;
                        Ok((code, kind))
                    }
                    Some(ApplyResolution::Conversion(kind)) => {
                        if args.len() != 1 {
                            return Err("native backend: malformed conversion call".to_owned());
                        }
                        let (code, _) = self.emit_expr(&args[0])?;
                        Ok((format!("((int64_t)({code}))"), Kind::Int(*kind)))
                    }
                    _ => Err(format!(
                        "native backend: call to `{name}` is not supported in native code yet"
                    )),
                }
            }
            _ => Err(
                "native backend: this expression is not supported in native code yet".to_owned(),
            ),
        }
    }

    fn lower_call_args(
        &mut self,
        args: &[Expr],
        signature: &types::ProcType,
    ) -> Result<Vec<String>, String> {
        let mut lowered = Vec::with_capacity(args.len());
        for ((arg, passing), ty) in args.iter().zip(&signature.passing).zip(&signature.params) {
            let kind = kind_from_type(*ty)?;
            let code = if matches!(kind, Kind::Array { .. }) {
                if !matches!(passing, Passing::In) && matches!(arg, Expr::ArrayTuple { .. }) {
                    return Err(
                        "native backend: array literals cannot be passed to OUT, INOUT, or LOC parameters"
                            .to_owned(),
                    );
                }
                self.emit_array_argument(arg, kind)?
            } else {
                match passing {
                    Passing::In => self.emit_expr(arg)?.0,
                    Passing::Out | Passing::InOut | Passing::Loc => {
                        self.emit_location_address(arg)?
                    }
                }
            };
            lowered.push(code);
        }
        Ok(lowered)
    }

    fn emit_array_argument(&mut self, argument: &Expr, expected: Kind) -> Result<String, String> {
        match argument {
            Expr::Name { name, .. } => {
                let Some(info) = self.vars.get(name).cloned() else {
                    return Err(format!("native backend: unknown array `{name}`"));
                };
                if info.kind != expected {
                    return Err("native backend: array argument mode mismatch".to_owned());
                }
                Ok(info.c_name)
            }
            Expr::Apply { .. } | Expr::Deref { .. } | Expr::MultiSlice { .. } => {
                let (code, kind) = self.emit_expr(argument)?;
                if kind != expected {
                    return Err("native backend: array argument mode mismatch".to_owned());
                }
                Ok(code)
            }
            Expr::ArraySlice { .. } => {
                let Kind::Array {
                    element, total, ..
                } = expected
                else {
                    return Err("native backend: expected an array parameter mode".to_owned());
                };
                let (code, kind) = self.emit_expr(argument)?;
                match kind {
                    Kind::Array {
                        element: value_element,
                        total: value_total,
                        ..
                    } => {
                        if value_element != element || value_total != total {
                            return Err(
                                "native backend: array argument mode mismatch".to_owned()
                            );
                        }
                        Ok(code)
                    }
                    Kind::DynamicArray {
                        element: value_element,
                        ..
                    } => {
                        if value_element != element {
                            return Err(
                                "native backend: array argument mode mismatch".to_owned()
                            );
                        }
                        Ok(format!(
                            "(({} *)z_slice_check_len({code}, {total}))",
                            element.c_type()
                        ))
                    }
                    _ => Err("native backend: array slice expected".to_owned()),
                }
            }
            Expr::ArrayTuple { elements, .. } => {
                let Kind::Array {
                    element,
                    dims,
                    rank,
                    total,
                    ..
                } = expected
                else {
                    return Err("native backend: expected an array parameter mode".to_owned());
                };
                let mut values = Vec::new();
                self.collect_array_literal(elements, &dims, rank, 0, &mut values)?;
                if values.len() != total {
                    return Err(format!(
                        "native backend: array literal has {} element(s), expected {total}",
                        values.len()
                    ));
                }
                let values = if let ScalarKind::Int(kind) = element {
                    values
                        .into_iter()
                        .map(|code| self.checked_int(&code, kind))
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    values
                };
                let element = element.c_type();
                if values.is_empty() {
                    return Ok(format!("({element}[]){{0}}"));
                }
                Ok(format!("({element}[]){{{}}}", values.join(", ")))
            }
            _ => Err(
                "native backend: array arguments require an array variable, array result, or array literal"
                    .to_owned(),
            ),
        }
    }

    fn resolved_proc_name(&self, name: &str) -> (usize, String) {
        if let Some(symbol) = self
            .check
            .imports
            .get(&(self.module_index, name.to_owned()))
        {
            if matches!(symbol.kind, ImportedKind::Proc) {
                return (symbol.module, symbol.name.clone());
            }
        }
        (self.module_index, name.to_owned())
    }

    fn emit_procedure_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: crate::lexer::Span,
    ) -> Result<(String, Kind), String> {
        let Some(ApplyResolution::Procedure(signature)) = self.check.resolutions.get(&span) else {
            return Err("native backend: procedure call has no resolved signature".to_owned());
        };
        if signature.passing.len() != args.len() || signature.params.len() != args.len() {
            return Err("native backend: procedure argument count mismatch".to_owned());
        }
        let lowered = self.lower_call_args(args, signature)?;
        let (target_module, target_name) = self.resolved_proc_name(name);
        let name = proc_name(target_module, &target_name);
        let call = format!("{name}({})", lowered.join(", "));
        match signature.result {
            Some(ty) => {
                let kind = kind_from_type(ty)?;
                if matches!(kind, Kind::Array { .. }) {
                    let unique = self.temp_counter;
                    self.temp_counter += 1;
                    let wrapper = format!("__own_ret_{unique}");
                    self.prelude.push(format!(
                        "z_ret_{name} {wrapper} = {call};"
                    ));
                    let data = format!("{wrapper}.items");
                    let push = self.cleanup_push_line(&data, kind)?;
                    if !push.is_empty() {
                        self.prelude.push(push);
                    }
                    self.register_temporary(data.clone(), kind);
                    Ok((data, kind))
                } else if self.kind_owns(kind) {
                    let value = self.temporary(&call, kind)?;
                    Ok((value, kind))
                } else {
                    Ok((call, kind))
                }
            }
            None => Err(format!(
                "native backend: procedure `{name}` does not return a value but is used in an expression"
            )),
        }
    }

    fn emit_location_address(&mut self, location: &Expr) -> Result<String, String> {
        match location {
            Expr::Name { name, .. } => {
                let Some(info) = self.vars.get(name).cloned() else {
                    return Err(format!("native backend: unknown variable `{name}`"));
                };
                if matches!(info.kind, Kind::Array { .. }) {
                    return Ok(info.c_name);
                }
                Ok(format!("&{}", info.c_name))
            }
            Expr::Apply { name, args, span } => {
                let Some(info) = self.vars.get(name).cloned() else {
                    return Err(format!("native backend: unknown array `{name}`"));
                };
                let (code, _) = self.array_index_expr(&info, args, *span)?;
                Ok(format!("&{code}"))
            }
            _ => Err("native backend: OUT/INOUT arguments must be simple locations".to_owned()),
        }
    }
}

fn text_conversion_supported(
    _conversion: &crate::builtins::Conversion,
    _kind: TextIo,
) -> Result<(), String> {
    Ok(())
}

fn expected_total(kind: Kind) -> usize {
    match kind {
        Kind::Array { total, .. } => total,
        _ => 0,
    }
}

fn array_size_expression(kind: Kind) -> Result<String, String> {
    let Kind::Array { element, total, .. } = kind else {
        return Err("native backend: expected an array mode".to_owned());
    };
    Ok(format!("((size_t){total} * sizeof({}))", element.c_type()))
}

fn array_scalar_kind(ty: Type) -> Result<ScalarKind, String> {
    match ty {
        Type::Int(kind) => Ok(ScalarKind::Int(kind)),
        Type::IntLiteral | Type::Set(_) => Ok(ScalarKind::Int(IntKind::Long)),
        Type::Float => Ok(ScalarKind::Float),
        Type::Bool => Ok(ScalarKind::Bool),
        Type::Char | Type::WChar => Ok(ScalarKind::Char),
        Type::String | Type::BoundedString { .. } | Type::WString | Type::WBoundedString { .. } => {
            Ok(ScalarKind::String)
        }
        Type::Struct(id) => Ok(ScalarKind::Struct(id)),
        Type::PowerSet(set) => Ok(ScalarKind::PowerSet(set.member)),
        Type::Association => Ok(ScalarKind::Association),
        Type::Access(access) => Ok(ScalarKind::Access {
            indexed: access.indexed,
            low: access.index.as_ref().map(|index| index.low).unwrap_or(0),
            high: access.index.as_ref().map(|index| index.high).unwrap_or(0),
        }),
        Type::Text {
            length,
            wide: false,
            indexed,
            index,
            ..
        } => Ok(ScalarKind::Text {
            length,
            indexed,
            low: index.map(|index| index.low).unwrap_or(0),
            high: index.map(|index| index.high).unwrap_or(0),
        }),
        other => Err(format!(
            "native backend: array element mode `{}` is not supported yet",
            other.describe()
        )),
    }
}

fn scalar_kind(kind: Kind) -> Option<ScalarKind> {
    match kind {
        Kind::Int(kind) => Some(ScalarKind::Int(kind)),
        Kind::Float => Some(ScalarKind::Float),
        Kind::Bool => Some(ScalarKind::Bool),
        Kind::Char => Some(ScalarKind::Char),
        Kind::String => Some(ScalarKind::String),
        Kind::Struct(id) => Some(ScalarKind::Struct(id)),
        Kind::PowerSet(member) => Some(ScalarKind::PowerSet(member)),
        Kind::Association => Some(ScalarKind::Association),
        Kind::Access { indexed, low, high } => Some(ScalarKind::Access { indexed, low, high }),
        _ => None,
    }
}

fn scalar_ref_target(target: types::RefTarget) -> Result<ScalarKind, String> {
    use types::RefTarget;
    match target {
        RefTarget::Int(kind) => Ok(ScalarKind::Int(kind)),
        RefTarget::Float => Ok(ScalarKind::Float),
        RefTarget::Bool => Ok(ScalarKind::Bool),
        RefTarget::Struct(id) => Ok(ScalarKind::Struct(id)),
        RefTarget::PowerSet(set) => Ok(ScalarKind::PowerSet(set.member)),
        RefTarget::Association => Ok(ScalarKind::Association),
        RefTarget::Access(access) => Ok(ScalarKind::Access {
            indexed: access.indexed,
            low: access.index.as_ref().map(|index| index.low).unwrap_or(0),
            high: access.index.as_ref().map(|index| index.high).unwrap_or(0),
        }),
        RefTarget::Char | RefTarget::WChar => Ok(ScalarKind::Char),
        RefTarget::String
        | RefTarget::BoundedString { .. }
        | RefTarget::WString
        | RefTarget::WBoundedString { .. } => Ok(ScalarKind::String),
        other => Err(format!(
            "native backend: reference target `{}` is not supported yet",
            other.describe()
        )),
    }
}

fn kind_from_type(ty: Type) -> Result<Kind, String> {
    match ty {
        Type::Int(kind) => Ok(Kind::Int(kind)),
        Type::IntLiteral => Ok(Kind::Int(IntKind::Long)),
        Type::Float => Ok(Kind::Float),
        Type::Bool => Ok(Kind::Bool),
        Type::Set(_) => Ok(Kind::Int(IntKind::Long)),
        Type::Struct(id) => Ok(Kind::Struct(id)),
        Type::PowerSet(set) => Ok(Kind::PowerSet(set.member)),
        Type::Association => Ok(Kind::Association),
        Type::Access(access) => Ok(Kind::Access {
            indexed: access.indexed,
            low: access.index.as_ref().map(|index| index.low).unwrap_or(0),
            high: access.index.as_ref().map(|index| index.high).unwrap_or(0),
        }),
        Type::Text {
            length,
            wide: false,
            indexed,
            index,
            ..
        } => Ok(Kind::Text {
            length,
            indexed,
            low: index.map(|index| index.low).unwrap_or(0),
            high: index.map(|index| index.high).unwrap_or(0),
        }),
        Type::Text { wide: true, .. } => {
            Err("native backend: wide text modes are not supported yet".to_owned())
        }
        Type::Char | Type::WChar => Ok(Kind::Char),
        Type::String | Type::BoundedString { .. } | Type::WString | Type::WBoundedString { .. } => {
            Ok(Kind::String)
        }
        Type::Array(array) => {
            let element = match array.element.ty() {
                Type::Int(kind) => ScalarKind::Int(kind),
                Type::IntLiteral => ScalarKind::Int(IntKind::Long),
                Type::Float => ScalarKind::Float,
                Type::Bool => ScalarKind::Bool,
                Type::Set(_) => ScalarKind::Int(IntKind::Long),
                Type::Struct(id) => ScalarKind::Struct(id),
                Type::PowerSet(set) => ScalarKind::PowerSet(set.member),
                Type::Association => ScalarKind::Association,
                Type::Access(access) => ScalarKind::Access {
                    indexed: access.indexed,
                    low: access.index.as_ref().map(|index| index.low).unwrap_or(0),
                    high: access.index.as_ref().map(|index| index.high).unwrap_or(0),
                },
                Type::Text {
                    length,
                    wide: false,
                    indexed,
                    index,
                    ..
                } => ScalarKind::Text {
                    length,
                    indexed,
                    low: index.map(|index| index.low).unwrap_or(0),
                    high: index.map(|index| index.high).unwrap_or(0),
                },
                Type::Char => ScalarKind::Char,
                Type::String
                | Type::BoundedString { .. }
                | Type::WString
                | Type::WBoundedString { .. } => ScalarKind::String,
                other => {
                    return Err(format!(
                        "native backend: array element mode `{}` is not supported yet",
                        other.describe()
                    ))
                }
            };
            let mut dims = [Dim { low: 0, high: 0 }; 4];
            dims[0] = Dim {
                low: array.low,
                high: array.high,
            };
            Ok(Kind::Array {
                element,
                dims,
                rank: 1,
                total: (array.high - array.low + 1).max(0) as usize,
            })
        }
        Type::Ref(target) => match target {
            types::RefTarget::Array(array) => {
                let element = array_scalar_kind(array.element.ty())?;
                let mut dims = [Dim { low: 0, high: 0 }; 4];
                dims[0] = Dim {
                    low: array.low,
                    high: array.high,
                };
                Ok(Kind::RefArray {
                    element,
                    dims,
                    rank: 1,
                    total: (array.high - array.low + 1).max(0) as usize,
                })
            }
            types::RefTarget::MultiArray(array) => {
                let element = array_scalar_kind(array.element.ty())?;
                let mut dims = [Dim { low: 0, high: 0 }; 4];
                for (index, dimension) in array.dimensions().iter().enumerate() {
                    dims[index] = Dim {
                        low: dimension.low,
                        high: dimension.high,
                    };
                }
                Ok(Kind::RefArray {
                    element,
                    dims,
                    rank: array.rank(),
                    total: array.total_len().max(0) as usize,
                })
            }
            _ => {
                let element = scalar_ref_target(target)?;
                Ok(Kind::Ref(element))
            }
        },
        Type::MultiArray(array) => {
            let element = match array.element.ty() {
                Type::Int(kind) => ScalarKind::Int(kind),
                Type::IntLiteral => ScalarKind::Int(IntKind::Long),
                Type::Float => ScalarKind::Float,
                Type::Bool => ScalarKind::Bool,
                Type::Set(_) => ScalarKind::Int(IntKind::Long),
                Type::Struct(id) => ScalarKind::Struct(id),
                Type::PowerSet(set) => ScalarKind::PowerSet(set.member),
                Type::Association => ScalarKind::Association,
                Type::Access(access) => ScalarKind::Access {
                    indexed: access.indexed,
                    low: access.index.as_ref().map(|index| index.low).unwrap_or(0),
                    high: access.index.as_ref().map(|index| index.high).unwrap_or(0),
                },
                Type::Text {
                    length,
                    wide: false,
                    indexed,
                    index,
                    ..
                } => ScalarKind::Text {
                    length,
                    indexed,
                    low: index.map(|index| index.low).unwrap_or(0),
                    high: index.map(|index| index.high).unwrap_or(0),
                },
                Type::Char => ScalarKind::Char,
                Type::String
                | Type::BoundedString { .. }
                | Type::WString
                | Type::WBoundedString { .. } => ScalarKind::String,
                other => {
                    return Err(format!(
                        "native backend: array element mode `{}` is not supported yet",
                        other.describe()
                    ))
                }
            };
            let mut dims = [Dim { low: 0, high: 0 }; 4];
            for (index, dimension) in array.dimensions().iter().enumerate() {
                dims[index] = Dim {
                    low: dimension.low,
                    high: dimension.high,
                };
            }
            Ok(Kind::Array {
                element,
                dims,
                rank: array.rank(),
                total: array.total_len().max(0) as usize,
            })
        }
        _ => Err(format!(
            "native backend: mode `{}` is not supported yet",
            ty.describe()
        )),
    }
}

fn mode_kind(
    mode: &ModeRef,
    module_modes: &[HashMap<String, Type>],
    module: usize,
) -> Result<Kind, String> {
    let mode_names = module_modes.get(module).cloned().unwrap_or_default();
    let ty = types::resolve_mode_ref(mode, &mode_names)
        .ok_or_else(|| format!("native backend: unsupported mode `{}`", mode.name))?;
    kind_from_type(ty)
}

fn case_labels_condition(
    labels: &[CaseLabel],
    selector: &str,
    explicit: Option<&str>,
) -> Vec<String> {
    let mut conditions = Vec::new();
    for label in labels {
        let condition = match label {
            CaseLabel::Value(value) => Some(format!("{selector} == {value}")),
            CaseLabel::Bool(value) => Some(format!("{selector} == {}", if *value { 1 } else { 0 })),
            CaseLabel::Char(value) => Some(format!("{selector} == {}", *value as u32)),
            CaseLabel::Range { low, high } => {
                Some(format!("({selector} >= {low} && {selector} <= {high})"))
            }
            CaseLabel::CharRange { low, high } => Some(format!(
                "({selector} >= {} && {selector} <= {})",
                *low as u32, *high as u32
            )),
            CaseLabel::Else => explicit.map(|explicit| format!("!{explicit}")),
            CaseLabel::Any => Some("1".to_owned()),
        };
        if let Some(condition) = condition {
            conditions.push(condition);
        }
    }
    conditions
}

fn c_ident(name: &str) -> String {
    let mut ident = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            ident.push(ch);
        } else {
            ident.push('_');
        }
    }
    if ident.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        ident.insert(0, '_');
    }
    ident
}

fn c_float_literal(value: f64) -> String {
    if value.is_nan() {
        return "NAN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "(-INFINITY)".to_owned()
        } else {
            "INFINITY".to_owned()
        };
    }
    let text = format!("{value:?}");
    if text.contains('.') || text.contains('e') || text.contains('E') {
        text
    } else {
        format!("{text}.0")
    }
}

fn c_escape(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if (ch as u32) < 0x20 => escaped.push_str(&format!("\\x{:02x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }
    escaped
}
