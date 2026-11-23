use crate::ast::*;
use crate::ir;
use crate::lexer::{SourceId, Span};
use crate::manifest::WidthProfile;
use crate::program::{self, ExportKind};
use crate::sema::{self, CheckResult};
use crate::types::{self, IntKind, Type};
use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, Yielder};
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
    pub exception: Option<String>,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        let exception = if message == "ASSERTFAIL" {
            Some("ASSERTFAIL".to_owned())
        } else if message.starts_with("OVERFLOW") {
            Some("OVERFLOW".to_owned())
        } else {
            None
        };
        Self {
            code: "E0500",
            span,
            message,
            exception,
        }
    }

    pub fn coded(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
            exception: None,
        }
    }

    pub fn coded_exception(
        code: &'static str,
        name: &str,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            span,
            message: message.into(),
            exception: Some(name.to_owned()),
        }
    }

    pub fn exception(name: &str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "E0510",
            span,
            message: message.into(),
            exception: Some(name.to_owned()),
        }
    }

    pub fn raised(name: &str, span: Span) -> Self {
        Self {
            code: "E0509",
            span,
            message: format!("unhandled exception `{name}`"),
            exception: Some(name.to_owned()),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub fn run(program: &Program, profile: WidthProfile) -> Result<(), Error> {
    let check = sema::analyze(program, profile);
    if let Some(diagnostic) = check.diagnostics.first() {
        let span = diagnostic.span.unwrap_or_else(|| {
            program
                .modules
                .first()
                .map(|module| module.span)
                .unwrap_or(Span::new(SourceId(0), 0, 0))
        });
        return Err(Error::coded(
            diagnostic.code,
            span,
            diagnostic.message.clone(),
        ));
    }
    run_checked(program, profile, &check, &[])
}

pub fn run_checked(
    program: &Program,
    profile: WidthProfile,
    check: &CheckResult,
    external: &[program::ExternalInterface],
) -> Result<(), Error> {
    let program = program::normalize(program)
        .map_err(|error| Error::coded(error.code, error.span, error.message))?;
    let info = program::analyze(&program)
        .map_err(|error| Error::coded(error.code, error.span, error.message))?;
    let module_modes: Vec<HashMap<String, Type>> = info
        .interfaces
        .iter()
        .map(|interface| interface.modes.clone())
        .collect();
    let error_span = program
        .modules
        .first()
        .map(|module| module.span)
        .unwrap_or_else(|| Span::new(SourceId(0), 0, 0));
    let mut modes = info.modes;
    let external_modes = program::add_external_modes(&mut modes, external)
        .map_err(|message| Error::coded("E0600", error_span, message))?;
    let mut exports = info.exports;
    program::add_external_exports(&mut exports, external, &external_modes)
        .map_err(|message| Error::coded("E0600", error_span, message))?;
    let modes = Rc::new(modes);
    // A module may use a seized structure mode in its declarations, so resolve
    // mode seizes before any variable is declared.
    let mut module_modes = module_modes;
    for (index, module) in program.modules.iter().enumerate() {
        if module.kind.is_spec() {
            continue;
        }
        let package_name = module.package_name.as_deref();
        let module_name = module.name.as_deref();
        for declaration in &module.declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Seize {
                continue;
            }
            if visibility.all {
                for export in exports
                    .iter()
                    .filter(|export| export.module != index)
                    .filter(|export| export.is_accessible_to(package_name, module_name))
                {
                    if let ExportKind::Mode(ty) = export.kind {
                        module_modes[index].insert(export.name.clone(), ty);
                    }
                }
            } else {
                for name in &visibility.names {
                    let Ok(export) =
                        program::resolve_export(&exports, name, package_name, module_name)
                    else {
                        continue;
                    };
                    if let ExportKind::Mode(ty) = export.kind {
                        module_modes[index].insert(name.name.clone(), ty);
                    }
                }
            }
        }
    }

    let lowered = ir::lower(&program, check)
        .map_err(|error| Error::coded("E0400", error.span, error.message))?;
    let mut moreta_decls = HashMap::new();
    let mut moreta_invariants = HashMap::new();
    for (id, source) in info.moreta_sources {
        moreta_decls.insert(id, source.declarations);
        if let Some(invariant) = source.invariant {
            moreta_invariants.insert(id, invariant);
        }
    }
    let yield_interval = std::env::var("Z200C_YIELD_INTERVAL")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    let scheduler = Rc::new(RefCell::new(ScheduleState::default()));
    let mut interpreter = Interpreter {
        profile,
        modules: Vec::new(),
        locals: Vec::new(),
        call_bases: Vec::new(),
        active_module: 0,
        active_scope_key: None,
        module_floor: 0,
        depth: 0,
        yield_interval,
        yield_counter: 0,
        exports,
        modes,
        module_modes,
        reaches: info.reaches,
        modules_by_span: collect_modules_by_span(&program),
        proc_defs: collect_proc_defs(&program),
        process_defs: collect_process_defs(&program),
        regions: program
            .modules
            .iter()
            .enumerate()
            .filter(|(_, module)| module.kind.is_region())
            .map(|(index, _)| index)
            .collect(),
        process_id: usize::MAX,
        process_name: String::new(),
        region_depths: HashMap::new(),
        object_depths: HashMap::new(),
        deadline: None,
        scheduler: Rc::clone(&scheduler),
        moreta_decls,
        moreta_invariants,
        package_names: program
            .modules
            .iter()
            .map(|module| module.package_name.clone())
            .collect(),
        nested_scopes: HashMap::new(),
        exprs: lowered.exprs,
        bodies: lowered.bodies,
        refs: Vec::new(),
        ref_allocated: Vec::new(),
        self_cells: Vec::new(),
        self_modes: Vec::new(),
    };

    interpreter.declare_modules(&program)?;
    interpreter.apply_seizes(&program)?;

    for (index, module) in program.modules.iter().enumerate() {
        if module.kind.is_spec() {
            continue;
        }
        interpreter.active_module = index;
        interpreter.initialize_declarations(&module.declarations)?;
        interpreter.run_module(module)?;
    }

    drop(interpreter);
    loop {
        expire_timed_waiters(&scheduler);
        if scheduler.borrow().ready.is_empty() {
            if let Some(deadline) = next_timed_deadline(&scheduler) {
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                }
                expire_timed_waiters(&scheduler);
            }
        }
        let Some(mut runnable) = scheduler.borrow_mut().ready.pop_front() else {
            break;
        };
        CURRENT_YIELDER.with(|slot| slot.set(runnable.entry.yielder.get()));
        match runnable.entry.coroutine.resume(runnable.input) {
            corosensei::CoroutineResult::Yield(yielded) => {
                match yielded {
                    ProcessYield::Tick => scheduler.borrow_mut().ready.push_back(Runnable {
                        input: ProcessInput::None,
                        entry: runnable.entry,
                    }),
                    ProcessYield::Delay {
                        event,
                        priority,
                        deadline,
                    } => {
                        let entry = runnable.entry;
                        let waiter = EventWaiter {
                            id: entry.id,
                            priority,
                            deadline,
                            slot: Rc::new(RefCell::new(Some(entry))),
                        };
                        scheduler
                            .borrow_mut()
                            .event_waiters
                            .entry(event)
                            .or_default()
                            .push(waiter);
                    }
                    ProcessYield::DelayCase {
                        events,
                        priority,
                        deadline,
                    } => {
                        let entry = runnable.entry;
                        let id = entry.id;
                        let slot = Rc::new(RefCell::new(Some(entry)));
                        let mut state = scheduler.borrow_mut();
                        for event in events {
                            state
                                .event_waiters
                                .entry(event)
                                .or_default()
                                .push(EventWaiter {
                                    id,
                                    priority,
                                    deadline,
                                    slot: Rc::clone(&slot),
                                });
                        }
                    }
                    ProcessYield::Receive {
                        signals,
                        buffers,
                        deadline,
                    } => {
                        let entry = runnable.entry;
                        let id = entry.id;
                        let slot = Rc::new(RefCell::new(Some(entry)));
                        let mut state = scheduler.borrow_mut();
                        for signal in signals {
                            state.receivers.entry(signal.name).or_default().push((
                                id,
                                Rc::clone(&slot),
                                deadline,
                            ));
                        }
                        for buffer in buffers {
                            state.buffers.entry(buffer).or_default().receivers.push(
                                BufferReceiver {
                                    id,
                                    deadline,
                                    slot: Rc::clone(&slot),
                                },
                            );
                        }
                    }
                    ProcessYield::SendBuffer {
                        buffer,
                        priority,
                        values,
                        deadline,
                    } => {
                        let entry = runnable.entry;
                        let mut state = scheduler.borrow_mut();
                        state
                            .buffers
                            .entry(buffer)
                            .or_default()
                            .senders
                            .push(BufferSender {
                                id: entry.id,
                                priority,
                                values,
                                deadline,
                                slot: Rc::new(RefCell::new(Some(entry))),
                            });
                    }
                    ProcessYield::Sleep { deadline } => {
                        scheduler
                            .borrow_mut()
                            .sleepers
                            .push((deadline, runnable.entry));
                    }
                    ProcessYield::AcquireObject { object } => {
                        scheduler
                            .borrow_mut()
                            .object_waiters
                            .entry(object)
                            .or_default()
                            .push(runnable.entry);
                    }
                    ProcessYield::TaskWait { object } => {
                        scheduler
                            .borrow_mut()
                            .task_waiters
                            .entry(object)
                            .or_default()
                            .push(runnable.entry);
                    }
                    ProcessYield::AcquireRegion { region } => {
                        scheduler
                            .borrow_mut()
                            .region_waiters
                            .entry(region)
                            .or_default()
                            .push(runnable.entry);
                    }
                }
            }
            corosensei::CoroutineResult::Return(result) => {
                CURRENT_YIELDER.with(|slot| slot.set(std::ptr::null()));
                scheduler
                    .borrow_mut()
                    .live_processes
                    .remove(&runnable.entry.id);
                result?;
            }
        }
    }
    CURRENT_YIELDER.with(|slot| slot.set(std::ptr::null()));
    if scheduler
        .borrow()
        .event_waiters
        .values()
        .any(|waiting| waiting.iter().any(|waiter| waiter.slot.borrow().is_some()))
        || scheduler
            .borrow()
            .receivers
            .values()
            .any(|receivers| receivers.iter().any(|(_, slot, _)| slot.borrow().is_some()))
        || scheduler
            .borrow()
            .object_waiters
            .values()
            .any(|waiters| !waiters.is_empty())
        || scheduler.borrow().buffers.values().any(|buffer| {
            buffer
                .senders
                .iter()
                .any(|sender| sender.slot.borrow().is_some())
                || buffer
                    .receivers
                    .iter()
                    .any(|receiver| receiver.slot.borrow().is_some())
        })
    {
        return Err(Error::coded(
            "E0500",
            error_span,
            "deadlock: all remaining processes are delayed",
        ));
    }

    Ok(())
}

type CallUpdates = Vec<(Box<ir::AssignTarget>, Value)>;

#[derive(Clone)]
enum CallArgument {
    Value(Value),
    Target(Box<ir::AssignTarget>),
    InOut {
        target: Box<ir::AssignTarget>,
        value: Value,
    },
    Location(VarCell),
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Undefined,
    Int { value: i128, kind: Option<IntKind> },
    Float(f64),
    Bool(bool),
    Char(char),
    String(String),
    Bits(String),
    Duration(i128),
    Instance(usize),
    Time(i128),
    Ref(Option<usize>),
    Struct(Vec<(String, Value)>),
    Moreta(Vec<(String, Value)>),
    Array { low: i128, values: Vec<Value> },
    Set(i128),
    PowerSet(std::collections::BTreeSet<i128>),
    Association(Option<Rc<RefCell<AssociationState>>>),
    Access(Option<Rc<RefCell<AccessState>>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssociationState {
    path: String,
    existing: bool,
    indexable: bool,
}

#[derive(Debug, Clone)]
struct AccessState {
    association: Option<Rc<RefCell<AssociationState>>>,
    association_name: Option<String>,
    indexed: bool,
    index_range: Option<types::ArrayDimension>,
    position: i128,
    usage: i128,
    out_of_file: bool,
}

impl PartialEq for AccessState {
    fn eq(&self, other: &Self) -> bool {
        let same_association = match (&self.association, &other.association) {
            (Some(left), Some(right)) => Rc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        };
        same_association
            && self.association_name == other.association_name
            && self.indexed == other.indexed
            && self.index_range == other.index_range
            && self.position == other.position
            && self.usage == other.usage
            && self.out_of_file == other.out_of_file
    }
}

const USAGE_READONLY: i128 = 0;
const USAGE_WRITEONLY: i128 = 1;
const USAGE_READWRITE: i128 = 2;
const WHERE_FIRST: i128 = 0;
const WHERE_SAME: i128 = 1;
const WHERE_LAST: i128 = 2;

struct TextState {
    /// Text record sub-location.
    record: VarCell,
    /// Actual index, used as the default transfer cursor.
    index: i128,
    /// Access sub-location.
    access: VarCell,
}

struct Var {
    ty: Type,
    value: Value,
    /// Present for TEXT/WTEXT locations.
    text: Option<Rc<RefCell<TextState>>>,
}

impl Var {
    fn new(ty: Type, value: Value) -> Self {
        if let Type::Text { length, wide, .. } = ty {
            let record_ty = if wide {
                Type::WBoundedString {
                    length,
                    varying: true,
                }
            } else {
                Type::BoundedString {
                    length,
                    varying: true,
                }
            };
            let record = Rc::new(RefCell::new(Self::new(record_ty, value.clone())));
            let access_ty = Type::Access(types::AccessType {
                indexed: false,
                index: None,
            });
            let access = Rc::new(RefCell::new(Self::new(access_ty, Value::Access(None))));
            Self {
                ty,
                value,
                text: Some(Rc::new(RefCell::new(TextState {
                    record,
                    index: 0,
                    access,
                }))),
            }
        } else {
            Self {
                ty,
                value,
                text: None,
            }
        }
    }
}

type VarCell = Rc<RefCell<Var>>;

fn write_var_value(var: &mut Var, value: Value) {
    if let Some(text) = &var.text {
        let record = text.borrow().record.clone();
        record.borrow_mut().value = value.clone();
    }
    var.value = value;
}

#[derive(Clone, Copy)]
enum ImportKind {
    Var,
    Proc,
    Mode,
}

impl ImportKind {
    fn of(kind: &ExportKind) -> Self {
        match kind {
            ExportKind::Var(_) => Self::Var,
            ExportKind::Proc(_) => Self::Proc,
            ExportKind::Mode(_) => Self::Mode,
        }
    }
}

#[derive(Clone, Copy)]
struct ProcRef<'a> {
    def: &'a ProcDef,
    module: usize,
    scope_key: Option<Span>,
}

#[derive(Clone)]
struct ReachBinding<'a> {
    meta: crate::reach::ExportMeta,
    kind: ReachKind<'a>,
}

#[derive(Clone)]
enum ReachKind<'a> {
    Var(VarCell),
    Proc(ProcRef<'a>),
    Mode(Type),
}

#[derive(Clone, Copy)]
struct ProcessRef<'a> {
    def: &'a ProcessDef,
    module: usize,
    scope_key: Option<Span>,
}

#[derive(Clone)]
enum ProcessYield {
    Tick,
    Delay {
        event: usize,
        priority: i128,
        deadline: Option<Instant>,
    },
    Receive {
        signals: Vec<ReceiverSignal>,
        buffers: Vec<usize>,
        deadline: Option<Instant>,
    },
    SendBuffer {
        buffer: usize,
        priority: i128,
        values: Vec<Value>,
        deadline: Option<Instant>,
    },
    Sleep {
        deadline: Instant,
    },
    DelayCase {
        events: Vec<usize>,
        priority: i128,
        deadline: Option<Instant>,
    },
    AcquireRegion {
        region: usize,
    },
    AcquireObject {
        object: usize,
    },
    TaskWait {
        object: usize,
    },
}

#[derive(Clone)]
struct ReceiverSignal {
    name: String,
}

#[derive(Clone)]
enum ProcessInput {
    None,
    Timeout,
    Delay {
        event: usize,
    },
    Signal {
        name: String,
        values: Vec<Value>,
        sender: usize,
    },
    Buffer {
        name: String,
        values: Vec<Value>,
        sender: usize,
    },
    TaskReady,
}

type ProcessCoroutine = Coroutine<ProcessInput, ProcessYield, Result<(), Error>, DefaultStack>;
type ReceiverSlot = Rc<RefCell<Option<ProcessEntry>>>;

struct TaskJob {
    proc_span: Span,
    args: Vec<Value>,
}

struct EventWaiter {
    id: usize,
    priority: i128,
    deadline: Option<Instant>,
    slot: ReceiverSlot,
}

struct SignalMessage {
    target: Option<usize>,
    allowed_process: Option<String>,
    sender: usize,
    priority: i128,
    values: Vec<Value>,
}

struct BufferMessage {
    priority: i128,
    sender: usize,
    values: Vec<Value>,
}

struct BufferSender {
    id: usize,
    priority: i128,
    values: Vec<Value>,
    deadline: Option<Instant>,
    slot: ReceiverSlot,
}

struct BufferReceiver {
    id: usize,
    deadline: Option<Instant>,
    slot: ReceiverSlot,
}

#[derive(Default)]
struct BufferState {
    messages: VecDeque<BufferMessage>,
    senders: Vec<BufferSender>,
    receivers: Vec<BufferReceiver>,
}

struct ProcessEntry {
    id: usize,
    coroutine: ProcessCoroutine,
    yielder: Rc<Cell<*const Yielder<ProcessInput, ProcessYield>>>,
}

struct Runnable {
    input: ProcessInput,
    entry: ProcessEntry,
}

#[derive(Default)]
struct ScheduleState {
    next_process_id: usize,
    ready: VecDeque<Runnable>,
    live_processes: HashSet<usize>,
    process_names: HashMap<usize, String>,
    event_waiters: HashMap<usize, Vec<EventWaiter>>,
    signals: HashMap<String, VecDeque<SignalMessage>>,
    receivers: HashMap<String, Vec<(usize, ReceiverSlot, Option<Instant>)>>,
    buffers: HashMap<usize, BufferState>,
    sleepers: Vec<(Instant, ProcessEntry)>,
    task_jobs: HashMap<usize, VecDeque<TaskJob>>,
    task_waiters: HashMap<usize, Vec<ProcessEntry>>,
    task_workers: HashSet<usize>,
    task_stopped: HashSet<usize>,
    region_owner: HashMap<usize, usize>,
    region_waiters: HashMap<usize, Vec<ProcessEntry>>,
    object_owner: HashMap<usize, usize>,
    object_waiters: HashMap<usize, Vec<ProcessEntry>>,
}

thread_local! {
    static CURRENT_YIELDER: Cell<*const Yielder<ProcessInput, ProcessYield>> = const { Cell::new(std::ptr::null()) };
}

fn cooperative_yield() {
    CURRENT_YIELDER.with(|slot| {
        let ptr = slot.get();
        if !ptr.is_null() {
            // The pointer is installed by the scheduler only while this process is
            // executing on the current thread.
            unsafe {
                let _: ProcessInput = (*ptr).suspend(ProcessYield::Tick);
            }
        }
    });
}

fn cooperative_delay(
    event: usize,
    priority: i128,
    deadline: Option<Instant>,
) -> Result<ProcessInput, Error> {
    CURRENT_YIELDER.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            return Err(Error::coded(
                "E0500",
                Span::new(SourceId(0), 0, 0),
                "DELAY is only valid inside a process",
            ));
        }
        Ok(unsafe {
            (*ptr).suspend(ProcessYield::Delay {
                event,
                priority,
                deadline,
            })
        })
    })
}

fn cooperative_acquire_region(region: usize) -> Result<(), Error> {
    CURRENT_YIELDER.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            return Err(Error::coded(
                "E0500",
                Span::new(SourceId(0), 0, 0),
                "region call outside a process",
            ));
        }
        unsafe {
            let _: ProcessInput = (*ptr).suspend(ProcessYield::AcquireRegion { region });
        }
        Ok(())
    })
}

fn cooperative_delay_case(
    events: Vec<usize>,
    priority: i128,
    deadline: Option<Instant>,
) -> Result<ProcessInput, Error> {
    CURRENT_YIELDER.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            return Err(Error::coded(
                "E0500",
                Span::new(SourceId(0), 0, 0),
                "DELAY CASE is only valid inside a process",
            ));
        }
        Ok(unsafe {
            (*ptr).suspend(ProcessYield::DelayCase {
                events,
                priority,
                deadline,
            })
        })
    })
}

fn cooperative_send_buffer(
    buffer: usize,
    priority: i128,
    values: Vec<Value>,
    deadline: Option<Instant>,
) -> Result<ProcessInput, Error> {
    CURRENT_YIELDER.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            return Err(Error::coded(
                "E0500",
                Span::new(SourceId(0), 0, 0),
                "buffer send outside a process",
            ));
        }
        Ok(unsafe {
            (*ptr).suspend(ProcessYield::SendBuffer {
                buffer,
                priority,
                values,
                deadline,
            })
        })
    })
}

fn cooperative_sleep(deadline: Instant) -> Result<(), Error> {
    CURRENT_YIELDER.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            return Err(Error::coded(
                "E0500",
                Span::new(SourceId(0), 0, 0),
                "CYCLE is only valid inside a process",
            ));
        }
        unsafe {
            let _: ProcessInput = (*ptr).suspend(ProcessYield::Sleep { deadline });
        }
        Ok(())
    })
}

fn cooperative_task_wait(object: usize) -> Result<ProcessInput, Error> {
    CURRENT_YIELDER.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            return Err(Error::coded(
                "E0500",
                Span::new(SourceId(0), 0, 0),
                "task worker outside a process",
            ));
        }
        Ok(unsafe { (*ptr).suspend(ProcessYield::TaskWait { object }) })
    })
}

fn cooperative_acquire_object(object: usize) -> Result<(), Error> {
    CURRENT_YIELDER.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            return Err(Error::coded(
                "E0500",
                Span::new(SourceId(0), 0, 0),
                "region object call outside a process",
            ));
        }
        unsafe {
            let _: ProcessInput = (*ptr).suspend(ProcessYield::AcquireObject { object });
        }
        Ok(())
    })
}

fn cooperative_receive(
    signals: Vec<String>,
    buffers: Vec<usize>,
    deadline: Option<Instant>,
) -> Result<ProcessInput, Error> {
    CURRENT_YIELDER.with(|slot| {
        let ptr = slot.get();
        if ptr.is_null() {
            return Err(Error::coded(
                "E0500",
                Span::new(SourceId(0), 0, 0),
                "RECEIVE is only valid inside a process",
            ));
        }
        Ok(unsafe {
            (*ptr).suspend(ProcessYield::Receive {
                signals: signals
                    .into_iter()
                    .map(|name| ReceiverSignal { name })
                    .collect(),
                buffers,
                deadline,
            })
        })
    })
}

fn next_timed_deadline(scheduler: &Rc<RefCell<ScheduleState>>) -> Option<Instant> {
    let state = scheduler.borrow();
    let waiting_deadline = state
        .event_waiters
        .values()
        .flat_map(|waiting| waiting.iter().filter_map(|waiter| waiter.deadline))
        .min();
    let receiver_deadline = state
        .receivers
        .values()
        .flat_map(|receivers| receivers.iter().filter_map(|(_, _, deadline)| *deadline))
        .min();
    let sleeper_deadline = state.sleepers.iter().map(|(deadline, _)| *deadline).min();
    let buffer_deadline = state
        .buffers
        .values()
        .flat_map(|buffer| {
            buffer
                .senders
                .iter()
                .filter_map(|sender| sender.deadline)
                .chain(
                    buffer
                        .receivers
                        .iter()
                        .filter_map(|receiver| receiver.deadline),
                )
        })
        .min();
    [
        waiting_deadline,
        receiver_deadline,
        sleeper_deadline,
        buffer_deadline,
    ]
    .into_iter()
    .flatten()
    .min()
}

fn expire_timed_waiters(scheduler: &Rc<RefCell<ScheduleState>>) {
    let now = Instant::now();
    let mut expired = Vec::new();
    let mut expired_ids = Vec::new();
    let mut state = scheduler.borrow_mut();

    for waiting in state.event_waiters.values_mut() {
        let mut index = 0;
        while index < waiting.len() {
            let expired_entry = waiting[index]
                .deadline
                .is_some_and(|deadline| deadline <= now)
                && waiting[index].slot.borrow().is_some();
            if expired_entry {
                let id = waiting[index].id;
                let entry = waiting[index].slot.borrow_mut().take();
                waiting.remove(index);
                if !expired_ids.contains(&id) {
                    expired_ids.push(id);
                }
                if let Some(entry) = entry {
                    expired.push(Runnable {
                        input: ProcessInput::Timeout,
                        entry,
                    });
                }
            } else {
                index += 1;
            }
        }
    }
    for waiting in state.event_waiters.values_mut() {
        waiting
            .retain(|waiter| !expired_ids.contains(&waiter.id) || waiter.slot.borrow().is_some());
    }

    for receivers in state.receivers.values_mut() {
        let mut index = 0;
        while index < receivers.len() {
            let (id, slot, deadline) = &receivers[index];
            let expired_entry =
                deadline.is_some_and(|deadline| deadline <= now) && slot.borrow().is_some();
            if expired_entry {
                let id = *id;
                let entry = receivers[index].1.borrow_mut().take();
                receivers.remove(index);
                expired_ids.push(id);
                if let Some(entry) = entry {
                    expired.push(Runnable {
                        input: ProcessInput::Timeout,
                        entry,
                    });
                }
            } else {
                index += 1;
            }
        }
    }
    for receivers in state.receivers.values_mut() {
        receivers.retain(|(id, _, _)| !expired_ids.contains(id));
    }
    let mut index = 0;
    while index < state.sleepers.len() {
        if state.sleepers[index].0 <= now {
            let (_, entry) = state.sleepers.remove(index);
            expired.push(Runnable {
                input: ProcessInput::None,
                entry,
            });
        } else {
            index += 1;
        }
    }
    for buffer in state.buffers.values_mut() {
        let mut index = 0;
        while index < buffer.senders.len() {
            let expired_entry = buffer.senders[index]
                .deadline
                .is_some_and(|deadline| deadline <= now)
                && buffer.senders[index].slot.borrow().is_some();
            if expired_entry {
                let id = buffer.senders[index].id;
                let entry = buffer.senders[index].slot.borrow_mut().take();
                buffer.senders.remove(index);
                expired_ids.push(id);
                if let Some(entry) = entry {
                    expired.push(Runnable {
                        input: ProcessInput::Timeout,
                        entry,
                    });
                }
            } else {
                index += 1;
            }
        }
        let mut index = 0;
        while index < buffer.receivers.len() {
            let expired_entry = buffer.receivers[index]
                .deadline
                .is_some_and(|deadline| deadline <= now)
                && buffer.receivers[index].slot.borrow().is_some();
            if expired_entry {
                let id = buffer.receivers[index].id;
                let entry = buffer.receivers[index].slot.borrow_mut().take();
                buffer.receivers.remove(index);
                expired_ids.push(id);
                if let Some(entry) = entry {
                    expired.push(Runnable {
                        input: ProcessInput::Timeout,
                        entry,
                    });
                }
            } else {
                index += 1;
            }
        }
        buffer
            .receivers
            .retain(|receiver| !expired_ids.contains(&receiver.id));
        buffer
            .senders
            .retain(|sender| !expired_ids.contains(&sender.id));
    }
    for runnable in expired {
        state.ready.push_back(runnable);
    }
}

#[derive(Clone, Default)]
struct Scope<'a> {
    vars: HashMap<String, VarCell>,
    procs: HashMap<String, ProcRef<'a>>,
    processes: HashMap<String, ProcessRef<'a>>,
    signals: HashMap<String, Vec<Type>>,
    signal_targets: HashMap<String, Option<String>>,
    modes: HashMap<String, Type>,
    /// Names granted into this reach by directly enclosed modules.
    exports: HashMap<String, Vec<ReachBinding<'a>>>,
    /// Whether hoisted-module `SEIZE`s have been applied.
    seizes_applied: bool,
    /// Span of the nested module that owns this scope, if any.
    key: Option<Span>,
}

struct Frame {
    result_type: Option<Type>,
    result: Option<Value>,
}

#[derive(Debug)]
enum Flow {
    Normal,
    Return,
    Stop,
    Timeout,
    Exit(Option<String>),
}

struct Interpreter<'a> {
    profile: WidthProfile,
    /// Persistent scopes, one per module, indexed like `program.modules`.
    modules: Vec<Scope<'a>>,
    /// Call and `BEGIN` scopes, searched before the active module scope.
    locals: Vec<Scope<'a>>,
    /// Index into `locals` at each procedure-call entry.
    call_bases: Vec<usize>,
    active_module: usize,
    /// Module scope used for lookups while executing a procedure.
    active_scope_key: Option<Span>,
    /// Lowest local scope visible in the current module.
    module_floor: usize,
    depth: usize,
    yield_interval: usize,
    yield_counter: usize,
    exports: Vec<program::ExportEntry>,
    modes: Rc<crate::modes::ModeTable>,
    module_modes: Vec<HashMap<String, Type>>,
    reaches: program::ReachTree,
    modules_by_span: HashMap<Span, &'a Module>,
    proc_defs: HashMap<Span, &'a ProcDef>,
    moreta_decls: HashMap<types::MoretaId, Vec<Declaration>>,
    moreta_invariants: HashMap<types::MoretaId, Expr>,
    package_names: Vec<Option<String>>,
    /// Scopes for nested modules declared in a module body, keyed by span.
    nested_scopes: HashMap<Span, Scope<'a>>,
    process_defs: HashMap<Span, &'a ProcessDef>,
    regions: HashSet<usize>,
    process_id: usize,
    process_name: String,
    region_depths: HashMap<usize, usize>,
    object_depths: HashMap<usize, usize>,
    deadline: Option<Instant>,
    scheduler: Rc<RefCell<ScheduleState>>,
    exprs: ir::ExprTable,
    bodies: ir::BodyTable,
    refs: Vec<Option<VarCell>>,
    ref_allocated: Vec<bool>,
    self_cells: Vec<VarCell>,
    self_modes: Vec<types::MoretaId>,
}

impl<'a> Interpreter<'a> {
    fn declare_modules(&mut self, program: &'a Program) -> Result<(), Error> {
        for (index, module) in program.modules.iter().enumerate() {
            let mode_names = self.module_modes[index].clone();
            let modes = Rc::clone(&self.modes);
            let mut scope = Scope {
                modes: mode_names.clone(),
                ..Scope::default()
            };
            if module.kind.is_body() {
                declare_into_scope(
                    &mut scope,
                    &module.declarations,
                    index,
                    &mode_names,
                    &modes,
                    None,
                )?;
            }
            self.modules.push(scope);
            if module.kind.is_body() {
                self.hoist_top_module(module, index)?;
            }
        }
        Ok(())
    }

    fn hoist_top_module(&mut self, module: &'a Module, index: usize) -> Result<(), Error> {
        let mut parent = std::mem::take(&mut self.modules[index]);
        let mode_names = self.module_modes[index].clone();
        self.hoist_children(module.span, &mut parent, index, &mode_names)?;
        self.modules[index] = parent;
        Ok(())
    }

    fn hoist_children(
        &mut self,
        span: Span,
        parent: &mut Scope<'a>,
        top: usize,
        mode_names: &HashMap<String, Type>,
    ) -> Result<(), Error> {
        self.hoist_reach_children(span, parent, top, mode_names, false)?;
        self.seed_reach_exports(span, parent);
        Ok(())
    }

    /// Hoist the modules that belong to `span`'s reach. Module bodies only hoist
    /// directly-enclosed modules; block reaches also hoist modules declared in the
    /// block. Names are always resolved against the live mode table.
    fn hoist_reach_children(
        &mut self,
        span: Span,
        parent: &mut Scope<'a>,
        top: usize,
        mode_names: &HashMap<String, Type>,
        include_block_children: bool,
    ) -> Result<(), Error> {
        let Some(node) = self.reaches.get(span) else {
            return Ok(());
        };
        let children: Vec<usize> = node.children.clone();
        for child_id in children {
            let Some(child_node) = self.reaches.nodes.get(child_id) else {
                continue;
            };
            if !child_node.hoisted && !include_block_children {
                continue;
            }
            let child_span = child_node.span;
            let Some(child) = self.modules_by_span.get(&child_span).copied() else {
                continue;
            };
            let child_modes = child_node.interface.modes.clone();

            let mut resolution_modes = child_modes.clone();
            resolution_modes.extend(parent.modes.clone());
            resolution_modes.extend(mode_names.clone());
            let modes = Rc::clone(&self.modes);
            let mut scope = Scope {
                modes: child_modes,
                key: Some(child_span),
                ..Scope::default()
            };
            declare_into_scope(
                &mut scope,
                &child.declarations,
                top,
                &resolution_modes,
                &modes,
                Some(child_span),
            )?;
            self.hoist_children(child_span, &mut scope, top, mode_names)?;
            self.nested_scopes.insert(child_span, scope);
        }
        Ok(())
    }

    fn seed_reach_exports(&mut self, span: Span, scope: &mut Scope<'a>) {
        let Some(node) = self.reaches.get(span) else {
            return;
        };
        let exports = node.exports.clone();
        for (name, entries) in exports {
            for entry in entries {
                let Some(owner) = self.nested_scopes.get(&entry.owner) else {
                    continue;
                };
                let binding = match entry.kind {
                    program::ReachExportKind::Var(_) => {
                        let cell = owner.vars.get(&name).cloned().or_else(|| {
                            match owner.exports.get(&name) {
                                Some(exports) => match exports.as_slice() {
                                    [ReachBinding {
                                        kind: ReachKind::Var(cell),
                                        ..
                                    }] => Some(cell.clone()),
                                    _ => None,
                                },
                                None => None,
                            }
                        });
                        let Some(cell) = cell else {
                            continue;
                        };
                        ReachBinding {
                            meta: entry.meta,
                            kind: ReachKind::Var(cell),
                        }
                    }
                    program::ReachExportKind::Proc(_) => {
                        let proc_ref = owner.procs.get(&name).copied().or_else(|| {
                            match owner.exports.get(&name) {
                                Some(exports) => match exports.as_slice() {
                                    [ReachBinding {
                                        kind: ReachKind::Proc(proc_ref),
                                        ..
                                    }] => Some(*proc_ref),
                                    _ => None,
                                },
                                None => None,
                            }
                        });
                        let Some(proc_ref) = proc_ref else {
                            continue;
                        };
                        ReachBinding {
                            meta: entry.meta,
                            kind: ReachKind::Proc(proc_ref),
                        }
                    }
                    program::ReachExportKind::Mode(ty) => ReachBinding {
                        meta: entry.meta,
                        kind: ReachKind::Mode(ty),
                    },
                };
                scope.exports.entry(name.clone()).or_default().push(binding);
            }
        }
    }

    fn apply_seizes(&mut self, program: &'a Program) -> Result<(), Error> {
        let mut imports = Vec::new();

        for (current, module) in program.modules.iter().enumerate() {
            if module.kind.is_spec() {
                continue;
            }
            let package_name = module.package_name.as_deref();
            let module_name = module.name.as_deref();

            for declaration in &module.declarations {
                let Declaration::Visibility(visibility) = declaration else {
                    continue;
                };
                if visibility.kind != VisibilityKind::Seize {
                    continue;
                }

                if visibility.all {
                    let old_prefix = program::seize_old_prefix(&visibility.prefix)
                        .map_err(|message| Error::coded("E0500", visibility.span, message))?;
                    let mut accessible: Vec<&program::ExportEntry> = self
                        .exports
                        .iter()
                        .filter(|export| export.module != current)
                        .filter(|export| export.prefix == old_prefix)
                        .filter(|export| export.is_accessible_to(package_name, module_name))
                        .collect();
                    accessible.sort_by(|left, right| {
                        left.name
                            .cmp(&right.name)
                            .then_with(|| left.package.cmp(&right.package))
                            .then_with(|| left.module.cmp(&right.module))
                    });

                    let mut names = HashSet::new();
                    for export in accessible {
                        if !names.insert(export.name.clone()) {
                            return Err(Error::coded(
                                "E0500",
                                visibility.span,
                                format!(
                                    "SEIZE ALL is ambiguous for `{}`; use a qualified SEIZE",
                                    export.name
                                ),
                            ));
                        }
                        if export.module == usize::MAX {
                            return Err(Error::coded(
                                "E0508",
                                visibility.span,
                                format!(
                                    "cannot SEIZE `{}`: external dependency is not linked",
                                    export.name
                                ),
                            ));
                        }
                        imports.push((
                            current,
                            export.name.clone(),
                            visibility.span,
                            export.module,
                            ImportKind::of(&export.kind),
                        ));
                    }
                    continue;
                }

                for name in &visibility.names {
                    let (source, local) = program::seize_name(name, &visibility.prefix)
                        .map_err(|message| Error::coded("E0500", name.span, message))?;
                    let export =
                        program::resolve_export(&self.exports, &source, package_name, module_name)
                            .map_err(|message| Error::coded("E0500", name.span, message))?;

                    if export.module == current {
                        return Err(Error::coded(
                            "E0508",
                            name.span,
                            format!("cannot SEIZE `{}` from the same module", name.qualified()),
                        ));
                    }
                    if export.module == usize::MAX {
                        return Err(Error::coded(
                            "E0508",
                            name.span,
                            format!(
                                "cannot SEIZE `{}`: external dependency is not linked",
                                name.qualified()
                            ),
                        ));
                    }

                    imports.push((
                        current,
                        local,
                        name.span,
                        export.module,
                        ImportKind::of(&export.kind),
                    ));
                }
            }
        }

        for (current, name, span, source, kind) in imports {
            let duplicate = match kind {
                ImportKind::Proc => {
                    let proc_ref = *self.modules[source]
                        .procs
                        .get(&name)
                        .or_else(|| match self.modules[source].exports.get(&name) {
                            Some(exports) => match exports.as_slice() {
                                [ReachBinding {
                                    kind: ReachKind::Proc(proc_ref),
                                    ..
                                }] => Some(proc_ref),
                                _ => None,
                            },
                            None => None,
                        })
                        .ok_or_else(|| {
                            Error::coded(
                                "E0508",
                                span,
                                format!("cannot SEIZE `{name}`: not a procedure"),
                            )
                        })?;
                    self.modules[current]
                        .procs
                        .insert(name.clone(), proc_ref)
                        .is_some()
                }
                ImportKind::Var => {
                    let cell = self.modules[source]
                        .vars
                        .get(&name)
                        .cloned()
                        .or_else(|| match self.modules[source].exports.get(&name) {
                            Some(exports) => match exports.as_slice() {
                                [ReachBinding {
                                    kind: ReachKind::Var(cell),
                                    ..
                                }] => Some(cell.clone()),
                                _ => None,
                            },
                            None => None,
                        })
                        .ok_or_else(|| {
                            Error::coded(
                                "E0508",
                                span,
                                format!("cannot SEIZE `{name}`: not a location"),
                            )
                        })?;
                    self.modules[current]
                        .vars
                        .insert(name.clone(), cell)
                        .is_some()
                }
                // Mode seizes are installed before declarations are collected.
                ImportKind::Mode => false,
            };
            if duplicate {
                return Err(Error::coded(
                    "E0508",
                    span,
                    format!("duplicate declaration of `{name}`"),
                ));
            }
        }

        Ok(())
    }

    fn moreta_initializer_declarations(&self, id: types::MoretaId) -> Vec<Declaration> {
        let mut declarations = Vec::new();
        for base in &self.modes.get_moreta(id).bases {
            declarations.extend(self.moreta_initializer_declarations(*base));
        }
        if let Some(own) = self.moreta_decls.get(&id) {
            declarations.extend(
                own.iter()
                    .filter(|declaration| matches!(declaration, Declaration::Var(_)))
                    .cloned(),
            );
        }
        declarations
    }

    fn create_moreta_object_with_args(
        &mut self,
        id: types::MoretaId,
        constructor_args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        if !self.moreta_decls.contains_key(&id) {
            return Err(Error::coded(
                "E0500",
                span,
                "internal: missing module mode declarations",
            ));
        }
        let initializer_declarations = self.moreta_initializer_declarations(id);
        self.locals.push(Scope::default());
        {
            let scope = self.locals.last_mut().expect("module mode scope");
            for component in &self.modes.get_moreta(id).components {
                if let crate::modes::MoretaComponent::Var { name, ty, .. } = component {
                    scope.vars.insert(
                        name.clone(),
                        Rc::new(RefCell::new(Var::new(*ty, default_value(&self.modes, *ty)))),
                    );
                }
            }
        }
        let result = (|| {
            self.initialize_declarations(&initializer_declarations)?;
            if let Some((signature, constructor_span)) = self.modes.moreta_constructor(id) {
                if signature.params.len() != constructor_args.len() {
                    return Err(Error::coded(
                        "E0500",
                        span,
                        format!(
                            "constructor expects {} argument(s), found {}",
                            signature.params.len(),
                            constructor_args.len()
                        ),
                    ));
                }
                let value = self.object_value_from_scope(id, span)?;
                let updated = self.invoke_component_on_value(
                    id,
                    constructor_span,
                    value,
                    constructor_args,
                    span,
                )?;
                self.apply_object_value_to_scope(id, updated, span)?;
            }
            self.check_moreta_invariants(id, span)
        })();
        if let Err(error) = result {
            self.locals.pop();
            return Err(error);
        }
        let scope = self.locals.pop().expect("module mode scope");
        let mut components = Vec::new();
        for component in &self.modes.get_moreta(id).components {
            if let crate::modes::MoretaComponent::Var { name, ty, .. } = component {
                let value = if let Some(cell) = scope.vars.get(name) {
                    cell.borrow().value.clone()
                } else {
                    default_value(&self.modes, *ty)
                };
                components.push((name.clone(), value));
            }
        }
        Ok(Value::Moreta(components))
    }

    fn initialize_declarations(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            let Declaration::Var(vars) = declaration else {
                continue;
            };
            for var in vars {
                if let Some(init) = &var.init {
                    let value = self.eval_expr(&init.value)?;
                    self.assign_var(&var.name, value, var.span)?;
                } else if let Ok(Type::Moreta(id)) = self.resolve_mode(&var.mode) {
                    let mut args = Vec::new();
                    if let Some(actuals) = &var.constructor_args {
                        for actual in actuals {
                            args.push(self.eval_expr(actual)?);
                        }
                    }
                    let value = self.create_moreta_object_with_args(id, args, var.span)?;
                    self.assign_var(&var.name, value, var.span)?;
                }
                if let Ok(Type::Moreta(id)) = self.resolve_mode(&var.mode) {
                    if self.modes.get_moreta(id).kind == crate::ast::MoretaKind::Task {
                        if let Some(cell) = self.find_var_cell(&var.name) {
                            self.start_task_worker(cell, var.span)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn declare_local(&mut self, declarations: &'a [Declaration]) -> Result<(), Error> {
        let module = self.active_module;
        let mode_names = self.module_modes[module].clone();
        let modes = Rc::clone(&self.modes);
        let scope = self.locals.last_mut().expect("local scope");
        declare_into_scope(scope, declarations, module, &mode_names, &modes, None)
    }

    fn declare_local_vars(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        let mut mode_names = self.modules[self.active_module].modes.clone();
        let floor = self.local_floor();
        for scope in &self.locals[floor..] {
            for (name, ty) in &scope.modes {
                mode_names.insert(name.clone(), *ty);
            }
        }
        let modes = Rc::clone(&self.modes);
        let scope = self.locals.last_mut().expect("local scope");
        for declaration in declarations {
            match declaration {
                Declaration::Var(vars) => {
                    for var in vars {
                        let ty = resolve_mode_with(&var.mode, &mode_names)?;
                        if scope.vars.contains_key(&var.name) || scope.procs.contains_key(&var.name)
                        {
                            return Err(Error::coded(
                                "E0508",
                                var.span,
                                format!("duplicate declaration of `{}`", var.name),
                            ));
                        }
                        scope.vars.insert(
                            var.name.clone(),
                            Rc::new(RefCell::new(Var::new(ty, default_value(&modes, ty)))),
                        );
                    }
                }
                Declaration::Proc(proc_def) => {
                    return Err(Error::coded(
                        "E0500",
                        proc_def.span,
                        "procedure definitions inside BEGIN are not implemented",
                    ));
                }
                Declaration::ProcSignature(_)
                | Declaration::Visibility(_)
                | Declaration::Mode(_)
                | Declaration::SpecModule(_)
                | Declaration::GenericProc(_)
                | Declaration::GenericProcInstance(_)
                | Declaration::Process(_)
                | Declaration::Signal(_)
                | Declaration::GenericProcess(_)
                | Declaration::GenericProcessInstance(_) => {}
            }
        }
        Ok(())
    }

    fn run_module(&mut self, module: &'a Module) -> Result<(), Error> {
        let body = self.bodies.get(module.span).ok_or_else(|| {
            Error::coded("E0500", module.span, "internal: missing module IR body")
        })?;
        let mut frame = Frame {
            result_type: None,
            result: None,
        };
        match self.exec_with_handler(module.span, &body, &mut frame)? {
            Flow::Normal | Flow::Return | Flow::Stop | Flow::Timeout | Flow::Exit(None) => Ok(()),
            Flow::Exit(Some(name)) => {
                if module.name.as_deref() == Some(name.as_str()) {
                    Ok(())
                } else {
                    Err(Error::coded(
                        "E0508",
                        module.span,
                        format!("unknown EXIT label `{name}`"),
                    ))
                }
            }
        }
    }

    fn exec_with_handler(
        &mut self,
        span: Span,
        stmts: &[ir::Stmt],
        frame: &mut Frame,
    ) -> Result<Flow, Error> {
        match self.exec_stmts(stmts, frame) {
            Err(error) if error.exception.is_some() => {
                let name = error.exception.as_deref().unwrap_or_default();
                if let Some(handler) = self.bodies.handler(span) {
                    if let Some(actions) = handler.alternatives.iter().find_map(|alternative| {
                        alternative
                            .exceptions
                            .iter()
                            .any(|exception| exception == name)
                            .then_some(&alternative.actions)
                    }) {
                        return self.exec_stmts(actions, frame);
                    }
                    if let Some(actions) = &handler.else_actions {
                        return self.exec_stmts(actions, frame);
                    }
                }
                Err(error)
            }
            other => other,
        }
    }

    fn exec_stmts(&mut self, stmts: &[ir::Stmt], frame: &mut Frame) -> Result<Flow, Error> {
        for stmt in stmts {
            if self.yield_interval <= 1 {
                cooperative_yield();
            } else {
                self.yield_counter += 1;
                if self.yield_counter >= self.yield_interval {
                    self.yield_counter = 0;
                    cooperative_yield();
                }
            }
            match stmt {
                ir::Stmt::Assign {
                    target,
                    op,
                    value,
                    span,
                } => {
                    let value = if *op == AssignOp::Assign {
                        self.eval_ir(value)?
                    } else {
                        let current = self.read_target(target, *span)?;
                        let right = self.eval_ir(value)?;
                        let target_ty = self.target_type(target, *span)?;
                        self.apply_binary(
                            binary_for_assign(*op),
                            current,
                            right,
                            Some(target_ty),
                            *span,
                        )?
                    };
                    self.assign_target(target, value, *span)?;
                }
                ir::Stmt::Assert { condition, span } => {
                    let value = self.eval_ir(condition)?;
                    if !self.as_bool(value, *span)? {
                        return Err(Error::coded_exception(
                            "E0501",
                            "ASSERTFAIL",
                            *span,
                            "ASSERTFAIL",
                        ));
                    }
                }
                ir::Stmt::Call { call, .. } => {
                    self.eval_ir(call)?;
                }
                ir::Stmt::DelayCase {
                    alternatives,
                    priority,
                    span,
                } => {
                    let priority = match priority {
                        Some(priority) => {
                            let value = self.eval_ir(priority)?;
                            let (priority, _) = self.expect_int(value, priority.span())?;
                            if priority < 0 {
                                return Err(Error::coded(
                                    "E0500",
                                    *span,
                                    "DELAY CASE priority must be non-negative",
                                ));
                            }
                            priority
                        }
                        None => 0,
                    };
                    let mut all_events = Vec::new();
                    let mut alternative_events = Vec::with_capacity(alternatives.len());
                    for alternative in alternatives {
                        let mut events = Vec::with_capacity(alternative.events.len());
                        for event in &alternative.events {
                            let (key, length) = self.event_location(event, *span)?;
                            let count = self
                                .scheduler
                                .borrow()
                                .event_waiters
                                .get(&key)
                                .map(|waiters| {
                                    waiters
                                        .iter()
                                        .filter(|waiter| waiter.slot.borrow().is_some())
                                        .count()
                                })
                                .unwrap_or(0);
                            if let Some(length) = length {
                                if count >= length {
                                    return Err(Error::exception(
                                        "DELAYFAIL",
                                        *span,
                                        "DELAYFAIL: event length exceeded",
                                    ));
                                }
                            }
                            events.push(key);
                            all_events.push(key);
                        }
                        alternative_events.push(events);
                    }
                    all_events.sort_unstable();
                    all_events.dedup();
                    let input = cooperative_delay_case(all_events, priority, self.deadline)?;
                    match input {
                        ProcessInput::Timeout => return Ok(Flow::Timeout),
                        ProcessInput::Delay { event } => {
                            let alternative = alternatives
                                .iter()
                                .zip(&alternative_events)
                                .find(|(_, events)| events.contains(&event))
                                .map(|(alternative, _)| alternative)
                                .ok_or_else(|| {
                                    Error::coded(
                                        "E0500",
                                        *span,
                                        "internal: DELAY CASE woke on an unknown event",
                                    )
                                })?;
                            let flow = self.exec_stmts(&alternative.actions, frame)?;
                            if !matches!(flow, Flow::Normal) {
                                return Ok(flow);
                            }
                        }
                        _ => {
                            return Err(Error::coded(
                                "E0500",
                                *span,
                                "internal: DELAY CASE resumed unexpectedly",
                            ))
                        }
                    }
                }
                ir::Stmt::Timing {
                    duration,
                    absolute,
                    body,
                    timeout,
                    span,
                } => {
                    let value = self.eval_ir(duration)?;
                    let milliseconds = if *absolute {
                        let Value::Time(target) = value else {
                            return Err(Error::coded(
                                "E0507",
                                duration.span(),
                                "AT requires a TIME value",
                            ));
                        };
                        (target - unix_time_ms()).max(0)
                    } else {
                        let Value::Duration(milliseconds) = value else {
                            return Err(Error::coded(
                                "E0507",
                                duration.span(),
                                "AFTER requires a DURATION value",
                            ));
                        };
                        milliseconds.max(0)
                    };
                    let deadline = Instant::now()
                        .checked_add(std::time::Duration::from_millis(milliseconds as u64))
                        .ok_or_else(|| Error::coded("E0502", *span, "duration is too large"))?;
                    let previous = self.deadline;
                    self.deadline = Some(deadline);
                    let flow = self.exec_stmts(body, frame);
                    self.deadline = previous;
                    match flow? {
                        Flow::Timeout => {
                            let flow = self.exec_stmts(timeout, frame)?;
                            if !matches!(flow, Flow::Normal) {
                                return Ok(flow);
                            }
                        }
                        Flow::Normal => {}
                        flow => return Ok(flow),
                    }
                }
                ir::Stmt::Cycle {
                    duration,
                    body,
                    span,
                } => {
                    let value = self.eval_ir(duration)?;
                    let Value::Duration(milliseconds) = value else {
                        return Err(Error::coded(
                            "E0507",
                            duration.span(),
                            "CYCLE requires a DURATION value",
                        ));
                    };
                    loop {
                        match self.exec_stmts(body, frame)? {
                            Flow::Normal => {}
                            Flow::Exit(None) => break,
                            flow => return Ok(flow),
                        }
                        let deadline = Instant::now()
                            .checked_add(std::time::Duration::from_millis(
                                milliseconds.max(0) as u64
                            ))
                            .ok_or_else(|| {
                                Error::coded("E0502", *span, "cycle duration is too large")
                            })?;
                        cooperative_sleep(deadline)?;
                    }
                }
                ir::Stmt::Stop { .. } => return Ok(Flow::Stop),
                ir::Stmt::Delay {
                    event,
                    priority,
                    span,
                } => {
                    let (key, length) = self.event_location(event, *span)?;
                    let priority = match priority {
                        Some(priority) => {
                            let value = self.eval_ir(priority)?;
                            let (priority, _) = self.expect_int(value, priority.span())?;
                            if priority < 0 {
                                return Err(Error::coded(
                                    "E0500",
                                    *span,
                                    "DELAY priority must be non-negative",
                                ));
                            }
                            priority
                        }
                        None => 0,
                    };
                    let count = self
                        .scheduler
                        .borrow()
                        .event_waiters
                        .get(&key)
                        .map(|waiters| {
                            waiters
                                .iter()
                                .filter(|waiter| waiter.slot.borrow().is_some())
                                .count()
                        })
                        .unwrap_or(0);
                    if let Some(length) = length {
                        if count >= length {
                            return Err(Error::exception(
                                "DELAYFAIL",
                                *span,
                                "DELAYFAIL: event length exceeded",
                            ));
                        }
                    }
                    match cooperative_delay(key, priority, self.deadline)? {
                        ProcessInput::Timeout => return Ok(Flow::Timeout),
                        ProcessInput::Delay { .. } | ProcessInput::None => {}
                        ProcessInput::Signal { .. }
                        | ProcessInput::Buffer { .. }
                        | ProcessInput::TaskReady => {
                            return Err(Error::coded(
                                "E0500",
                                *span,
                                "internal: DELAY resumed with a signal",
                            ))
                        }
                    }
                }
                ir::Stmt::Continue { event, span } => {
                    let (key, _) = self.event_location(event, *span)?;
                    let mut scheduler = self.scheduler.borrow_mut();
                    let Some(waiting) = scheduler.event_waiters.get_mut(&key) else {
                        continue;
                    };
                    let mut best = None;
                    for (index, waiter) in waiting.iter().enumerate() {
                        if waiter.slot.borrow().is_some()
                            && best
                                .map(|best: usize| waiter.priority > waiting[best].priority)
                                .unwrap_or(true)
                        {
                            best = Some(index);
                        }
                    }
                    let Some(best) = best else {
                        continue;
                    };
                    let id = waiting[best].id;
                    let entry = waiting[best].slot.borrow_mut().take();
                    let Some(entry) = entry else {
                        continue;
                    };
                    for waiters in scheduler.event_waiters.values_mut() {
                        waiters.retain(|waiter| waiter.id != id);
                    }
                    scheduler.ready.push_back(Runnable {
                        input: ProcessInput::Delay { event: key },
                        entry,
                    });
                }
                ir::Stmt::Send {
                    signal,
                    values,
                    target,
                    priority,
                    span,
                } => {
                    let priority = match priority {
                        Some(priority) => {
                            let value = self.eval_ir(priority)?;
                            let (priority, _) = self.expect_int(value, priority.span())?;
                            if priority < 0 {
                                return Err(Error::coded(
                                    "E0500",
                                    *span,
                                    "SEND priority must be non-negative",
                                ));
                            }
                            priority
                        }
                        None => 0,
                    };
                    if self.lookup_signal_modes(signal).is_none() {
                        let (key, length, mode) = self.buffer_location(signal, *span)?;
                        if values.len() != 1 {
                            return Err(Error::coded(
                                "E0507",
                                *span,
                                format!("buffer `{signal}` expects exactly one value"),
                            ));
                        }
                        let value = self.eval_ir(&values[0])?;
                        let value = self.convert(value, mode, *span)?;
                        let mut scheduler = self.scheduler.borrow_mut();
                        let receiver = scheduler.buffers.get(&key).and_then(|buffer| {
                            buffer
                                .receivers
                                .iter()
                                .enumerate()
                                .find_map(|(index, receiver)| {
                                    receiver
                                        .slot
                                        .borrow()
                                        .is_some()
                                        .then_some((index, receiver.id))
                                })
                        });
                        if let Some((index, id)) = receiver {
                            let state = scheduler.buffers.get_mut(&key).expect("buffer");
                            let slot = Rc::clone(&state.receivers[index].slot);
                            state.receivers.remove(index);
                            for buffer in scheduler.buffers.values_mut() {
                                buffer.receivers.retain(|receiver| receiver.id != id);
                            }
                            let entry = slot.borrow_mut().take().expect("buffer receiver");
                            scheduler.ready.push_back(Runnable {
                                input: ProcessInput::Buffer {
                                    name: signal.clone(),
                                    values: vec![value],
                                    sender: self.process_id,
                                },
                                entry,
                            });
                        } else {
                            let full = length.is_some_and(|length| {
                                scheduler
                                    .buffers
                                    .get(&key)
                                    .map(|buffer| buffer.messages.len() >= length)
                                    .unwrap_or(false)
                            });
                            if full {
                                drop(scheduler);
                                if let ProcessInput::Timeout = cooperative_send_buffer(
                                    key,
                                    priority,
                                    vec![value],
                                    self.deadline,
                                )? {
                                    return Ok(Flow::Timeout);
                                }
                            } else {
                                scheduler
                                    .buffers
                                    .entry(key)
                                    .or_default()
                                    .messages
                                    .push_back(BufferMessage {
                                        priority,
                                        sender: self.process_id,
                                        values: vec![value],
                                    });
                            }
                        }
                        continue;
                    }
                    let modes = self.lookup_signal_modes(signal).ok_or_else(|| {
                        Error::coded("E0505", *span, format!("unknown signal `{signal}`"))
                    })?;
                    if modes.len() != values.len() {
                        return Err(Error::coded(
                            "E0507",
                            *span,
                            format!(
                                "signal `{signal}` expects {} value(s), found {}",
                                modes.len(),
                                values.len()
                            ),
                        ));
                    }
                    let mut lowered = Vec::with_capacity(values.len());
                    for (value, mode) in values.iter().zip(modes) {
                        let value = self.eval_ir(value)?;
                        lowered.push(self.convert(value, mode, *span)?);
                    }
                    let target = match target {
                        Some(target) => match self.eval_ir(target)? {
                            Value::Instance(id) => Some(id),
                            _ => {
                                return Err(Error::coded(
                                    "E0507",
                                    *span,
                                    "SEND TO requires an INSTANCE value",
                                ))
                            }
                        },
                        None => None,
                    };
                    let allowed_process = self.lookup_signal_target(signal).flatten();
                    if let Some(target) = target {
                        let scheduler = self.scheduler.borrow();
                        if !scheduler.live_processes.contains(&target) {
                            return Err(Error::exception(
                                "EMPTY",
                                *span,
                                "EMPTY: SEND TO a process whose lifetime has ended",
                            ));
                        }
                        if let Some(allowed) = &allowed_process {
                            if scheduler.process_names.get(&target) != Some(allowed) {
                                return Err(Error::exception(
                                    "SENDFAIL",
                                    *span,
                                    "SENDFAIL: signal target has the wrong process name",
                                ));
                            }
                        }
                    }
                    let mut scheduler = self.scheduler.borrow_mut();
                    let receiver = scheduler.receivers.get(signal).and_then(|receivers| {
                        receivers
                            .iter()
                            .find(|(id, slot, _)| {
                                slot.borrow().is_some()
                                    && target.map(|target| target == *id).unwrap_or(true)
                                    && allowed_process.as_ref().is_none_or(|allowed| {
                                        scheduler.process_names.get(id) == Some(allowed)
                                    })
                            })
                            .map(|(id, slot, _)| (*id, Rc::clone(slot)))
                    });
                    if let Some((id, slot)) = receiver {
                        let entry = slot.borrow_mut().take().ok_or_else(|| {
                            Error::coded("E0500", *span, "internal: signal receiver disappeared")
                        })?;
                        for receivers in scheduler.receivers.values_mut() {
                            receivers.retain(|(receiver_id, _, _)| *receiver_id != id);
                        }
                        scheduler.ready.push_back(Runnable {
                            input: ProcessInput::Signal {
                                name: signal.clone(),
                                values: lowered,
                                sender: self.process_id,
                            },
                            entry,
                        });
                    } else {
                        scheduler
                            .signals
                            .entry(signal.clone())
                            .or_default()
                            .push_back(SignalMessage {
                                target,
                                allowed_process,
                                sender: self.process_id,
                                priority,
                                values: lowered,
                            });
                    }
                }
                ir::Stmt::ReceiveCase {
                    set,
                    alternatives,
                    else_actions,
                    span,
                } => {
                    let mut pending = None;
                    for alternative in alternatives {
                        {
                            let mut scheduler = self.scheduler.borrow_mut();
                            if let Some(queue) = scheduler.signals.get_mut(&alternative.signal) {
                                let mut selected: Option<usize> = None;
                                for (index, message) in queue.iter().enumerate() {
                                    let eligible = (message.target.is_none()
                                        || message.target == Some(self.process_id))
                                        && message
                                            .allowed_process
                                            .as_ref()
                                            .is_none_or(|allowed| allowed == &self.process_name);
                                    if eligible
                                        && selected
                                            .map(|selected| {
                                                message.priority > queue[selected].priority
                                            })
                                            .unwrap_or(true)
                                    {
                                        selected = Some(index);
                                    }
                                }
                                if let Some(index) = selected {
                                    let message = queue.remove(index).expect("signal message");
                                    pending = Some((
                                        alternative.signal.clone(),
                                        message.values,
                                        message.sender,
                                    ));
                                    break;
                                }
                            }
                        }
                        if let Ok((key, _, _)) = self.buffer_location(&alternative.signal, *span) {
                            if let Some((values, sender)) =
                                self.take_buffer_value(&alternative.signal, key)
                            {
                                pending = Some((alternative.signal.clone(), values, sender));
                                break;
                            }
                        }
                    }
                    let received = if let Some(received) = pending {
                        received
                    } else if let Some(else_actions) = else_actions {
                        return self.exec_stmts(else_actions, frame);
                    } else {
                        let mut signals = Vec::new();
                        let mut buffers = Vec::new();
                        for alternative in alternatives {
                            if self.lookup_signal_modes(&alternative.signal).is_some() {
                                signals.push(alternative.signal.clone());
                            } else {
                                let (key, _, _) =
                                    self.buffer_location(&alternative.signal, *span)?;
                                buffers.push(key);
                            }
                        }
                        match cooperative_receive(signals, buffers, self.deadline)? {
                            ProcessInput::Signal {
                                name,
                                values,
                                sender,
                            }
                            | ProcessInput::Buffer {
                                name,
                                values,
                                sender,
                            } => (name, values, sender),
                            ProcessInput::Timeout => return Ok(Flow::Timeout),
                            ProcessInput::Delay { .. }
                            | ProcessInput::TaskReady
                            | ProcessInput::None => {
                                return Err(Error::coded(
                                    "E0500",
                                    *span,
                                    "internal: receive resumed without a value",
                                ));
                            }
                        }
                    };
                    if let Some(set) = set {
                        self.assign_target(set, Value::Instance(received.2), *span)?;
                    }
                    let alternative = alternatives
                        .iter()
                        .find(|alternative| alternative.signal == received.0)
                        .ok_or_else(|| {
                            Error::coded("E0500", *span, "internal: received unknown alternative")
                        })?;
                    let modes = if let Some(modes) = self.lookup_signal_modes(&alternative.signal) {
                        modes
                    } else {
                        let (_, _, mode) = self.buffer_location(&alternative.signal, *span)?;
                        vec![mode]
                    };
                    self.locals.push(Scope::default());
                    let setup = (|| {
                        for ((name, mode), value) in alternative
                            .bindings
                            .iter()
                            .zip(modes.iter())
                            .zip(received.1)
                        {
                            let value = self.convert(value, *mode, *span)?;
                            self.locals.last_mut().expect("receive scope").vars.insert(
                                name.clone(),
                                Rc::new(RefCell::new(Var::new(*mode, value))),
                            );
                        }
                        self.exec_stmts(&alternative.actions, frame)
                    })();
                    self.locals.pop();
                    let flow = setup?;
                    if !matches!(flow, Flow::Normal) {
                        return Ok(flow);
                    }
                }
                ir::Stmt::Start {
                    process,
                    args,
                    passing,
                    targets,
                    span,
                } => {
                    let Some(process_ref) = self.lookup_process(process) else {
                        return Err(Error::coded(
                            "E0505",
                            *span,
                            format!("unknown process `{process}`"),
                        ));
                    };
                    let call_args = self.prepare_call_args(args, passing, targets, *span)?;
                    self.spawn_process(process_ref, call_args, *span)?;
                }
                ir::Stmt::Return { value, span } => {
                    if let Some(value) = value {
                        let result_type = frame.result_type.ok_or_else(|| {
                            Error::coded(
                                "E0508",
                                *span,
                                "RETURN with a value in a procedure that has no result",
                            )
                        })?;
                        let value = self.eval_ir(value)?;
                        frame.result = Some(self.convert(value, result_type, *span)?);
                    }
                    return Ok(Flow::Return);
                }
                ir::Stmt::Result { value, span } => {
                    let result_type = frame.result_type.ok_or_else(|| {
                        Error::coded("E0500", *span, "RESULT outside a value procedure")
                    })?;
                    let value = self.eval_ir(value)?;
                    frame.result = Some(self.convert(value, result_type, *span)?);
                }
                ir::Stmt::If {
                    branches,
                    else_actions,
                    else_span,
                    ..
                } => {
                    let flow = 'flow: {
                        for branch in branches {
                            let condition = self.eval_ir(&branch.condition)?;
                            if self.as_bool(condition, branch.condition.span())? {
                                break 'flow self.exec_block(&branch.body, branch.span, frame)?;
                            }
                        }
                        if let (Some(actions), Some(span)) = (else_actions, else_span) {
                            break 'flow self.exec_block(actions, *span, frame)?;
                        }
                        Flow::Normal
                    };
                    if !matches!(flow, Flow::Normal) {
                        return Ok(flow);
                    }
                }
                ir::Stmt::Do {
                    control,
                    body,
                    span,
                } => {
                    let flow = self.exec_do(control, body, frame, *span)?;
                    if !matches!(flow, Flow::Normal) {
                        return Ok(flow);
                    }
                }
                ir::Stmt::Exit { label, .. } => return Ok(Flow::Exit(label.clone())),
                ir::Stmt::Case {
                    selectors,
                    alternatives,
                    else_actions,
                    else_span,
                    ..
                } => {
                    let mut values = Vec::with_capacity(selectors.len());
                    for selector in selectors {
                        values.push(self.eval_ir(selector)?);
                    }

                    let explicitly_labelled = alternatives.iter().any(|alternative| {
                        alternative.labels.len() == values.len()
                            && alternative.labels.iter().zip(values.iter()).all(
                                |(labels, value)| {
                                    labels
                                        .iter()
                                        .any(|label| explicit_label_matches(*label, value))
                                },
                            )
                    });

                    let mut matched = false;
                    for alternative in alternatives {
                        if alternative.labels.len() != values.len() {
                            return Err(Error::coded(
                                "E0500",
                                alternative.span,
                                "internal: CASE alternative has the wrong number of label lists",
                            ));
                        }
                        let matches =
                            alternative
                                .labels
                                .iter()
                                .zip(values.iter())
                                .all(|(labels, value)| {
                                    labels.iter().any(|label| match label {
                                        CaseLabel::Else => !explicitly_labelled,
                                        CaseLabel::Any => true,
                                        other => explicit_label_matches(*other, value),
                                    })
                                });
                        if matches {
                            matched = true;
                            let flow =
                                self.exec_block(&alternative.body, alternative.span, frame)?;
                            if !matches!(flow, Flow::Normal) {
                                return Ok(flow);
                            }
                            break;
                        }
                    }

                    if !matched {
                        if let (Some(actions), Some(span)) = (else_actions, else_span) {
                            let flow = self.exec_block(actions, *span, frame)?;
                            if !matches!(flow, Flow::Normal) {
                                return Ok(flow);
                            }
                        }
                    }
                }
                ir::Stmt::Labelled { label, action, .. } => {
                    let flow = self.exec_stmts(std::slice::from_ref(action.as_ref()), frame)?;
                    match flow {
                        Flow::Exit(Some(name)) if &name == label => {}
                        Flow::Normal => {}
                        flow => return Ok(flow),
                    }
                }
                ir::Stmt::Begin {
                    declarations,
                    actions,
                    span,
                } => {
                    self.locals.push(Scope::default());
                    let result = (|| {
                        self.declare_local_vars(declarations)?;
                        self.initialize_declarations(declarations)?;
                        let mut scope = self.locals.pop().expect("begin scope");
                        let mode_names = self.module_modes[self.active_module].clone();
                        self.hoist_block_children(*span, &mut scope, &mode_names)?;
                        self.seed_reach_exports(*span, &mut scope);
                        self.locals.push(scope);
                        self.exec_with_handler(*span, actions, frame)
                    })();
                    self.locals.pop();
                    let flow = match result {
                        Ok(Flow::Exit(None)) => Flow::Normal,
                        Ok(flow) => flow,
                        Err(error) => return Err(error),
                    };
                    if !matches!(flow, Flow::Normal) {
                        return Ok(flow);
                    }
                }
                ir::Stmt::Empty { .. } => {}
                ir::Stmt::Cause { name, span } => {
                    return Err(Error::raised(name, *span));
                }
                ir::Stmt::Module {
                    label,
                    declarations,
                    actions,
                    span,
                } => {
                    let flow =
                        self.exec_nested_module(label.as_deref(), declarations, actions, *span)?;
                    if !matches!(flow, Flow::Normal) {
                        return Ok(flow);
                    }
                }
                ir::Stmt::Handled { action, span } => {
                    let flow = self.exec_with_handler(
                        *span,
                        std::slice::from_ref(action.as_ref()),
                        frame,
                    )?;
                    if !matches!(flow, Flow::Normal) {
                        return Ok(flow);
                    }
                }
            }
        }
        Ok(Flow::Normal)
    }

    fn resolve_text_width(
        &mut self,
        width: crate::builtins::ClauseWidth,
        io: &[ir::TextIoArg],
        io_index: &mut usize,
        span: Span,
    ) -> Result<usize, Error> {
        match width {
            crate::builtins::ClauseWidth::Fixed(width) => Ok(width),
            crate::builtins::ClauseWidth::Variable => {
                let Some(ir::TextIoArg::Value(argument)) = io.get(*io_index) else {
                    return Err(textfail(span, "missing variable width value"));
                };
                *io_index += 1;
                let value = self.eval_ir(argument)?;
                let (value, _) = self.expect_int(value, argument.span())?;
                if value < 0 {
                    return Err(textfail(span, "negative variable width"));
                }
                Ok(value as usize)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_association(
        &mut self,
        op: crate::builtins::AssociationOp,
        first: &str,
        first_ref: Option<&ir::Node>,
        second: Option<&str>,
        index: Option<&ir::Node>,
        usage_node: Option<&ir::Node>,
        where_node: Option<&ir::Node>,
        value: Option<&ir::Node>,
        ty: Option<Type>,
        span: Span,
    ) -> Result<Value, Error> {
        use crate::builtins::AssociationOp;
        let first_cell = self.transfer_cell(first, first_ref, span)?;
        match op {
            AssociationOp::Associate => {
                let Some(value) = value else {
                    return Err(Error::coded("E0507", span, "ASSOCIATE requires a path"));
                };
                let path = self.eval_ir(value)?;
                let Value::String(path) = path else {
                    return Err(Error::coded("E0500", span, "ASSOCIATE path must be CHARS"));
                };
                let cell = self.find_var_cell(first).ok_or_else(|| {
                    Error::coded("E0505", span, format!("unknown variable `{first}`"))
                })?;
                let mut var = cell.borrow_mut();
                match &var.value {
                    Value::Association(None) => {}
                    Value::Association(Some(_)) => {
                        return Err(Error::exception(
                            "ASSOCIATEFAIL",
                            span,
                            "ASSOCIATEFAIL: association already exists",
                        ));
                    }
                    _ => {
                        return Err(Error::coded(
                            "E0507",
                            span,
                            "expected an ASSOCIATION location",
                        ))
                    }
                }
                let existing = std::path::Path::new(&path).exists();
                var.value = Value::Association(Some(Rc::new(RefCell::new(AssociationState {
                    path,
                    existing,
                    indexable: false,
                }))));
                Ok(Value::Undefined)
            }
            AssociationOp::IsAssociated => {
                let cell = self.find_var_cell(first).ok_or_else(|| {
                    Error::coded("E0505", span, format!("unknown variable `{first}`"))
                })?;
                let var = cell.borrow();
                let Value::Association(state) = &var.value else {
                    return Err(Error::coded(
                        "E0507",
                        span,
                        "expected an ASSOCIATION location",
                    ));
                };
                Ok(Value::Bool(state.is_some()))
            }
            AssociationOp::Existing
            | AssociationOp::Readable
            | AssociationOp::Writeable
            | AssociationOp::Indexable
            | AssociationOp::Sequencible
            | AssociationOp::Variable => {
                let state = self.association_state(first, span)?;
                let state = state.borrow();
                let value = match op {
                    AssociationOp::Existing => state.existing,
                    AssociationOp::Readable => state.existing,
                    AssociationOp::Writeable => state.existing,
                    AssociationOp::Indexable => state.indexable,
                    AssociationOp::Sequencible => true,
                    AssociationOp::Variable => false,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(value))
            }
            AssociationOp::Modify => {
                let _ = self.association_state(first, span)?;
                Ok(Value::Undefined)
            }
            AssociationOp::Dissociate => {
                let cell = self.find_var_cell(first).ok_or_else(|| {
                    Error::coded("E0505", span, format!("unknown variable `{first}`"))
                })?;
                let mut var = cell.borrow_mut();
                let Value::Association(state) = &mut var.value else {
                    return Err(Error::coded(
                        "E0507",
                        span,
                        "expected an ASSOCIATION location",
                    ));
                };
                if state.take().is_none() {
                    return Err(Error::exception(
                        "NOTASSOCIATED",
                        span,
                        "NOTASSOCIATED: empty association",
                    ));
                }
                Ok(Value::Undefined)
            }
            AssociationOp::Create => {
                let state = self.association_state(first, span)?;
                if state.borrow().existing {
                    return Err(Error::exception(
                        "CREATEFAIL",
                        span,
                        "CREATEFAIL: file already exists",
                    ));
                }
                let path = state.borrow().path.clone();
                std::fs::write(&path, "").map_err(|error| {
                    Error::exception("CREATEFAIL", span, format!("CREATEFAIL: {error}"))
                })?;
                state.borrow_mut().existing = true;
                Ok(Value::Undefined)
            }
            AssociationOp::Delete => {
                let state = self.association_state(first, span)?;
                if !state.borrow().existing {
                    return Err(Error::exception(
                        "DELETEFAIL",
                        span,
                        "DELETEFAIL: file does not exist",
                    ));
                }
                let path = state.borrow().path.clone();
                std::fs::remove_file(&path).map_err(|error| {
                    Error::exception("DELETEFAIL", span, format!("DELETEFAIL: {error}"))
                })?;
                state.borrow_mut().existing = false;
                Ok(Value::Undefined)
            }
            AssociationOp::Connect => {
                let Some(second) = second else {
                    return Err(Error::coded(
                        "E0507",
                        span,
                        "CONNECT requires an association",
                    ));
                };
                let association = self.association_state(second, span)?;
                if !association.borrow().existing {
                    return Err(Error::exception(
                        "CONNECTFAIL",
                        span,
                        "CONNECTFAIL: association has no existing file",
                    ));
                }
                let usage = match usage_node {
                    Some(node) => {
                        let value = self.eval_ir(node)?;
                        let Value::Set(value) = value else {
                            return Err(Error::coded(
                                "E0500",
                                span,
                                "CONNECT usage must be a USAGE value",
                            ));
                        };
                        value
                    }
                    None => USAGE_READWRITE,
                };
                let where_value = match where_node {
                    Some(node) => {
                        let value = self.eval_ir(node)?;
                        let Value::Set(value) = value else {
                            return Err(Error::coded(
                                "E0500",
                                span,
                                "CONNECT where must be a WHERE value",
                            ));
                        };
                        value
                    }
                    None => WHERE_FIRST,
                };
                let cell = first_cell.clone();
                let (indexed, index_range, is_text, previous_position) = {
                    let var = cell.borrow();
                    match var.ty {
                        Type::Access(access) => {
                            let previous_position = match &var.value {
                                Value::Access(None) => None,
                                Value::Access(Some(state)) => Some(state.borrow().position),
                                _ => {
                                    return Err(Error::coded(
                                        "E0507",
                                        span,
                                        "expected an ACCESS location",
                                    ))
                                }
                            };
                            (access.indexed, access.index, false, previous_position)
                        }
                        Type::Text { indexed, index, .. } => {
                            let previous_position = var.text.as_ref().and_then(|text| {
                                let access = text.borrow().access.clone();
                                let access = access.borrow();
                                match &access.value {
                                    Value::Access(Some(state)) => Some(state.borrow().position),
                                    _ => None,
                                }
                            });
                            (indexed, index, true, previous_position)
                        }
                        _ => {
                            return Err(Error::coded(
                                "E0507",
                                span,
                                "expected an ACCESS or TEXT location",
                            ))
                        }
                    }
                };
                let path = association.borrow().path.clone();
                let contents = std::fs::read_to_string(&path).map_err(|error| {
                    Error::exception("READFAIL", span, format!("READFAIL: {error}"))
                })?;
                let file_len = contents.lines().count() as i128;
                let base = match where_value {
                    WHERE_FIRST => 0,
                    WHERE_SAME => previous_position.ok_or_else(|| {
                        Error::exception(
                            "CONNECTFAIL",
                            span,
                            "CONNECTFAIL: SAME requires an existing connection",
                        )
                    })?,
                    WHERE_LAST => file_len,
                    _ => return Err(Error::coded("E0500", span, "internal: invalid WHERE value")),
                };
                let position = match index {
                    Some(index) => {
                        let value = self.eval_ir(index)?;
                        let (value, _) = self.expect_int(value, index.span())?;
                        check_index_range(index_range, value, span)?;
                        let lower = index_range.map(|range| range.low).unwrap_or(0);
                        base + value - lower
                    }
                    None => base,
                };
                let mut var = cell.borrow_mut();
                if !is_text {
                    association.borrow_mut().indexable = indexed;
                }
                let state = Rc::new(RefCell::new(AccessState {
                    association: Some(association),
                    association_name: Some(second.to_owned()),
                    indexed,
                    index_range,
                    position,
                    usage,
                    out_of_file: false,
                }));
                if is_text {
                    let text = var.text.clone().expect("text location has text state");
                    let access = text.borrow().access.clone();
                    access.borrow_mut().value = Value::Access(Some(state));
                } else {
                    var.value = Value::Access(Some(state));
                }
                Ok(Value::Undefined)
            }
            AssociationOp::Disconnect => {
                let mut var = first_cell.borrow_mut();
                let is_text = matches!(var.ty, Type::Text { .. });
                let disconnected = if is_text {
                    let text = var.text.clone().expect("text location has text state");
                    let access = text.borrow().access.clone();
                    let mut access = access.borrow_mut();
                    match &mut access.value {
                        Value::Access(state) => state.take().is_some(),
                        _ => {
                            return Err(Error::coded(
                                "E0507",
                                span,
                                "expected an ACCESS or TEXT location",
                            ))
                        }
                    }
                } else {
                    match &mut var.value {
                        Value::Access(state) => state.take().is_some(),
                        _ => {
                            return Err(Error::coded(
                                "E0507",
                                span,
                                "expected an ACCESS or TEXT location",
                            ))
                        }
                    }
                };
                if !disconnected {
                    return Err(Error::exception(
                        "NOTCONNECTED",
                        span,
                        "NOTCONNECTED: access location is not connected",
                    ));
                }
                Ok(Value::Undefined)
            }
            AssociationOp::GetAssociation => {
                let cell = first_cell.clone();
                let association_name = {
                    let var = cell.borrow();
                    match &var.value {
                        Value::Access(state) => state
                            .as_ref()
                            .and_then(|state| state.borrow().association_name.clone()),
                        _ if matches!(var.ty, Type::Text { .. }) => {
                            var.text.as_ref().and_then(|text| {
                                let access = text.borrow().access.clone();
                                let access = access.borrow();
                                match &access.value {
                                    Value::Access(Some(state)) => {
                                        state.borrow().association_name.clone()
                                    }
                                    _ => None,
                                }
                            })
                        }
                        _ => {
                            return Err(Error::coded(
                                "E0507",
                                span,
                                "expected an ACCESS or TEXT location",
                            ))
                        }
                    }
                };
                let Some(association_name) = association_name else {
                    return Ok(Value::Ref(None));
                };
                let association_cell = self.find_var_cell(&association_name).ok_or_else(|| {
                    Error::coded(
                        "E0505",
                        span,
                        format!("unknown association variable `{association_name}`"),
                    )
                })?;
                Ok(Value::Ref(Some(self.intern_ref(association_cell, false))))
            }
            AssociationOp::GetUsage => {
                let access = self.access_state_cell(&first_cell, span)?;
                let usage = access.borrow().usage;
                Ok(Value::Set(usage))
            }
            AssociationOp::OutOfFile => {
                let access = self.access_state_cell(&first_cell, span)?;
                let out_of_file = access.borrow().out_of_file;
                Ok(Value::Bool(out_of_file))
            }
            AssociationOp::ReadRecord => {
                let access = self.access_state(first, span)?;
                let (association, indexed, range, current, usage, out_of_file) = {
                    let access = access.borrow();
                    let Some(association) = access.association.clone() else {
                        return Err(Error::exception(
                            "NOTCONNECTED",
                            span,
                            "NOTCONNECTED: access location is not connected",
                        ));
                    };
                    (
                        association,
                        access.indexed,
                        access.index_range,
                        access.position,
                        access.usage,
                        access.out_of_file,
                    )
                };
                if usage == USAGE_WRITEONLY {
                    return Err(Error::exception(
                        "READFAIL",
                        span,
                        "READFAIL: access location is write-only",
                    ));
                }
                if !indexed && out_of_file {
                    return Err(Error::exception(
                        "READFAIL",
                        span,
                        "READFAIL: access location is at end of file",
                    ));
                }
                let position = match index {
                    Some(index) => {
                        let value = self.eval_ir(index)?;
                        let (value, _) = self.expect_int(value, index.span())?;
                        check_index_range(range, value, span)?;
                        value
                    }
                    None => current,
                };
                if indexed {
                    check_index_range(range, position, span)?;
                }
                if position < 0 {
                    return Err(Error::exception(
                        "RANGEFAIL",
                        span,
                        "RANGEFAIL: negative record index",
                    ));
                }
                let path = association.borrow().path.clone();
                let contents = std::fs::read_to_string(&path).map_err(|error| {
                    Error::exception("READFAIL", span, format!("READFAIL: {error}"))
                })?;
                let lines: Vec<&str> = contents.lines().collect();
                let Some(record) = lines.get(position as usize) else {
                    access.borrow_mut().out_of_file = true;
                    return Ok(Value::Undefined);
                };
                {
                    let mut access = access.borrow_mut();
                    access.position = position + 1;
                    access.out_of_file = false;
                }
                let record = Value::String((*record).to_owned());
                if let Some(target) = second {
                    self.assign_var(target, record, span)?;
                    Ok(Value::Undefined)
                } else {
                    let _ = ty;
                    Ok(record)
                }
            }
            AssociationOp::WriteRecord => {
                let Some(value) = value else {
                    return Err(Error::coded("E0507", span, "WRITERECORD requires a value"));
                };
                let value = self.eval_ir(value)?;
                let Value::String(record) = value else {
                    return Err(Error::coded(
                        "E0500",
                        span,
                        "WRITERECORD value must be CHARS",
                    ));
                };
                let access = self.access_state(first, span)?;
                let (association, indexed, range, current, usage) = {
                    let access = access.borrow();
                    let Some(association) = access.association.clone() else {
                        return Err(Error::exception(
                            "NOTCONNECTED",
                            span,
                            "NOTCONNECTED: access location is not connected",
                        ));
                    };
                    (
                        association,
                        access.indexed,
                        access.index_range,
                        access.position,
                        access.usage,
                    )
                };
                if usage == USAGE_READONLY {
                    return Err(Error::exception(
                        "WRITEFAIL",
                        span,
                        "WRITEFAIL: access location is read-only",
                    ));
                }
                let path = association.borrow().path.clone();
                if indexed || index.is_some() {
                    let position = match index {
                        Some(index) => {
                            let value = self.eval_ir(index)?;
                            let (value, _) = self.expect_int(value, index.span())?;
                            value
                        }
                        None => current,
                    };
                    check_index_range(range, position, span)?;
                    if position < 0 {
                        return Err(Error::exception(
                            "RANGEFAIL",
                            span,
                            "RANGEFAIL: negative record index",
                        ));
                    }
                    write_line_at(&path, position as usize, &record).map_err(|error| {
                        Error::exception("WRITEFAIL", span, format!("WRITEFAIL: {error}"))
                    })?;
                    access.borrow_mut().position = position + 1;
                } else {
                    use std::io::Write as _;
                    let mut file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .map_err(|error| {
                            Error::exception("WRITEFAIL", span, format!("WRITEFAIL: {error}"))
                        })?;
                    writeln!(file, "{record}").map_err(|error| {
                        Error::exception("WRITEFAIL", span, format!("WRITEFAIL: {error}"))
                    })?;
                }
                Ok(Value::Undefined)
            }
        }
    }

    fn association_state(
        &self,
        name: &str,
        span: Span,
    ) -> Result<Rc<RefCell<AssociationState>>, Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::coded("E0505", span, format!("unknown variable `{name}`")))?;
        let var = cell.borrow();
        let Value::Association(Some(state)) = &var.value else {
            return Err(Error::exception(
                "NOTASSOCIATED",
                span,
                "NOTASSOCIATED: empty association",
            ));
        };
        Ok(state.clone())
    }

    fn transfer_cell(
        &mut self,
        first: &str,
        first_ref: Option<&ir::Node>,
        span: Span,
    ) -> Result<VarCell, Error> {
        if let Some(reference) = first_ref {
            return self.eval_reference(reference, span);
        }
        self.find_var_cell(first)
            .ok_or_else(|| Error::coded("E0505", span, format!("unknown variable `{first}`")))
    }

    fn access_state_cell(
        &self,
        cell: &VarCell,
        span: Span,
    ) -> Result<Rc<RefCell<AccessState>>, Error> {
        let var = cell.borrow();
        let state = match &var.value {
            Value::Access(state) => state.clone(),
            _ if matches!(var.ty, Type::Text { .. }) => {
                let access = var
                    .text
                    .as_ref()
                    .map(|text| text.borrow().access.clone())
                    .ok_or_else(|| {
                        Error::coded("E0507", span, "text location has no access state")
                    })?;
                let access = access.borrow();
                match &access.value {
                    Value::Access(state) => state.clone(),
                    _ => {
                        return Err(Error::coded(
                            "E0507",
                            span,
                            "expected an ACCESS or TEXT location",
                        ))
                    }
                }
            }
            _ => {
                return Err(Error::coded(
                    "E0507",
                    span,
                    "expected an ACCESS or TEXT location",
                ))
            }
        };
        state.ok_or_else(|| {
            Error::exception(
                "NOTCONNECTED",
                span,
                "NOTCONNECTED: access location is not connected",
            )
        })
    }

    fn access_state(&self, name: &str, span: Span) -> Result<Rc<RefCell<AccessState>>, Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::coded("E0505", span, format!("unknown variable `{name}`")))?;
        let var = cell.borrow();
        let state = match &var.value {
            Value::Access(state) => state.clone(),
            _ if matches!(var.ty, Type::Text { .. }) => {
                let access = var
                    .text
                    .as_ref()
                    .map(|text| text.borrow().access.clone())
                    .ok_or_else(|| {
                        Error::coded("E0507", span, "text location has no access state")
                    })?;
                let access = access.borrow();
                match &access.value {
                    Value::Access(state) => state.clone(),
                    _ => {
                        return Err(Error::coded(
                            "E0507",
                            span,
                            "expected an ACCESS or TEXT location",
                        ))
                    }
                }
            }
            _ => {
                return Err(Error::coded(
                    "E0507",
                    span,
                    "expected an ACCESS or TEXT location",
                ))
            }
        };
        state.ok_or_else(|| {
            Error::exception(
                "NOTCONNECTED",
                span,
                "NOTCONNECTED: access location is not connected",
            )
        })
    }

    fn read_text_record(
        &mut self,
        access: &Rc<RefCell<AccessState>>,
        index: Option<i128>,
        span: Span,
    ) -> Result<Option<String>, Error> {
        let (association, position, usage, out_of_file, indexed, index_range) = {
            let access = access.borrow();
            let Some(association) = access.association.clone() else {
                return Err(Error::exception(
                    "NOTCONNECTED",
                    span,
                    "NOTCONNECTED: text location is not connected",
                ));
            };
            (
                association,
                access.position,
                access.usage,
                access.out_of_file,
                access.indexed,
                access.index_range,
            )
        };
        let position = match index {
            Some(value) => {
                if !indexed {
                    return Err(textfail(span, "indexed transfer on sequential access"));
                }
                check_index_range(index_range, value, span)?;
                value
            }
            None => position,
        };
        if usage == USAGE_WRITEONLY {
            return Err(Error::exception(
                "READFAIL",
                span,
                "READFAIL: text access location is write-only",
            ));
        }
        if out_of_file && index.is_none() {
            return Err(Error::exception(
                "READFAIL",
                span,
                "READFAIL: text access location is at end of file",
            ));
        }
        if position < 0 {
            return Err(Error::exception(
                "RANGEFAIL",
                span,
                "RANGEFAIL: negative record index",
            ));
        }
        let path = association.borrow().path.clone();
        let contents = std::fs::read_to_string(&path)
            .map_err(|error| Error::exception("READFAIL", span, format!("READFAIL: {error}")))?;
        let lines: Vec<&str> = contents.lines().collect();
        let Some(record) = lines.get(position as usize) else {
            access.borrow_mut().out_of_file = true;
            return Ok(None);
        };
        {
            let mut access = access.borrow_mut();
            access.position = position + 1;
            access.out_of_file = false;
        }
        Ok(Some((*record).to_owned()))
    }

    fn write_text_record(
        &mut self,
        access: &Rc<RefCell<AccessState>>,
        index: Option<i128>,
        record: &str,
        span: Span,
    ) -> Result<(), Error> {
        let (association, position, usage, indexed, index_range) = {
            let access = access.borrow();
            let Some(association) = access.association.clone() else {
                return Err(Error::exception(
                    "NOTCONNECTED",
                    span,
                    "NOTCONNECTED: text location is not connected",
                ));
            };
            (
                association,
                access.position,
                access.usage,
                access.indexed,
                access.index_range,
            )
        };
        let position = match index {
            Some(value) => {
                if !indexed {
                    return Err(textfail(span, "indexed transfer on sequential access"));
                }
                check_index_range(index_range, value, span)?;
                value
            }
            None => position,
        };
        if usage == USAGE_READONLY {
            return Err(Error::exception(
                "WRITEFAIL",
                span,
                "WRITEFAIL: text access location is read-only",
            ));
        }
        if position < 0 {
            return Err(Error::exception(
                "RANGEFAIL",
                span,
                "RANGEFAIL: negative record index",
            ));
        }
        let path = association.borrow().path.clone();
        write_line_at(&path, position as usize, record)
            .map_err(|error| Error::exception("WRITEFAIL", span, format!("WRITEFAIL: {error}")))?;
        access.borrow_mut().position = position + 1;
        Ok(())
    }

    fn exec_block(
        &mut self,
        stmts: &[ir::Stmt],
        span: Span,
        frame: &mut Frame,
    ) -> Result<Flow, Error> {
        let mut scope = Scope::default();
        let mode_names = self.module_modes[self.active_module].clone();
        self.hoist_block_children(span, &mut scope, &mode_names)?;
        self.seed_reach_exports(span, &mut scope);
        self.locals.push(scope);
        let result = self.exec_stmts(stmts, frame);
        self.locals.pop();
        result
    }

    fn hoist_block_children(
        &mut self,
        span: Span,
        parent: &mut Scope<'a>,
        base_modes: &HashMap<String, Type>,
    ) -> Result<(), Error> {
        let top = self.active_module;
        self.hoist_reach_children(span, parent, top, base_modes, true)
    }

    fn exec_nested_module(
        &mut self,
        label: Option<&str>,
        declarations: &[Declaration],
        actions: &[ir::Stmt],
        span: Span,
    ) -> Result<Flow, Error> {
        if let Some(scope) = self.nested_scopes.remove(&span) {
            return self.exec_hoisted_module(label, declarations, actions, span, scope);
        }

        let parent = self.current_reach();
        self.locals.push(Scope {
            modes: self
                .reaches
                .get(span)
                .map(|node| node.interface.modes.clone())
                .unwrap_or_default(),
            key: Some(span),
            ..Scope::default()
        });
        let previous_floor = self.module_floor;
        let previous_scope = self.active_scope_key;
        self.module_floor = self.locals.len() - 1;
        self.active_scope_key = Some(span);

        let result = (|| {
            self.apply_nested_seizes(declarations, &parent)?;
            self.declare_local_vars(declarations)?;
            self.initialize_declarations(declarations)?;

            let mut frame = Frame {
                result_type: None,
                result: None,
            };
            match self.exec_with_handler(span, actions, &mut frame)? {
                Flow::Normal | Flow::Exit(None) => Ok(Flow::Normal),
                Flow::Exit(Some(name)) if label == Some(name.as_str()) => Ok(Flow::Normal),
                flow => Ok(flow),
            }
        })();

        self.module_floor = previous_floor;
        self.active_scope_key = previous_scope;
        self.locals.pop();
        result
    }

    fn exec_hoisted_module(
        &mut self,
        label: Option<&str>,
        declarations: &[Declaration],
        actions: &[ir::Stmt],
        span: Span,
        scope: Scope<'a>,
    ) -> Result<Flow, Error> {
        let parent = self.current_reach();
        self.locals.push(scope);
        let previous_floor = self.module_floor;
        let previous_scope = self.active_scope_key;
        self.module_floor = self.locals.len() - 1;
        self.active_scope_key = Some(span);

        let result = (|| {
            if !self.locals.last().expect("hoisted scope").seizes_applied {
                self.apply_nested_seizes(declarations, &parent)?;
                self.locals
                    .last_mut()
                    .expect("hoisted scope")
                    .seizes_applied = true;
            }
            self.initialize_declarations(declarations)?;

            let mut frame = Frame {
                result_type: None,
                result: None,
            };
            match self.exec_with_handler(span, actions, &mut frame)? {
                Flow::Normal | Flow::Exit(None) => Ok(Flow::Normal),
                Flow::Exit(Some(name)) if label == Some(name.as_str()) => Ok(Flow::Normal),
                flow => Ok(flow),
            }
        })();

        self.module_floor = previous_floor;
        self.active_scope_key = previous_scope;
        let scope = self.locals.pop().expect("hoisted scope");
        self.nested_scopes.insert(span, scope);
        result
    }

    fn current_reach(&self) -> Scope<'a> {
        if let Some(scope) = self.locals.last() {
            scope.clone()
        } else {
            self.modules[self.active_module].clone()
        }
    }

    fn apply_nested_seizes(
        &mut self,
        declarations: &[Declaration],
        parent: &Scope<'a>,
    ) -> Result<(), Error> {
        let package = self
            .package_names
            .get(self.active_module)
            .and_then(|name| name.clone());
        let module_name = self
            .active_scope_key
            .and_then(|key| self.modules_by_span.get(&key))
            .and_then(|module| module.name.clone());

        for declaration in declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Seize {
                continue;
            }
            let old_prefix = program::seize_old_prefix(&visibility.prefix)
                .map_err(|message| Error::coded("E0500", visibility.span, message))?;

            if visibility.all {
                if old_prefix.is_empty() {
                    for (name, cell) in &parent.vars {
                        self.insert_nested_var(name, cell.clone(), visibility.span)?;
                    }
                    for (name, proc_ref) in &parent.procs {
                        self.insert_nested_proc(name, *proc_ref, visibility.span)?;
                    }
                    for (name, ty) in &parent.modes {
                        self.insert_nested_mode(name, *ty, visibility.span)?;
                    }
                }
                for (name, exports) in &parent.exports {
                    let matching: Vec<&ReachBinding<'a>> = exports
                        .iter()
                        .filter(|binding| {
                            binding.meta.matches(
                                old_prefix,
                                package.as_deref(),
                                module_name.as_deref(),
                            )
                        })
                        .collect();
                    match matching.as_slice() {
                        [] => {}
                        [export] => {
                            self.insert_reach_binding(name, (*export).clone(), visibility.span)?
                        }
                        _ => {
                            return Err(Error::coded("E0500", 
                                visibility.span,
                                format!(
                                    "SEIZE ALL is ambiguous for `{name}`; several sibling modules grant it"
                                ),
                            ));
                        }
                    }
                }
                continue;
            }

            for name in &visibility.names {
                let mut source_prefix = old_prefix.to_vec();
                source_prefix.extend(name.prefix.iter().cloned());

                if source_prefix.is_empty() {
                    if let Some(cell) = parent.vars.get(&name.name) {
                        self.insert_nested_var(&name.name, cell.clone(), name.span)?;
                        continue;
                    }
                    if let Some(proc_ref) = parent.procs.get(&name.name) {
                        self.insert_nested_proc(&name.name, *proc_ref, name.span)?;
                        continue;
                    }
                    if let Some(ty) = parent.modes.get(&name.name) {
                        self.insert_nested_mode(&name.name, *ty, name.span)?;
                        continue;
                    }
                }

                let Some(exports) = parent.exports.get(&name.name) else {
                    return Err(Error::coded(
                        "E0508",
                        name.span,
                        format!(
                            "cannot SEIZE `{}`: no such name in the enclosing reach",
                            name.qualified()
                        ),
                    ));
                };
                let selected = crate::reach::select(
                    exports.iter().map(|binding| (&binding.meta, &binding.kind)),
                    &source_prefix,
                    package.as_deref(),
                    module_name.as_deref(),
                );
                match selected {
                    Ok(kind) => {
                        let binding = ReachBinding {
                            meta: crate::reach::ExportMeta::default(),
                            kind: kind.clone(),
                        };
                        self.insert_reach_binding(&name.name, binding, name.span)?;
                    }
                    Err(crate::reach::SelectError::NotFound) => {
                        return Err(Error::coded(
                            "E0508",
                            name.span,
                            format!(
                                "cannot SEIZE `{}`: no such name in the enclosing reach",
                                name.qualified()
                            ),
                        ));
                    }
                    Err(crate::reach::SelectError::Ambiguous) => {
                        return Err(Error::coded(
                            "E0500",
                            name.span,
                            format!(
                                "SEIZE `{}` is ambiguous; several sibling modules grant it",
                                name.qualified()
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn insert_nested_var(&mut self, name: &str, cell: VarCell, span: Span) -> Result<(), Error> {
        let scope = self.locals.last_mut().expect("nested module scope");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.modes.contains_key(name)
        {
            return Err(Error::coded(
                "E0508",
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.vars.insert(name.to_owned(), cell);
        Ok(())
    }

    fn insert_nested_proc(
        &mut self,
        name: &str,
        proc_ref: ProcRef<'a>,
        span: Span,
    ) -> Result<(), Error> {
        let scope = self.locals.last_mut().expect("nested module scope");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.modes.contains_key(name)
        {
            return Err(Error::coded(
                "E0508",
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.procs.insert(name.to_owned(), proc_ref);
        Ok(())
    }

    fn insert_nested_mode(&mut self, name: &str, ty: Type, span: Span) -> Result<(), Error> {
        let scope = self.locals.last_mut().expect("nested module scope");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.modes.contains_key(name)
        {
            return Err(Error::coded(
                "E0508",
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.modes.insert(name.to_owned(), ty);
        Ok(())
    }

    fn insert_reach_binding(
        &mut self,
        name: &str,
        binding: ReachBinding<'a>,
        span: Span,
    ) -> Result<(), Error> {
        let scope = self.locals.last_mut().expect("nested module scope");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.modes.contains_key(name)
            || scope.exports.contains_key(name)
        {
            return Err(Error::coded(
                "E0508",
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        match binding.kind {
            ReachKind::Var(cell) => {
                scope.vars.insert(name.to_owned(), cell);
            }
            ReachKind::Proc(proc_ref) => {
                scope.procs.insert(name.to_owned(), proc_ref);
            }
            ReachKind::Mode(ty) => {
                scope.modes.insert(name.to_owned(), ty);
            }
        }
        Ok(())
    }

    fn exec_do(
        &mut self,
        control: &ir::DoControlIr,
        body: &[ir::Stmt],
        frame: &mut Frame,
        span: Span,
    ) -> Result<Flow, Error> {
        match control {
            ir::DoControlIr::None => match self.exec_block(body, span, frame)? {
                Flow::Exit(None) => Ok(Flow::Normal),
                flow => Ok(flow),
            },
            ir::DoControlIr::While(condition) => {
                loop {
                    let value = self.eval_ir(condition)?;
                    if !self.as_bool(value, condition.span())? {
                        break;
                    }
                    match self.exec_block(body, span, frame)? {
                        Flow::Normal => {}
                        Flow::Exit(None) => break,
                        flow => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            ir::DoControlIr::ForEver => {
                loop {
                    match self.exec_block(body, span, frame)? {
                        Flow::Normal => {}
                        Flow::Exit(None) => break,
                        flow => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            ir::DoControlIr::For {
                counter,
                start,
                end,
                step,
                down,
            } => {
                let start_value = self.eval_ir(start)?;
                let end_value = self.eval_ir(end)?;
                let step_value = match step {
                    Some(expr) => self.eval_ir(expr)?,
                    None => Value::Int {
                        value: 1,
                        kind: None,
                    },
                };
                let step_span = step.as_ref().map(|expr| expr.span()).unwrap_or(span);

                let (start_value, start_kind) = self.expect_int(start_value, start.span())?;
                let (end_value, end_kind) = self.expect_int(end_value, end.span())?;
                let (step_value, step_kind) = self.expect_int(step_value, step_span)?;

                let kind = combine_kinds(start_kind, end_kind, span)?;
                let kind = combine_kinds(kind, step_kind, span)?;

                if step_value <= 0 {
                    return Err(Error::coded(
                        "E0500",
                        step_span,
                        "DO FOR step must be positive",
                    ));
                }

                self.make_int(start_value, kind, start.span())?;
                self.make_int(end_value, kind, end.span())?;
                self.make_int(step_value, kind, step_span)?;

                let counter_ty = match kind {
                    Some(kind) => Type::Int(kind),
                    None => Type::IntLiteral,
                };

                self.locals.push(Scope::default());
                let result = (|| -> Result<Flow, Error> {
                    {
                        let scope = self.locals.last_mut().expect("local scope");
                        scope.vars.insert(
                            counter.clone(),
                            Rc::new(RefCell::new(Var::new(
                                counter_ty,
                                Value::Int {
                                    value: start_value,
                                    kind,
                                },
                            ))),
                        );
                    }

                    {
                        let mut scope = self.locals.pop().expect("loop counter scope");
                        let mode_names = self.module_modes[self.active_module].clone();
                        self.hoist_block_children(span, &mut scope, &mode_names)?;
                        self.seed_reach_exports(span, &mut scope);
                        self.locals.push(scope);
                    }

                    let mut current = start_value;
                    loop {
                        if !*down && current > end_value {
                            break;
                        }
                        if *down && current < end_value {
                            break;
                        }

                        let cell = self
                            .locals
                            .last()
                            .and_then(|scope| scope.vars.get(counter))
                            .cloned()
                            .expect("loop counter exists");
                        cell.borrow_mut().value = Value::Int {
                            value: current,
                            kind,
                        };

                        match self.exec_stmts(body, frame)? {
                            Flow::Normal => {}
                            Flow::Exit(None) => break,
                            flow => return Ok(flow),
                        }

                        let next = if *down {
                            current.checked_sub(step_value)
                        } else {
                            current.checked_add(step_value)
                        };
                        match next {
                            Some(value) => current = value,
                            // An overflowing next value terminates the loop (§6.5.2).
                            None => break,
                        }
                    }

                    Ok(Flow::Normal)
                })();
                self.locals.pop();
                result
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, Error> {
        let expression = self.exprs.get(expr.span()).ok_or_else(|| {
            Error::coded("E0500", expr.span(), "internal: missing typed expression")
        })?;
        self.eval_ir(&expression)
    }

    fn eval_ir(&mut self, expr: &ir::Expr) -> Result<Value, Error> {
        match expr {
            ir::Expr::Integer {
                value, ty, span, ..
            } => self.make_int(*value, type_kind(*ty), *span),
            ir::Expr::Float { value, .. } => Ok(Value::Float(*value)),
            ir::Expr::SetValue { value, .. } => Ok(Value::Set(*value)),
            ir::Expr::FloatConversion { arg, span, .. } => {
                let value = self.eval_ir(arg)?;
                match value {
                    Value::Float(value) => Ok(Value::Float(value)),
                    Value::Int { value, .. } => Ok(Value::Float(value as f64)),
                    _ => Err(Error::coded(
                        "E0507",
                        *span,
                        "conversion to FLOAT requires a numeric value",
                    )),
                }
            }
            ir::Expr::Bool { value, .. } => Ok(Value::Bool(*value)),
            ir::Expr::Char { value, .. } => Ok(Value::Char(*value)),
            ir::Expr::String { value, .. } => Ok(Value::String(value.clone())),
            ir::Expr::Bits { value, .. } => Ok(Value::Bits(value.clone())),
            ir::Expr::Null { .. } => Ok(Value::Ref(None)),
            ir::Expr::This { span, .. } => {
                if self.process_id == usize::MAX {
                    return Err(Error::coded(
                        "E0500",
                        *span,
                        "THIS is only valid inside a process",
                    ));
                }
                Ok(Value::Instance(self.process_id))
            }
            ir::Expr::Start {
                process,
                args,
                passing,
                targets,
                span,
                ..
            } => {
                let Some(process_ref) = self.lookup_process(process) else {
                    return Err(Error::coded(
                        "E0505",
                        *span,
                        format!("unknown process `{process}`"),
                    ));
                };
                let call_args = self.prepare_call_args(args, passing, targets, *span)?;
                self.spawn_process(process_ref, call_args, *span)
                    .map(Value::Instance)
            }
            ir::Expr::SelfValue { span, .. } => {
                let Some(id) = self.self_modes.last().copied() else {
                    return Err(Error::coded(
                        "E0500",
                        *span,
                        "SELF is only valid inside a module-mode component",
                    ));
                };
                self.current_self_value(id, *span)
            }
            ir::Expr::SelfField {
                component,
                path,
                span,
                ..
            } => {
                let value = self.read_var(component, *span)?;
                read_path(value, path, *span)
            }
            ir::Expr::AddressOf { name, span, .. } => {
                let cell = self.find_var_cell(name).ok_or_else(|| {
                    Error::coded("E0505", *span, format!("unknown variable `{name}`"))
                })?;
                Ok(Value::Ref(Some(self.intern_ref(cell, false))))
            }
            ir::Expr::Deref { base, ty, span, .. } => {
                let cell = self.eval_reference(base, *span)?;
                let (value, cell_ty) = {
                    let var = cell.borrow();
                    (var.value.clone(), var.ty)
                };
                if cell_ty != *ty {
                    return Err(Error::coded(
                        "E0507",
                        *span,
                        "reference mode does not match the referenced location",
                    ));
                }
                if matches!(value, Value::Undefined) {
                    return Err(Error::coded("E0504", *span, "use of undefined value"));
                }
                self.convert(value, *ty, *span)
            }
            ir::Expr::Allocate {
                mode,
                value,
                constructor_args,
                span,
                ..
            } => {
                let value = if let Some(args) = constructor_args {
                    let mut values = Vec::with_capacity(args.len());
                    for arg in args {
                        values.push(self.eval_ir(arg)?);
                    }
                    let Type::Moreta(id) = mode else {
                        return Err(Error::coded(
                            "E0500",
                            *span,
                            "constructor arguments require a module mode",
                        ));
                    };
                    self.create_moreta_object_with_args(*id, values, *span)?
                } else {
                    match value {
                        Some(value) => {
                            let value = self.eval_ir(value)?;
                            self.convert(value, *mode, *span)?
                        }
                        None => default_value(&self.modes, *mode),
                    }
                };
                let cell = Rc::new(RefCell::new(Var::new(*mode, value)));
                if let Type::Moreta(id) = mode {
                    if self.modes.get_moreta(*id).kind == crate::ast::MoretaKind::Task {
                        self.start_task_worker(Rc::clone(&cell), *span)?;
                    }
                }
                Ok(Value::Ref(Some(self.intern_ref(cell, true))))
            }
            ir::Expr::Name { name, span, .. } => self.read_var(name, *span),
            ir::Expr::Builtin {
                builtin,
                args,
                span,
                ..
            } => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval_ir(arg)?);
                }
                match builtin {
                    crate::builtins::Builtin::Write => {
                        if let Some(value) = values.first() {
                            let Value::String(text) = value else {
                                return Err(Error::coded("E0507", *span, "WRITE requires CHARS"));
                            };
                            print!("{text}");
                            use std::io::Write as _;
                            std::io::stdout()
                                .flush()
                                .map_err(|error| Error::coded("E0500", *span, error.to_string()))?;
                        }
                        Ok(Value::Undefined)
                    }
                    crate::builtins::Builtin::WriteLine => {
                        if let Some(value) = values.first() {
                            let Value::String(text) = value else {
                                return Err(Error::coded(
                                    "E0507",
                                    *span,
                                    "WRITELINE requires CHARS",
                                ));
                            };
                            println!("{text}");
                        } else {
                            println!();
                        }
                        Ok(Value::Undefined)
                    }
                    crate::builtins::Builtin::AbsoluteTime => {
                        if values.is_empty() {
                            return Ok(Value::Time(unix_time_ms()));
                        }
                        if values.len() != 6 {
                            return Err(Error::coded(
                                "E0507",
                                *span,
                                "ABSTIME currently supports zero or six arguments",
                            ));
                        }
                        let mut parts = [0i128; 6];
                        for (index, value) in values.into_iter().enumerate() {
                            let (part, _) = self.expect_int(value, *span)?;
                            parts[index] = part;
                        }
                        let days =
                            days_from_civil(parts[0] as i32, parts[1] as u32, parts[2] as u32);
                        let seconds = days * 86_400 + parts[3] * 3_600 + parts[4] * 60 + parts[5];
                        Ok(Value::Time(seconds * 1_000))
                    }
                    crate::builtins::Builtin::Duration(unit) => {
                        let Some(value) = values.first() else {
                            return Err(Error::coded(
                                "E0507",
                                *span,
                                "duration built-in requires an argument",
                            ));
                        };
                        let (value, _) = self.expect_int(value.clone(), *span)?;
                        let multiplier = match unit {
                            crate::builtins::DurationUnit::Milliseconds => 1,
                            crate::builtins::DurationUnit::Seconds => 1_000,
                            crate::builtins::DurationUnit::Minutes => 60_000,
                            crate::builtins::DurationUnit::Hours => 3_600_000,
                            crate::builtins::DurationUnit::Days => 86_400_000,
                        };
                        let milliseconds = value.checked_mul(multiplier).ok_or_else(|| {
                            Error::coded("E0502", *span, "duration value overflow")
                        })?;
                        Ok(Value::Duration(milliseconds))
                    }
                    crate::builtins::Builtin::Terminate => {
                        let Some(Value::Ref(reference)) = values.first() else {
                            return Err(Error::coded(
                                "E0507",
                                *span,
                                "TERMINATE requires a reference value",
                            ));
                        };
                        let Some(handle) = reference else {
                            return Err(Error::exception(
                                "EMPTY",
                                *span,
                                "EMPTY: TERMINATE of NULL",
                            ));
                        };
                        if !self.ref_allocated.get(*handle).copied().unwrap_or(false) {
                            return Err(Error::coded(
                                "E0507",
                                *span,
                                "TERMINATE requires an allocated reference value",
                            ));
                        }
                        if let Some(cell) = self.refs.get(*handle).and_then(Clone::clone) {
                            let (value, ty) = {
                                let var = cell.borrow();
                                (var.value.clone(), var.ty)
                            };
                            if let Type::Moreta(id) = ty {
                                if self.modes.get_moreta(id).kind == crate::ast::MoretaKind::Task {
                                    self.stop_task_worker(Rc::as_ptr(&cell) as usize)?;
                                }
                                if let Some((_, destructor_span)) = self.modes.moreta_destructor(id)
                                {
                                    self.invoke_component_on_value(
                                        id,
                                        destructor_span,
                                        value,
                                        Vec::new(),
                                        *span,
                                    )?;
                                }
                            }
                            write_var_value(&mut cell.borrow_mut(), Value::Undefined);
                        }
                        if let Some(slot) = self.refs.get_mut(*handle) {
                            *slot = None;
                        }
                        Ok(Value::Undefined)
                    }
                    crate::builtins::Builtin::GetTextIndex
                    | crate::builtins::Builtin::SetTextIndex => Err(Error::coded(
                        "E0500",
                        *span,
                        "internal: text index built-in was not lowered",
                    )),
                    crate::builtins::Builtin::ReadLine => {
                        let mut line = String::new();
                        std::io::stdin()
                            .read_line(&mut line)
                            .map_err(|error| Error::coded("E0500", *span, error.to_string()))?;
                        let line = line.trim_end_matches(['\n', '\r']).to_owned();
                        Ok(Value::String(line))
                    }
                }
            }
            ir::Expr::TextIo {
                kind,
                target,
                index,
                ops,
                io,
                span,
            } => {
                let cell = self.find_var_cell(target).ok_or_else(|| {
                    Error::coded("E0505", *span, format!("unknown variable `{target}`"))
                })?;
                let (source, text_index, text_state, text_length) = {
                    let var = cell.borrow();
                    if let Some(text) = var.text.clone() {
                        let text_length = match var.ty {
                            Type::Text { length, .. } => Some(length),
                            _ => None,
                        };
                        let state = text.borrow();
                        let source = match &state.record.borrow().value {
                            Value::String(text) => text.clone(),
                            _ => {
                                return Err(Error::coded(
                                    "E0500",
                                    *span,
                                    "text record must be CHARS",
                                ))
                            }
                        };
                        (source, state.index, Some(text.clone()), text_length)
                    } else {
                        let source = match &var.value {
                            Value::String(text) => text.clone(),
                            _ => {
                                return Err(Error::coded(
                                    "E0500",
                                    *span,
                                    "text location must be CHARS",
                                ))
                            }
                        };
                        (source, 0, None, None)
                    }
                };
                let text_access = text_state.as_ref().and_then(|text| {
                    let access = text.borrow().access.clone();
                    let access = access.borrow();
                    match &access.value {
                        Value::Access(state) => state.clone(),
                        _ => None,
                    }
                });
                let transfer_index = match index {
                    Some(index) => {
                        let value = self.eval_ir(index)?;
                        let (index, _) = self.expect_int(value, index.span())?;
                        Some(index)
                    }
                    None => None,
                };
                let mut cursor = if text_access.is_some() {
                    0
                } else {
                    transfer_index.unwrap_or(text_index)
                };
                if cursor < 0 {
                    return Err(textfail(*span, "negative text index"));
                }
                let mut chars: Vec<char> = if matches!(kind, crate::builtins::TextIo::Read) {
                    if let Some(access) = &text_access {
                        match self.read_text_record(access, transfer_index, *span)? {
                            Some(record) => {
                                if let Some(length) = text_length {
                                    if record.chars().count() > length {
                                        return Err(textfail(
                                            *span,
                                            "record exceeds the text length",
                                        ));
                                    }
                                }
                                record.chars().collect()
                            }
                            None => return Ok(Value::Undefined),
                        }
                    } else {
                        source.chars().collect()
                    }
                } else {
                    source.chars().collect()
                };
                let mut io_index = 0;
                match kind {
                    crate::builtins::TextIo::Write => {
                        for op in ops {
                            match op {
                                crate::builtins::FormatOp::Text(text) => {
                                    write_text_at(&mut chars, &mut cursor, text, *span)?;
                                }
                                crate::builtins::FormatOp::Convert(conversion) => {
                                    let width = self.resolve_text_width(
                                        conversion.width,
                                        io,
                                        &mut io_index,
                                        *span,
                                    )?;
                                    let Some(crate::ir::TextIoArg::Value(argument)) =
                                        io.get(io_index)
                                    else {
                                        return Err(textfail(*span, "missing io list value"));
                                    };
                                    io_index += 1;
                                    let value = self.eval_ir(argument)?;
                                    let representation =
                                        format_value(&value, conversion, width, *span)?;
                                    write_text_at(&mut chars, &mut cursor, &representation, *span)?;
                                }
                                crate::builtins::FormatOp::Edit(editing) => {
                                    let width = self.resolve_text_width(
                                        editing.width,
                                        io,
                                        &mut io_index,
                                        *span,
                                    )?;
                                    apply_editing_write(
                                        &mut chars,
                                        &mut cursor,
                                        editing,
                                        width,
                                        *span,
                                    )?;
                                }
                                crate::builtins::FormatOp::Io(control) => {
                                    apply_io_write(&mut chars, &mut cursor, *control, *span)?;
                                }
                            }
                        }
                        let rendered: String = chars.into_iter().collect();
                        if let Some(length) = text_length {
                            if rendered.chars().count() > length {
                                return Err(textfail(*span, "text record exceeds the text length"));
                            }
                        }
                        {
                            let mut var = cell.borrow_mut();
                            if let Some(text) = &text_state {
                                let record = text.borrow().record.clone();
                                record.borrow_mut().value = Value::String(rendered.clone());
                            }
                            var.value = Value::String(rendered.clone());
                        }
                        if let Some(text) = &text_state {
                            text.borrow_mut().index = 0;
                        }
                        if let Some(access) = &text_access {
                            self.write_text_record(access, transfer_index, &rendered, *span)?;
                        }
                    }
                    crate::builtins::TextIo::Read => {
                        for op in ops {
                            match op {
                                crate::builtins::FormatOp::Text(text) => {
                                    read_text_exact(&chars, &mut cursor, text, *span)?;
                                }
                                crate::builtins::FormatOp::Convert(conversion) => {
                                    let width = self.resolve_text_width(
                                        conversion.width,
                                        io,
                                        &mut io_index,
                                        *span,
                                    )?;
                                    let Some(crate::ir::TextIoArg::Location(name)) =
                                        io.get(io_index)
                                    else {
                                        return Err(textfail(*span, "missing io list location"));
                                    };
                                    io_index += 1;
                                    let target_ty = self
                                        .find_var_cell(name)
                                        .map(|cell| cell.borrow().ty)
                                        .ok_or_else(|| {
                                            Error::coded(
                                                "E0505",
                                                *span,
                                                format!("unknown variable `{name}`"),
                                            )
                                        })?;
                                    let value = read_conversion(
                                        &chars,
                                        &mut cursor,
                                        conversion,
                                        width,
                                        target_ty,
                                        *span,
                                    )?;
                                    self.assign_var(name, value, *span)?;
                                }
                                crate::builtins::FormatOp::Edit(editing) => {
                                    let width = self.resolve_text_width(
                                        editing.width,
                                        io,
                                        &mut io_index,
                                        *span,
                                    )?;
                                    apply_editing_read(&chars, &mut cursor, editing, width, *span)?;
                                }
                                crate::builtins::FormatOp::Io(control) => {
                                    apply_io_read(&chars, &mut cursor, *control, *span)?;
                                }
                            }
                        }
                        if text_access.is_some() {
                            let record: String = chars.into_iter().collect();
                            let mut var = cell.borrow_mut();
                            if let Some(text) = &text_state {
                                let record_cell = text.borrow().record.clone();
                                record_cell.borrow_mut().value = Value::String(record.clone());
                            }
                            var.value = Value::String(record);
                        }
                    }
                }
                if let Some(text) = &text_state {
                    text.borrow_mut().index = 0;
                }
                Ok(Value::Undefined)
            }
            ir::Expr::TextIndex {
                get,
                target,
                value,
                span,
                ..
            } => {
                let cell = self.find_var_cell(target).ok_or_else(|| {
                    Error::coded("E0505", *span, format!("unknown variable `{target}`"))
                })?;
                let (text_length, text) = {
                    let var = cell.borrow();
                    let Type::Text { length, .. } = var.ty else {
                        return Err(Error::coded(
                            "E0507",
                            *span,
                            "text index requires a TEXT or WTEXT location",
                        ));
                    };
                    let text = var.text.clone().ok_or_else(|| {
                        Error::coded("E0507", *span, "text location has no text state")
                    })?;
                    (length as i128, text)
                };
                if *get {
                    let index = text.borrow().index;
                    Ok(Value::Int {
                        value: index,
                        kind: Some(IntKind::Int),
                    })
                } else {
                    let value = value.as_ref().ok_or_else(|| {
                        Error::coded("E0500", *span, "SETTEXTINDEX requires an index")
                    })?;
                    let value = self.eval_ir(value)?;
                    let (index, _) = self.expect_int(value, *span)?;
                    if index < 0 || index > text_length {
                        return Err(Error::exception(
                            "TEXTFAIL",
                            *span,
                            format!("TEXTFAIL: text index {index} is outside 0..{text_length}"),
                        ));
                    }
                    text.borrow_mut().index = index;
                    Ok(Value::Undefined)
                }
            }
            ir::Expr::TextSub {
                op,
                target,
                source,
                source_ref,
                span,
                ..
            } => {
                use crate::builtins::TextSubOp;
                let cell = self.find_var_cell(target).ok_or_else(|| {
                    Error::coded("E0505", *span, format!("unknown variable `{target}`"))
                })?;
                let text = {
                    let var = cell.borrow();
                    if !matches!(var.ty, Type::Text { .. }) {
                        return Err(Error::coded(
                            "E0507",
                            *span,
                            "text sub-location requires a TEXT or WTEXT location",
                        ));
                    }
                    var.text.clone().ok_or_else(|| {
                        Error::coded("E0500", *span, "text location has no text state")
                    })?
                };
                match op {
                    TextSubOp::GetAccess => {
                        let access = text.borrow().access.clone();
                        Ok(Value::Ref(Some(self.intern_ref(access, false))))
                    }
                    TextSubOp::GetRecord => {
                        let record = text.borrow().record.clone();
                        Ok(Value::Ref(Some(self.intern_ref(record, false))))
                    }
                    TextSubOp::SetAccess => {
                        let source_cell = self.transfer_cell(
                            source.as_deref().unwrap_or_default(),
                            source_ref.as_ref(),
                            *span,
                        )?;
                        if !matches!(source_cell.borrow().ty, Type::Access(_)) {
                            return Err(Error::coded(
                                "E0507",
                                *span,
                                "SETTEXTACCESS requires an ACCESS location",
                            ));
                        }
                        text.borrow_mut().access = source_cell;
                        Ok(Value::Undefined)
                    }
                    TextSubOp::SetRecord => {
                        let source_cell = self.transfer_cell(
                            source.as_deref().unwrap_or_default(),
                            source_ref.as_ref(),
                            *span,
                        )?;
                        let record_value = source_cell.borrow().value.clone();
                        {
                            let mut var = cell.borrow_mut();
                            var.value = record_value;
                        }
                        text.borrow_mut().record = source_cell;
                        Ok(Value::Undefined)
                    }
                }
            }
            ir::Expr::ArrayLiteral { values, array, .. } => {
                let element = array.element.ty();
                let mut lowered = Vec::with_capacity(values.len());
                for node in values {
                    let value = self.eval_ir(node)?;
                    lowered.push(self.convert(value, element, node.span())?);
                }
                Ok(Value::Array {
                    low: array.low,
                    values: lowered,
                })
            }
            ir::Expr::MethodCall {
                object,
                deref,
                proc_span,
                args,
                passing,
                targets,
                span,
                ..
            } => {
                let call_args = self.prepare_call_args(args, passing, targets, *span)?;
                if object == "<deref>" {
                    let Some(reference) = deref else {
                        return Err(Error::coded(
                            "E0500",
                            *span,
                            "internal: dereferenced method call has no reference",
                        ));
                    };
                    let cell = self.eval_reference(reference, *span)?;
                    let (ty, value) = {
                        let var = cell.borrow();
                        (var.ty, var.value.clone())
                    };
                    let temp = "\u{0}deref".to_owned();
                    self.locals.push(Scope::default());
                    self.locals
                        .last_mut()
                        .expect("deref method scope")
                        .vars
                        .insert(temp.clone(), Rc::new(RefCell::new(Var::new(ty, value))));
                    let result = self.call_method(&temp, *proc_span, call_args, *span, false);
                    let updated = self
                        .locals
                        .last_mut()
                        .expect("deref method scope")
                        .vars
                        .remove(&temp)
                        .map(|cell| cell.borrow().value.clone());
                    self.locals.pop();
                    if let Some(updated) = updated {
                        cell.borrow_mut().value = updated;
                    }
                    result
                } else if object == "<self>" {
                    let Some(cell) = self.self_cells.last().cloned() else {
                        return Err(Error::coded(
                            "E0500",
                            *span,
                            "SELF method call outside a module-mode component",
                        ));
                    };
                    let Some(id) = self.self_modes.last().copied() else {
                        return Err(Error::coded(
                            "E0500",
                            *span,
                            "SELF method call outside a module-mode component",
                        ));
                    };
                    let value = self.current_self_value(id, *span)?;
                    let temp = "\u{0}self".to_owned();
                    let temp_cell = Rc::new(RefCell::new(Var::new(Type::Moreta(id), value)));
                    self.locals
                        .last_mut()
                        .expect("local scope")
                        .vars
                        .insert(temp.clone(), Rc::clone(&temp_cell));
                    let result = self.call_method(&temp, *proc_span, call_args, *span, false);
                    let updated = self
                        .locals
                        .last_mut()
                        .expect("local scope")
                        .vars
                        .remove(&temp)
                        .map(|cell| cell.borrow().value.clone());
                    if let Some(updated) = updated {
                        cell.borrow_mut().value = updated;
                    }
                    self.refresh_self_locals(id, *span)?;
                    result
                } else {
                    self.call_method(object, *proc_span, call_args, *span, true)
                }
            }
            ir::Expr::Association {
                op,
                first,
                first_ref,
                second,
                index,
                usage,
                where_expr,
                value,
                ty,
                span,
            } => self.exec_association(
                *op,
                first,
                first_ref.as_ref(),
                second.as_deref(),
                index.as_ref(),
                usage.as_ref(),
                where_expr.as_ref(),
                value.as_ref(),
                *ty,
                *span,
            ),
            ir::Expr::PowerSetLiteral { entries, ty, span } => {
                let Type::PowerSet(set) = ty else {
                    return Err(Error::coded(
                        "E0500",
                        *span,
                        "internal: powerset literal has wrong mode",
                    ));
                };
                let mut values = std::collections::BTreeSet::new();
                for (value, end) in entries {
                    let value = self.eval_ir(value)?;
                    let start = powerset_member_rep(set.member, value, *span)?;
                    if let Some(end) = end {
                        let end = self.eval_ir(end)?;
                        let end = powerset_member_rep(set.member, end, *span)?;
                        if start <= end {
                            for member in start..=end {
                                values.insert(member);
                            }
                        }
                    } else {
                        values.insert(start);
                    }
                }
                Ok(Value::PowerSet(values))
            }
            ir::Expr::LabelledArrayLiteral { entries, array, .. } => {
                let element = array.element.ty();
                let mut values =
                    vec![Value::Undefined; (array.high - array.low + 1).max(0) as usize];
                for (indices, node) in entries {
                    if indices.is_empty() {
                        continue;
                    }
                    let value = self.eval_ir(node)?;
                    let value = self.convert(value, element, node.span())?;
                    for index in indices {
                        values[(index - array.low) as usize] = value.clone();
                    }
                }
                Ok(Value::Array {
                    low: array.low,
                    values,
                })
            }
            ir::Expr::ArraySlice {
                name,
                offset,
                count,
                array,
                span,
            } => {
                let value = self.read_var(name, *span)?;
                read_array_slice(value, *offset, *count, array.low, *span)
            }
            ir::Expr::ArraySliceDynamic {
                name,
                first,
                last,
                up,
                low,
                span,
                ..
            } => {
                let value = self.read_var(name, *span)?;
                let len = array_len(&value, *span)?;
                let first_value = self.eval_ir(first)?;
                let (first_value, _) = self.expect_int(first_value, first.span())?;
                let last_value = self.eval_ir(last)?;
                let (last_value, _) = self.expect_int(last_value, last.span())?;
                let (offset, count) =
                    dynamic_slice_range(len, *low, first_value, last_value, *up, *span)?;
                read_array_slice(value, offset, count, *low, *span)
            }
            ir::Expr::Index {
                name,
                indices,
                array,
                span,
                ..
            } => {
                let value = self.read_var(name, *span)?;
                let mut index_values = Vec::with_capacity(indices.len());
                for index in indices {
                    let index_value = self.eval_ir(index)?;
                    let (index, _) = self.expect_int(index_value, index.span())?;
                    index_values.push(index);
                }
                let offset = array_offset(*array, &index_values, *span)?;
                read_array_at(value, offset, *span)
            }
            ir::Expr::Conversion {
                kind, arg, span, ..
            } => {
                let value = self.eval_ir(arg)?;
                let (value, _) = self.expect_int(value, *span)?;
                self.make_int(value, Some(*kind), *span)
            }
            ir::Expr::Call {
                name,
                args,
                passing,
                targets,
                ty: _,
                span,
            } => {
                let Some(proc_ref) = self.lookup_proc(name) else {
                    return Err(Error::coded(
                        "E0505",
                        *span,
                        format!("unknown name `{name}`"),
                    ));
                };
                if proc_ref.def.params.len() != args.len() {
                    return Err(Error::coded(
                        "E0506",
                        *span,
                        format!(
                            "procedure `{name}` expects {} argument(s), found {}",
                            proc_ref.def.params.len(),
                            args.len()
                        ),
                    ));
                }

                let call_args = self.prepare_call_args(args, passing, targets, *span)?;
                self.call_proc(proc_ref, call_args, *span)
            }
            ir::Expr::Unary {
                op,
                operand,
                ty,
                span,
            } => {
                let value = self.eval_ir(operand)?;
                match op {
                    UnaryOp::Neg => match value {
                        Value::Float(value) => Ok(Value::Float(-value)),
                        value => {
                            let (value, _) = self.expect_int(value, *span)?;
                            let negated = value.checked_neg().ok_or_else(|| {
                                Error::coded_exception(
                                    "E0502",
                                    "OVERFLOW",
                                    *span,
                                    "OVERFLOW in unary `-`",
                                )
                            })?;
                            self.make_int(negated, type_kind(*ty), *span)
                        }
                    },
                    UnaryOp::Not => match value {
                        Value::Bool(value) => Ok(Value::Bool(!value)),
                        Value::Bits(value) => Ok(Value::Bits(
                            value
                                .chars()
                                .map(|bit| if bit == '1' { '0' } else { '1' })
                                .collect(),
                        )),
                        _ => Err(Error::coded(
                            "E0507",
                            *span,
                            "expected a BOOL or bit string value",
                        )),
                    },
                }
            }
            ir::Expr::Field {
                base, field, span, ..
            } => {
                let object = match base.as_ref() {
                    ir::Expr::Name { name, .. } => self.task_object_key(name),
                    _ => None,
                };
                if let Some(object) = object {
                    self.acquire_object(object)?;
                }
                let result = (|| {
                    let value = self.eval_ir(base)?;
                    read_field(value, field, *span)
                })();
                if let Some(object) = object {
                    self.release_object(object);
                }
                result
            }
            ir::Expr::Binary {
                op,
                left,
                right,
                ty,
                span,
            } => match op {
                BinaryOp::AndIf => {
                    let left_value = self.eval_ir(left)?;
                    if !self.as_bool(left_value, *span)? {
                        Ok(Value::Bool(false))
                    } else {
                        let right_value = self.eval_ir(right)?;
                        Ok(Value::Bool(self.as_bool(right_value, *span)?))
                    }
                }
                BinaryOp::OrIf => {
                    let left_value = self.eval_ir(left)?;
                    if self.as_bool(left_value, *span)? {
                        Ok(Value::Bool(true))
                    } else {
                        let right_value = self.eval_ir(right)?;
                        Ok(Value::Bool(self.as_bool(right_value, *span)?))
                    }
                }
                _ => {
                    let left_value = self.eval_ir(left)?;
                    let right_value = self.eval_ir(right)?;
                    self.apply_binary(*op, left_value, right_value, Some(*ty), *span)
                }
            },
        }
    }

    fn check_moreta_invariants(&mut self, id: types::MoretaId, span: Span) -> Result<(), Error> {
        let mut invariants = Vec::new();
        for base in &self.modes.get_moreta(id).bases {
            invariants.extend(self.collect_moreta_invariants(*base));
        }
        invariants.extend(self.collect_moreta_invariants(id));
        for invariant in invariants {
            let value = self.eval_expr(&invariant)?;
            if !self.as_bool(value, span)? {
                return Err(Error::exception(
                    "INVFAIL",
                    span,
                    "INVFAIL: module mode invariant is false",
                ));
            }
        }
        Ok(())
    }

    fn collect_moreta_invariants(&self, id: types::MoretaId) -> Vec<Expr> {
        self.moreta_invariants
            .get(&id)
            .map(|invariant| vec![invariant.clone()])
            .unwrap_or_default()
    }

    fn prepare_call_args(
        &mut self,
        args: &[ir::Node],
        passing: &[Passing],
        targets: &[Option<ir::AssignTarget>],
        span: Span,
    ) -> Result<Vec<CallArgument>, Error> {
        let mut call_args = Vec::with_capacity(args.len());
        for ((arg, passing), target) in args.iter().zip(passing).zip(targets) {
            match passing {
                Passing::In => call_args.push(CallArgument::Value(self.eval_ir(arg)?)),
                Passing::Out => {
                    let Some(target) = target else {
                        return Err(Error::coded(
                            "E0500",
                            span,
                            "internal: missing OUT argument location",
                        ));
                    };
                    call_args.push(CallArgument::Target(Box::new(target.clone())));
                }
                Passing::InOut => {
                    let Some(target) = target else {
                        return Err(Error::coded(
                            "E0500",
                            span,
                            "internal: missing INOUT argument location",
                        ));
                    };
                    let value = self.read_target(target, arg.span())?;
                    call_args.push(CallArgument::InOut {
                        target: Box::new(target.clone()),
                        value,
                    });
                }
                Passing::Loc => {
                    let Some(ir::AssignTarget::Var(name)) = target else {
                        return Err(Error::coded(
                            "E0500",
                            span,
                            "internal: LOC argument is not a variable",
                        ));
                    };
                    let Some(cell) = self.find_var_cell(name) else {
                        return Err(Error::coded(
                            "E0505",
                            span,
                            format!("unknown variable `{name}`"),
                        ));
                    };
                    call_args.push(CallArgument::Location(cell));
                }
            }
        }
        Ok(call_args)
    }

    fn call_method(
        &mut self,
        object_name: &str,
        proc_span: Span,
        args: Vec<CallArgument>,
        span: Span,
        dispatch_task: bool,
    ) -> Result<Value, Error> {
        if self.depth >= 1024 {
            return Err(Error::coded("E0506", span, "call depth limit exceeded"));
        }
        let object_ty = self
            .find_var_cell(object_name)
            .map(|cell| cell.borrow().ty)
            .ok_or_else(|| {
                Error::coded("E0505", span, format!("unknown variable `{object_name}`"))
            })?;
        let Type::Moreta(id) = object_ty else {
            return Err(Error::coded(
                "E0507",
                span,
                "method call requires a module-mode object",
            ));
        };
        let object_cell = self.find_var_cell(object_name).ok_or_else(|| {
            Error::coded("E0505", span, format!("unknown object `{object_name}`"))
        })?;
        let mode_kind = self.modes.get_moreta(id).kind;
        let region_object = mode_kind == crate::ast::MoretaKind::Region;
        let object_key = Rc::as_ptr(&object_cell) as usize;
        if mode_kind == crate::ast::MoretaKind::Task && dispatch_task {
            if self.scheduler.borrow().task_stopped.contains(&object_key) {
                return Err(Error::exception(
                    "EMPTY",
                    span,
                    "EMPTY: task object has terminated",
                ));
            }
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                let CallArgument::Value(value) = arg else {
                    return Err(Error::coded(
                        "E0507",
                        span,
                        "task procedure arguments must be IN values",
                    ));
                };
                values.push(value);
            }
            let mut scheduler = self.scheduler.borrow_mut();
            scheduler
                .task_jobs
                .entry(object_key)
                .or_default()
                .push_back(TaskJob {
                    proc_span,
                    args: values,
                });
            if let Some(entry) = scheduler
                .task_waiters
                .get_mut(&object_key)
                .and_then(|waiters| waiters.pop())
            {
                scheduler.ready.push_back(Runnable {
                    input: ProcessInput::TaskReady,
                    entry,
                });
            }
            return Ok(Value::Undefined);
        }
        if region_object {
            self.acquire_object(object_key)?;
        }
        let prepared = (|| {
            let object = self.read_var(object_name, span)?;
            let Value::Moreta(components) = object else {
                return Err(Error::coded("E0507", span, "expected a module-mode object"));
            };
            let proc_def = self.proc_defs.get(&proc_span).copied().ok_or_else(|| {
                Error::coded("E0500", proc_span, "internal: missing method definition")
            })?;
            Ok((components, proc_def))
        })();
        let (components, proc_def) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                if region_object {
                    self.release_object(object_key);
                }
                return Err(error);
            }
        };
        self.self_cells.push(object_cell);
        self.self_modes.push(id);

        self.locals.push(Scope::default());
        self.call_bases.push(self.locals.len() - 1);
        self.depth += 1;
        {
            let scope = self.locals.last_mut().expect("method scope");
            for component in &self.modes.get_moreta(id).components {
                if let crate::modes::MoretaComponent::Var { name, ty, .. } = component {
                    let value = components
                        .iter()
                        .find(|(component, _)| component == name)
                        .map(|(_, value)| value.clone())
                        .unwrap_or_else(|| default_value(&self.modes, *ty));
                    scope
                        .vars
                        .insert(name.clone(), Rc::new(RefCell::new(Var::new(*ty, value))));
                }
            }
        }
        let outcome = (|| {
            self.check_moreta_invariants(id, span)?;
            let outcome = self.call_proc_inner(proc_def, args, span)?;
            self.check_moreta_invariants(id, span)?;
            Ok(outcome)
        })();
        self.depth -= 1;
        self.call_bases.pop();
        let updated = if outcome.is_ok() {
            let mut updated = components;
            let scope = self.locals.last().expect("method scope");
            for (name, value) in &mut updated {
                if let Some(cell) = scope.vars.get(name) {
                    *value = cell.borrow().value.clone();
                }
            }
            Some(Value::Moreta(updated))
        } else {
            None
        };
        self.locals.pop();
        self.self_cells.pop();
        self.self_modes.pop();
        let update_result = if let Some(updated) = updated {
            self.assign_var(object_name, updated, span)
        } else {
            Ok(())
        };
        if region_object {
            self.release_object(object_key);
        }
        update_result?;
        let (result, updates) = outcome?;
        for (target, value) in updates {
            self.assign_target(&target, value, span)?;
        }
        Ok(result)
    }

    fn call_proc(
        &mut self,
        proc_ref: ProcRef<'a>,
        args: Vec<CallArgument>,
        span: Span,
    ) -> Result<Value, Error> {
        if self.depth >= 1024 {
            return Err(Error::coded("E0506", span, "call depth limit exceeded"));
        }
        let region = self
            .regions
            .contains(&proc_ref.module)
            .then_some(proc_ref.module);
        if let Some(region) = region {
            self.acquire_region(region)?;
        }

        let previous_module = self.active_module;
        let previous_scope = self.active_scope_key;
        self.active_module = proc_ref.module;
        self.active_scope_key = proc_ref.scope_key;
        self.locals.push(Scope::default());
        self.call_bases.push(self.locals.len() - 1);
        self.depth += 1;

        let outcome = self.call_proc_inner(proc_ref.def, args, span);

        self.depth -= 1;
        self.call_bases.pop();
        self.locals.pop();
        self.active_module = previous_module;
        self.active_scope_key = previous_scope;
        if let Some(region) = region {
            self.release_region(region);
        }

        let (result, updates) = outcome?;
        for (target, value) in updates {
            self.assign_target(&target, value, span)?;
        }
        Ok(result)
    }

    fn call_proc_inner(
        &mut self,
        proc_def: &'a ProcDef,
        args: Vec<CallArgument>,
        span: Span,
    ) -> Result<(Value, CallUpdates), Error> {
        let mut writebacks: Vec<(String, Box<ir::AssignTarget>, bool)> = Vec::new();
        for (param, arg) in proc_def.params.iter().zip(args) {
            let ty = self.resolve_mode(&param.mode)?;
            let value = match (param.passing, arg) {
                (Passing::In, CallArgument::Value(value)) => self.convert(value, ty, param.span)?,
                (Passing::Out, CallArgument::Target(target)) => {
                    writebacks.push((param.name.clone(), target, true));
                    Value::Undefined
                }
                (Passing::InOut, CallArgument::InOut { target, value }) => {
                    writebacks.push((param.name.clone(), target, false));
                    self.convert(value, ty, param.span)?
                }
                (Passing::Loc, CallArgument::Location(cell)) => {
                    if cell.borrow().ty != ty {
                        return Err(Error::coded(
                            "E0507",
                            param.span,
                            format!("LOC argument for `{}` has the wrong mode", param.name),
                        ));
                    }
                    let scope = self.locals.last_mut().expect("local scope");
                    if scope.vars.contains_key(&param.name) || scope.procs.contains_key(&param.name)
                    {
                        return Err(Error::coded(
                            "E0508",
                            param.span,
                            format!("duplicate declaration of `{}`", param.name),
                        ));
                    }
                    scope.vars.insert(param.name.clone(), cell);
                    continue;
                }
                _ => {
                    return Err(Error::coded(
                        "E0500",
                        param.span,
                        format!(
                            "internal: wrong argument kind for parameter `{}`",
                            param.name
                        ),
                    ));
                }
            };
            let scope = self.locals.last_mut().expect("local scope");
            if scope.vars.contains_key(&param.name) || scope.procs.contains_key(&param.name) {
                return Err(Error::coded(
                    "E0508",
                    param.span,
                    format!("duplicate declaration of `{}`", param.name),
                ));
            }
            scope.vars.insert(
                param.name.clone(),
                Rc::new(RefCell::new(Var::new(ty, value))),
            );
        }

        self.declare_local(&proc_def.declarations)?;
        self.initialize_declarations(&proc_def.declarations)?;

        let result_type = match &proc_def.result {
            Some(mode) => Some(self.resolve_mode(mode)?),
            None => None,
        };

        let mut frame = Frame {
            result_type,
            result: None,
        };

        let body = self.bodies.get(proc_def.span).ok_or_else(|| {
            Error::coded(
                "E0500",
                proc_def.span,
                "internal: missing procedure IR body",
            )
        })?;

        match self.exec_with_handler(proc_def.span, &body, &mut frame)? {
            Flow::Normal | Flow::Return | Flow::Stop | Flow::Timeout | Flow::Exit(None) => (),
            Flow::Exit(Some(name)) => {
                return Err(Error::coded(
                    "E0508",
                    proc_def.span,
                    format!("unknown EXIT label `{name}`"),
                ))
            }
        }
        let result = match result_type {
            Some(_) => frame.result.ok_or_else(|| {
                Error::coded(
                    "E0506",
                    span,
                    format!("procedure `{}` returned no result", proc_def.name),
                )
            })?,
            None => Value::Undefined,
        };

        let mut updates = Vec::with_capacity(writebacks.len());
        {
            let scope = self.locals.last().expect("local scope");
            for (name, target, is_out) in writebacks {
                let value = scope
                    .vars
                    .get(&name)
                    .map(|cell| cell.borrow().value.clone())
                    .ok_or_else(|| {
                        Error::coded(
                            "E0500",
                            span,
                            format!("internal: missing parameter `{name}`"),
                        )
                    })?;
                if is_out && matches!(value, Value::Undefined) {
                    return Err(Error::coded(
                        "E0504",
                        span,
                        format!("OUT parameter `{name}` was not assigned"),
                    ));
                }
                updates.push((target, value));
            }
        }
        Ok((result, updates))
    }

    fn apply_binary(
        &self,
        op: BinaryOp,
        left: Value,
        right: Value,
        result_type: Option<Type>,
        span: Span,
    ) -> Result<Value, Error> {
        match op {
            BinaryOp::And | BinaryOp::AndIf => match (left, right) {
                (Value::Bits(left), Value::Bits(right)) => {
                    Ok(Value::Bits(bits_binary("and", &left, &right, span)?))
                }
                (left, right) => Ok(Value::Bool(
                    self.as_bool(left, span)? && self.as_bool(right, span)?,
                )),
            },
            BinaryOp::Or | BinaryOp::OrIf => match (left, right) {
                (Value::Bits(left), Value::Bits(right)) => {
                    Ok(Value::Bits(bits_binary("or", &left, &right, span)?))
                }
                (left, right) => Ok(Value::Bool(
                    self.as_bool(left, span)? || self.as_bool(right, span)?,
                )),
            },
            BinaryOp::Xor => match (left, right) {
                (Value::Bits(left), Value::Bits(right)) => {
                    Ok(Value::Bits(bits_binary("xor", &left, &right, span)?))
                }
                (left, right) => Ok(Value::Bool(
                    self.as_bool(left, span)? ^ self.as_bool(right, span)?,
                )),
            },
            BinaryOp::Eq => Ok(Value::Bool(self.values_equal(&left, &right, span)?)),
            BinaryOp::Ne => Ok(Value::Bool(!self.values_equal(&left, &right, span)?)),
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let ordering = self.order_values(&left, &right, span)?;
                let result = match op {
                    BinaryOp::Lt => ordering == Ordering::Less,
                    BinaryOp::Le => ordering != Ordering::Greater,
                    BinaryOp::Gt => ordering == Ordering::Greater,
                    BinaryOp::Ge => ordering != Ordering::Less,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(result))
            }
            BinaryOp::In => {
                let Value::PowerSet(set) = right else {
                    return Err(Error::coded(
                        "E0500",
                        span,
                        "the right operand of IN must be a powerset",
                    ));
                };
                let member = match left {
                    Value::Int { value, .. } => value,
                    Value::Char(value) => value as i128,
                    Value::Bool(value) => i128::from(value),
                    _ => return Err(Error::coded("E0500", span, "invalid powerset member value")),
                };
                Ok(Value::Bool(set.contains(&member)))
            }
            BinaryOp::Concat => match (left, right) {
                (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
                (Value::Bits(left), Value::Bits(right)) => Ok(Value::Bits(left + &right)),
                _ => Err(Error::coded(
                    "E0507",
                    span,
                    "`//` requires matching character or bit strings",
                )),
            },
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Rem
            | BinaryOp::Pow => {
                if let (Value::PowerSet(left), Value::PowerSet(right)) = (&left, &right) {
                    let result = match op {
                        BinaryOp::Add => left.union(right).copied().collect(),
                        BinaryOp::Sub => left.difference(right).copied().collect(),
                        BinaryOp::Mul => left.intersection(right).copied().collect(),
                        _ => {
                            return Err(Error::coded(
                                "E0500",
                                span,
                                "this operator does not accept powerset operands",
                            ));
                        }
                    };
                    return Ok(Value::PowerSet(result));
                }
                if let (Value::Float(left), Value::Float(right)) = (&left, &right) {
                    let value = match op {
                        BinaryOp::Add => left + right,
                        BinaryOp::Sub => left - right,
                        BinaryOp::Mul => left * right,
                        BinaryOp::Div => {
                            if *right == 0.0 {
                                return Err(Error::coded("E0503", span, "division by zero"));
                            }
                            left / right
                        }
                        _ => {
                            return Err(Error::coded(
                                "E0500",
                                span,
                                "this operator does not accept FLOAT operands",
                            ));
                        }
                    };
                    return Ok(Value::Float(value));
                }
                let (left, _) = self.expect_int(left, span)?;
                let (right, _) = self.expect_int(right, span)?;
                let kind = match result_type {
                    Some(Type::Int(kind)) => Some(kind),
                    Some(Type::IntLiteral) | None => None,
                    Some(other) => {
                        return Err(Error::coded(
                            "E0507",
                            span,
                            format!("expected an integer result, found {}", other.describe()),
                        ));
                    }
                };

                let result = match op {
                    BinaryOp::Add => left.checked_add(right),
                    BinaryOp::Sub => left.checked_sub(right),
                    BinaryOp::Mul => left.checked_mul(right),
                    BinaryOp::Div => {
                        if right == 0 {
                            return Err(Error::coded("E0503", span, "division by zero"));
                        }
                        left.checked_div(right)
                    }
                    BinaryOp::Rem => {
                        if right == 0 {
                            return Err(Error::coded("E0503", span, "remainder by zero"));
                        }
                        left.checked_rem(right)
                    }
                    BinaryOp::Mod => {
                        if right <= 0 {
                            return Err(Error::coded(
                                "E0503",
                                span,
                                "MOD requires a positive divisor",
                            ));
                        }
                        left.checked_rem(right).and_then(|remainder| {
                            if remainder < 0 {
                                remainder.checked_add(right)
                            } else {
                                Some(remainder)
                            }
                        })
                    }
                    BinaryOp::Pow => {
                        if right < 0 {
                            return Err(Error::coded("E0503", span, "negative exponent"));
                        }
                        let exponent = u32::try_from(right)
                            .map_err(|_| Error::coded("E0503", span, "exponent is too large"))?;
                        left.checked_pow(exponent)
                    }
                    _ => unreachable!(),
                };

                match result {
                    Some(value) => self.make_int(value, kind, span),
                    None => Err(Error::coded_exception(
                        "E0502", "OVERFLOW", span, "OVERFLOW",
                    )),
                }
            }
        }
    }

    fn values_equal(&self, left: &Value, right: &Value, span: Span) -> Result<bool, Error> {
        if matches!(left, Value::Undefined) || matches!(right, Value::Undefined) {
            return Err(Error::coded("E0504", span, "use of undefined value"));
        }
        match (left, right) {
            (Value::Int { value: left, .. }, Value::Int { value: right, .. }) => Ok(left == right),
            (Value::Float(left), Value::Float(right)) => Ok(left == right),
            (Value::Bool(left), Value::Bool(right)) => Ok(left == right),
            (Value::Char(left), Value::Char(right)) => Ok(left == right),
            (Value::String(left), Value::String(right)) => Ok(left == right),
            (Value::Bits(left), Value::Bits(right)) => Ok(left == right),
            (Value::Ref(left), Value::Ref(right)) => Ok(left == right),
            (Value::Instance(left), Value::Instance(right)) => Ok(left == right),
            (Value::PowerSet(left), Value::PowerSet(right)) => Ok(left == right),
            (Value::Set(left), Value::Set(right)) => Ok(left == right),
            (
                Value::Array {
                    low: left_low,
                    values: left_values,
                },
                Value::Array {
                    low: right_low,
                    values: right_values,
                },
            ) => {
                if left_low != right_low || left_values.len() != right_values.len() {
                    return Ok(false);
                }
                for (left, right) in left_values.iter().zip(right_values) {
                    if !self.values_equal(left, right, span)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Value::Struct(left), Value::Struct(right)) => {
                if left.len() != right.len() {
                    return Err(Error::coded(
                        "E0507",
                        span,
                        "cannot compare structures of different modes",
                    ));
                }
                for ((left_name, left_value), (right_name, right_value)) in left.iter().zip(right) {
                    if left_name != right_name {
                        return Err(Error::coded(
                            "E0507",
                            span,
                            "cannot compare structures of different modes",
                        ));
                    }
                    if !self.values_equal(left_value, right_value, span)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Err(Error::coded("E0507", span, "cannot compare these values")),
        }
    }

    fn order_values(&self, left: &Value, right: &Value, span: Span) -> Result<Ordering, Error> {
        match (left, right) {
            (Value::Int { value: left, .. }, Value::Int { value: right, .. }) => {
                Ok(left.cmp(right))
            }
            (Value::Char(left), Value::Char(right)) => Ok(left.cmp(right)),
            (Value::Float(left), Value::Float(right)) => left
                .partial_cmp(right)
                .ok_or_else(|| Error::coded("E0507", span, "cannot order NaN values")),
            (Value::String(left), Value::String(right)) => Ok(left.cmp(right)),
            (Value::Bits(left), Value::Bits(right)) => Ok(left.cmp(right)),
            (Value::PowerSet(left), Value::PowerSet(right)) => {
                if left == right {
                    Ok(Ordering::Equal)
                } else if left.is_subset(right) {
                    Ok(Ordering::Less)
                } else if left.is_superset(right) {
                    Ok(Ordering::Greater)
                } else {
                    Err(Error::coded("E0500", span, "powersets are not ordered"))
                }
            }
            _ => Err(Error::coded("E0507", span, "cannot order these values")),
        }
    }

    fn as_bool(&self, value: Value, span: Span) -> Result<bool, Error> {
        match value {
            Value::Bool(value) => Ok(value),
            _ => Err(Error::coded("E0507", span, "expected a BOOL value")),
        }
    }

    fn expect_int(&self, value: Value, span: Span) -> Result<(i128, Option<IntKind>), Error> {
        match value {
            Value::Int { value, kind } => Ok((value, kind)),
            _ => Err(Error::coded("E0507", span, "expected an integer value")),
        }
    }

    fn make_int(&self, value: i128, kind: Option<IntKind>, span: Span) -> Result<Value, Error> {
        let bounds = match kind {
            Some(kind) => types::int_bounds(kind, self.profile),
            None => types::int_bounds(IntKind::Long, self.profile),
        };

        let Some((low, high)) = bounds else {
            return Err(Error::coded(
                "E0507",
                span,
                "integer widths above 127 are not supported",
            ));
        };

        if value < low || value > high {
            let target = kind.map(IntKind::name).unwrap_or("&INT");
            return Err(Error::coded_exception(
                "E0502",
                "OVERFLOW",
                span,
                format!("OVERFLOW: `{value}` does not fit {target}"),
            ));
        }

        Ok(Value::Int { value, kind })
    }

    fn convert(&self, value: Value, ty: Type, span: Span) -> Result<Value, Error> {
        match ty {
            Type::Float => match value {
                Value::Float(value) => Ok(Value::Float(value)),
                _ => Err(Error::coded("E0507", span, "expected a FLOAT value")),
            },
            Type::Int(kind) => {
                let (value, _) = self.expect_int(value, span)?;
                self.make_int(value, Some(kind), span)
            }
            Type::Bool => match value {
                Value::Bool(value) => Ok(Value::Bool(value)),
                _ => Err(Error::coded("E0507", span, "expected a BOOL value")),
            },
            Type::Char => match value {
                Value::Char(value) if (value as u32) <= 0xFF => Ok(Value::Char(value)),
                Value::Char(_) => Err(Error::coded(
                    "E0507",
                    span,
                    "character value is outside the CHAR range",
                )),
                _ => Err(Error::coded("E0507", span, "expected a CHAR value")),
            },
            Type::WChar => match value {
                Value::Char(value) if (value as u32) <= 0xFFFF => Ok(Value::Char(value)),
                Value::Char(_) => Err(Error::coded(
                    "E0507",
                    span,
                    "character value is outside the WCHAR range",
                )),
                _ => Err(Error::coded("E0507", span, "expected a WCHAR value")),
            },
            Type::WString => match value {
                Value::String(value) => Ok(Value::String(value)),
                _ => Err(Error::coded(
                    "E0507",
                    span,
                    "expected a wide character string value",
                )),
            },
            Type::WBoundedString { length, varying } => match value {
                Value::String(value) => {
                    let actual = value.chars().count();
                    let fits = if varying {
                        actual <= length
                    } else {
                        actual == length
                    };
                    if fits {
                        Ok(Value::String(value))
                    } else {
                        Err(Error::coded(
                            "E0507",
                            span,
                            format!(
                                "string length {actual} does not fit WCHARS({length}){}",
                                if varying { " VARYING" } else { "" }
                            ),
                        ))
                    }
                }
                _ => Err(Error::coded(
                    "E0507",
                    span,
                    "expected a wide character string value",
                )),
            },
            Type::Ref(_) | Type::Ptr | Type::Row(_) | Type::Null => match value {
                Value::Ref(_) => Ok(value),
                _ => Err(Error::coded("E0507", span, "expected a reference value")),
            },
            Type::Event { .. } | Type::Buffer { .. } => Err(Error::coded(
                "E0507",
                span,
                "synchronization locations do not have values",
            )),
            Type::Time => match value {
                Value::Time(value) => Ok(Value::Time(value)),
                _ => Err(Error::coded("E0507", span, "expected a TIME value")),
            },
            Type::Instance => match value {
                Value::Instance(value) => Ok(Value::Instance(value)),
                _ => Err(Error::coded("E0507", span, "expected an INSTANCE value")),
            },
            Type::Duration => match value {
                Value::Duration(value) => Ok(Value::Duration(value)),
                _ => Err(Error::coded("E0507", span, "expected a DURATION value")),
            },
            Type::Text { length, wide, .. } => match value {
                Value::String(value) => {
                    let actual = value.chars().count();
                    let valid = actual <= length
                        && value.chars().all(|ch| {
                            if wide {
                                (ch as u32) <= 0xFFFF
                            } else {
                                (ch as u32) <= 0xFF
                            }
                        });
                    if valid {
                        Ok(Value::String(value))
                    } else {
                        Err(Error::coded(
                            "E0507",
                            span,
                            format!(
                                "text length {actual} does not fit {}({length})",
                                if wide { "WTEXT" } else { "TEXT" }
                            ),
                        ))
                    }
                }
                _ => Err(Error::coded(
                    "E0507",
                    span,
                    if wide {
                        "expected a wide character string value"
                    } else {
                        "expected a character string value"
                    },
                )),
            },
            Type::Bits => match value {
                Value::Bits(value) => Ok(Value::Bits(value)),
                _ => Err(Error::coded("E0507", span, "expected a bit string value")),
            },
            Type::BoundedBits { length, varying } => match value {
                Value::Bits(value) => {
                    let actual = value.len();
                    let fits = if varying {
                        actual <= length
                    } else {
                        actual == length
                    };
                    if fits {
                        Ok(Value::Bits(value))
                    } else {
                        Err(Error::coded(
                            "E0507",
                            span,
                            format!(
                                "string length {actual} does not fit BOOLS({length}){}",
                                if varying { " VARYING" } else { "" }
                            ),
                        ))
                    }
                }
                _ => Err(Error::coded("E0507", span, "expected a bit string value")),
            },
            Type::String => match value {
                Value::String(value) => Ok(Value::String(value)),
                _ => Err(Error::coded(
                    "E0507",
                    span,
                    "expected a character string value",
                )),
            },
            Type::BoundedString { length, varying } => match value {
                Value::String(value) => {
                    let actual = value.chars().count();
                    let fits = if varying {
                        actual <= length
                    } else {
                        actual == length
                    };
                    if fits {
                        Ok(Value::String(value))
                    } else {
                        Err(Error::coded(
                            "E0507",
                            span,
                            format!(
                                "string length {actual} does not fit CHARS({length}){}",
                                if varying { " VARYING" } else { "" }
                            ),
                        ))
                    }
                }
                _ => Err(Error::coded(
                    "E0507",
                    span,
                    "expected a character string value",
                )),
            },
            Type::Struct(_) => match value {
                Value::Struct(_) => Ok(value),
                _ => Err(Error::coded("E0507", span, "expected a structure value")),
            },
            Type::Moreta(_) => match value {
                Value::Moreta(_) => Ok(value),
                _ => Err(Error::coded("E0507", span, "expected a module mode value")),
            },
            Type::Set(_) => match value {
                Value::Set(_) => Ok(value),
                _ => Err(Error::coded("E0507", span, "expected a set value")),
            },
            Type::Array(array) => match value {
                Value::Array { low, values }
                    if low == array.low
                        && values.len() == (array.high - array.low + 1) as usize =>
                {
                    Ok(Value::Array { low, values })
                }
                _ => Err(Error::coded("E0507", span, "expected an array value")),
            },
            Type::MultiArray(array) => match value {
                Value::Array { low, values }
                    if low == array.dimensions()[0].low
                        && values.len() == array.total_len() as usize =>
                {
                    Ok(Value::Array { low, values })
                }
                _ => Err(Error::coded(
                    "E0507",
                    span,
                    "expected a multidimensional array value",
                )),
            },
            Type::DynamicArray { low, .. } => match value {
                Value::Array {
                    low: value_low,
                    values,
                } if value_low == low => Ok(Value::Array {
                    low: value_low,
                    values,
                }),
                _ => Err(Error::coded("E0507", span, "expected an array slice value")),
            },
            Type::Association => match value {
                Value::Association(_) => Ok(value),
                _ => Err(Error::coded("E0507", span, "expected an ASSOCIATION value")),
            },
            Type::PowerSet(_) => match value {
                Value::PowerSet(_) => Ok(value),
                _ => Err(Error::coded("E0507", span, "expected a powerset value")),
            },
            Type::Access(_) => match value {
                Value::Access(_) => Ok(value),
                _ => Err(Error::coded("E0507", span, "expected an ACCESS value")),
            },
            Type::IntLiteral => Err(Error::coded(
                "E0500",
                span,
                "internal: cannot declare a literal type",
            )),
            Type::ArrayTuple => Err(Error::coded(
                "E0500",
                span,
                "array literal has no target array mode",
            )),
        }
    }

    fn resolve_mode(&self, mode: &ModeRef) -> Result<Type, Error> {
        if mode.name == "BUFFER" {
            let element = mode.buffer_element.as_deref().ok_or_else(|| {
                Error::coded("E0400", mode.span, "BUFFER requires an element mode")
            })?;
            let element = self.resolve_mode(element)?;
            let Some(element) = crate::types::RefTarget::from_type(element) else {
                return Err(Error::coded(
                    "E0400",
                    mode.span,
                    "BUFFER element must be a value mode",
                ));
            };
            return Ok(Type::Buffer {
                length: mode.string_length,
                element,
            });
        }
        if mode.name == "EVENT" {
            return Ok(Type::Event {
                length: mode.string_length,
            });
        }
        if mode.reference || mode.row {
            let mut inner = mode.clone();
            inner.reference = false;
            inner.row = false;
            let base = self.resolve_mode(&inner)?;
            let Some(target) = crate::types::RefTarget::from_type(base) else {
                return Err(Error::coded(
                    "E0400",
                    mode.span,
                    "this mode cannot be referenced",
                ));
            };
            return Ok(if mode.reference {
                Type::Ref(target)
            } else {
                Type::Row(target)
            });
        }
        if mode.powerset {
            let member = match crate::types::resolve_mode_ref(
                &ModeRef {
                    name: mode.name.clone(),
                    arrays: Vec::new(),
                    string_length: None,
                    varying: false,
                    access: false,
                    powerset: false,
                    reference: false,
                    row: false,
                    buffer_element: None,
                    access_indexed: false,
                    access_index: None,
                    span: mode.span,
                },
                &self.module_modes[self.active_module],
            ) {
                Some(Type::Int(kind)) => crate::types::PowerSetMember::Int(kind),
                Some(Type::Bool) => crate::types::PowerSetMember::Bool,
                Some(Type::Char) => crate::types::PowerSetMember::Char,
                Some(Type::WChar) => crate::types::PowerSetMember::WChar,
                _ => {
                    return Err(Error::coded(
                        "E0500",
                        mode.span,
                        "invalid POWERSET member mode",
                    ))
                }
            };
            return Ok(Type::PowerSet(crate::types::PowerSetType { member }));
        }
        if mode.access {
            return Ok(Type::Access(crate::types::AccessType {
                indexed: mode.access_indexed,
                index: mode
                    .access_index
                    .map(|bounds| crate::types::ArrayDimension {
                        low: bounds.low,
                        high: bounds.high,
                    }),
            }));
        }
        if !mode.arrays.is_empty() {
            let element_mode = ModeRef {
                name: mode.name.clone(),
                arrays: Vec::new(),
                string_length: mode.string_length,
                varying: mode.varying,
                access: mode.access,
                powerset: mode.powerset,
                reference: mode.reference,
                row: mode.row,
                buffer_element: None,
                access_indexed: mode.access_indexed,
                access_index: mode.access_index,
                span: mode.span,
            };
            let base = self.resolve_mode(&element_mode)?;
            let Some(element) = crate::types::ArrayElement::from_type(base) else {
                return Err(Error::coded(
                    "E0500",
                    mode.span,
                    format!(
                        "array elements must be scalar or structure modes, found {}",
                        base.describe()
                    ),
                ));
            };
            return crate::types::array_type_from_bounds(&mode.arrays, element)
                .ok_or_else(|| Error::coded("E0500", mode.span, "unsupported array dimensions"));
        }
        if let Some(length) = mode.string_length {
            return Ok(match mode.name.as_str() {
                "TEXT" => Type::Text {
                    length,
                    wide: false,
                    indexed: mode.access_indexed,
                    index: mode
                        .access_index
                        .map(|bounds| crate::types::ArrayDimension {
                            low: bounds.low,
                            high: bounds.high,
                        }),
                },
                "WTEXT" => Type::Text {
                    length,
                    wide: true,
                    indexed: mode.access_indexed,
                    index: mode
                        .access_index
                        .map(|bounds| crate::types::ArrayDimension {
                            low: bounds.low,
                            high: bounds.high,
                        }),
                },
                "WCHARS" => Type::WBoundedString {
                    length,
                    varying: mode.varying,
                },
                "BOOLS" => Type::BoundedBits {
                    length,
                    varying: mode.varying,
                },
                _ => Type::BoundedString {
                    length,
                    varying: mode.varying,
                },
            });
        }
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(ty) = scope.modes.get(&mode.name) {
                return Ok(*ty);
            }
        }
        if let Some(scope) = self.active_module_scope() {
            if let Some(ty) = scope.modes.get(&mode.name) {
                return Ok(*ty);
            }
            if let Some(exports) = scope.exports.get(&mode.name) {
                if let [ReachBinding {
                    kind: ReachKind::Mode(ty),
                    meta,
                    ..
                }] = exports.as_slice()
                {
                    if meta.prefix.is_empty() {
                        return Ok(*ty);
                    }
                }
            }
        }
        resolve_mode_with(mode, &self.modules[self.active_module].modes)
    }

    fn active_module_scope(&self) -> Option<&Scope<'a>> {
        if let Some(key) = self.active_scope_key {
            for scope in self.locals.iter().rev() {
                if scope.key == Some(key) {
                    return Some(scope);
                }
            }
            return self.nested_scopes.get(&key);
        }
        self.modules.get(self.active_module)
    }

    fn object_value_from_scope(&mut self, id: types::MoretaId, span: Span) -> Result<Value, Error> {
        let names: Vec<String> = self
            .modes
            .get_moreta(id)
            .components
            .iter()
            .filter_map(|component| match component {
                crate::modes::MoretaComponent::Var { name, .. } => Some(name.clone()),
                crate::modes::MoretaComponent::Proc { .. }
                | crate::modes::MoretaComponent::Mode { .. } => None,
            })
            .collect();
        let mut values = Vec::with_capacity(names.len());
        for name in names {
            let value = self
                .find_var_cell(&name)
                .map(|cell| cell.borrow().value.clone())
                .unwrap_or(Value::Undefined);
            values.push((name, value));
        }
        let _ = span;
        Ok(Value::Moreta(values))
    }

    fn apply_object_value_to_scope(
        &mut self,
        id: types::MoretaId,
        value: Value,
        span: Span,
    ) -> Result<(), Error> {
        let Value::Moreta(values) = value else {
            return Err(Error::coded("E0500", span, "expected a module-mode object"));
        };
        let names: Vec<String> = self
            .modes
            .get_moreta(id)
            .components
            .iter()
            .filter_map(|component| match component {
                crate::modes::MoretaComponent::Var { name, .. } => Some(name.clone()),
                crate::modes::MoretaComponent::Proc { .. }
                | crate::modes::MoretaComponent::Mode { .. } => None,
            })
            .collect();
        for (name, value) in values {
            if names.contains(&name) {
                for scope in self.locals.iter_mut().rev() {
                    if let Some(cell) = scope.vars.get(&name) {
                        write_var_value(&mut cell.borrow_mut(), value.clone());
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn invoke_component_on_value(
        &mut self,
        id: types::MoretaId,
        proc_span: Span,
        value: Value,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        let temp = "\u{0}component".to_owned();
        let cell = Rc::new(RefCell::new(Var::new(Type::Moreta(id), value)));
        self.locals.push(Scope::default());
        self.locals
            .last_mut()
            .expect("component scope")
            .vars
            .insert(temp.clone(), Rc::clone(&cell));
        let call_args = args.into_iter().map(CallArgument::Value).collect();
        let result = self.call_method(&temp, proc_span, call_args, span, false);
        let updated = self
            .locals
            .last_mut()
            .expect("component scope")
            .vars
            .remove(&temp)
            .map(|cell| cell.borrow().value.clone())
            .unwrap_or(Value::Undefined);
        self.locals.pop();
        result?;
        Ok(updated)
    }

    fn current_self_value(&mut self, id: types::MoretaId, span: Span) -> Result<Value, Error> {
        let names: Vec<String> = self
            .modes
            .get_moreta(id)
            .components
            .iter()
            .filter_map(|component| match component {
                crate::modes::MoretaComponent::Var { name, .. } => Some(name.clone()),
                crate::modes::MoretaComponent::Proc { .. }
                | crate::modes::MoretaComponent::Mode { .. } => None,
            })
            .collect();
        let mut values = Vec::with_capacity(names.len());
        for name in names {
            let value = self
                .find_var_cell(&name)
                .map(|cell| cell.borrow().value.clone())
                .unwrap_or(Value::Undefined);
            values.push((name, value));
        }
        let _ = span;
        Ok(Value::Moreta(values))
    }

    fn refresh_self_locals(&mut self, id: types::MoretaId, span: Span) -> Result<(), Error> {
        let Some(cell) = self.self_cells.last().cloned() else {
            return Ok(());
        };
        let Value::Moreta(values) = cell.borrow().value.clone() else {
            return Ok(());
        };
        let names: Vec<String> = self
            .modes
            .get_moreta(id)
            .components
            .iter()
            .filter_map(|component| match component {
                crate::modes::MoretaComponent::Var { name, .. } => Some(name.clone()),
                crate::modes::MoretaComponent::Proc { .. }
                | crate::modes::MoretaComponent::Mode { .. } => None,
            })
            .collect();
        for (name, value) in values {
            if !names.contains(&name) {
                continue;
            }
            for scope in self.locals.iter_mut().rev() {
                if let Some(cell) = scope.vars.get(&name) {
                    cell.borrow_mut().value = value.clone();
                    break;
                }
            }
        }
        let _ = span;
        Ok(())
    }

    fn intern_ref(&mut self, cell: VarCell, allocated: bool) -> usize {
        if let Some(index) = self
            .refs
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|slot| Rc::ptr_eq(slot, &cell)))
        {
            if allocated {
                self.ref_allocated[index] = true;
            }
            return index;
        }
        let index = self.refs.len();
        self.refs.push(Some(cell));
        self.ref_allocated.push(allocated);
        index
    }

    fn eval_reference(&mut self, reference: &ir::Node, span: Span) -> Result<VarCell, Error> {
        match self.eval_ir(reference)? {
            Value::Ref(Some(handle)) => {
                self.refs.get(handle).and_then(Clone::clone).ok_or_else(|| {
                    Error::exception("EMPTY", span, "EMPTY: reference lifetime has ended")
                })
            }
            Value::Ref(None) => Err(Error::exception("EMPTY", span, "EMPTY: NULL reference")),
            _ => Err(Error::coded("E0507", span, "expected a reference value")),
        }
    }

    fn acquire_object(&mut self, object: usize) -> Result<(), Error> {
        if let Some(depth) = self.object_depths.get_mut(&object) {
            *depth += 1;
            return Ok(());
        }
        let owner = self.scheduler.borrow().object_owner.get(&object).copied();
        match owner {
            None => {
                self.scheduler
                    .borrow_mut()
                    .object_owner
                    .insert(object, self.process_id);
            }
            Some(owner) if owner == self.process_id => {}
            Some(_) => cooperative_acquire_object(object)?,
        }
        self.object_depths.insert(object, 1);
        Ok(())
    }

    fn release_object(&mut self, object: usize) {
        let Some(depth) = self.object_depths.get_mut(&object) else {
            return;
        };
        *depth -= 1;
        if *depth > 0 {
            return;
        }
        self.object_depths.remove(&object);
        let mut scheduler = self.scheduler.borrow_mut();
        if let Some(waiters) = scheduler.object_waiters.get_mut(&object) {
            if !waiters.is_empty() {
                let entry = waiters.remove(0);
                scheduler.object_owner.insert(object, entry.id);
                scheduler.ready.push_back(Runnable {
                    input: ProcessInput::None,
                    entry,
                });
                return;
            }
        }
        scheduler.object_owner.remove(&object);
    }

    fn acquire_region(&mut self, region: usize) -> Result<(), Error> {
        if let Some(depth) = self.region_depths.get_mut(&region) {
            *depth += 1;
            return Ok(());
        }
        let owner = self.scheduler.borrow().region_owner.get(&region).copied();
        match owner {
            None => {
                self.scheduler
                    .borrow_mut()
                    .region_owner
                    .insert(region, self.process_id);
            }
            Some(owner) if owner == self.process_id => {}
            Some(_) => cooperative_acquire_region(region)?,
        }
        self.region_depths.insert(region, 1);
        Ok(())
    }

    fn release_region(&mut self, region: usize) {
        let Some(depth) = self.region_depths.get_mut(&region) else {
            return;
        };
        *depth -= 1;
        if *depth > 0 {
            return;
        }
        self.region_depths.remove(&region);
        let mut scheduler = self.scheduler.borrow_mut();
        if let Some(waiters) = scheduler.region_waiters.get_mut(&region) {
            if !waiters.is_empty() {
                let entry = waiters.remove(0);
                scheduler.region_owner.insert(region, entry.id);
                scheduler.ready.push_back(Runnable {
                    input: ProcessInput::None,
                    entry,
                });
                return;
            }
        }
        scheduler.region_owner.remove(&region);
    }

    fn buffer_location(
        &self,
        name: &str,
        span: Span,
    ) -> Result<(usize, Option<usize>, Type), Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::coded("E0505", span, format!("unknown buffer `{name}`")))?;
        let ty = cell.borrow().ty;
        let Type::Buffer { length, element } = ty else {
            return Err(Error::coded(
                "E0507",
                span,
                format!("`{name}` is not a BUFFER location"),
            ));
        };
        Ok((Rc::as_ptr(&cell) as usize, length, element.ty()))
    }

    fn take_buffer_value(&mut self, name: &str, key: usize) -> Option<(Vec<Value>, usize)> {
        let mut scheduler = self.scheduler.borrow_mut();
        let state = scheduler.buffers.get_mut(&key)?;
        let message_index = {
            let mut best: Option<usize> = None;
            for (index, message) in state.messages.iter().enumerate() {
                if best
                    .map(|best| message.priority > state.messages[best].priority)
                    .unwrap_or(true)
                {
                    best = Some(index);
                }
            }
            best
        };
        let (received, sender, sender_entry) = if let Some(index) = message_index {
            let message = state.messages.remove(index)?;
            let mut sender_entry = None;
            if let Some(sender_index) = {
                let mut best: Option<usize> = None;
                for (index, sender) in state.senders.iter().enumerate() {
                    if best
                        .map(|best| sender.priority > state.senders[best].priority)
                        .unwrap_or(true)
                    {
                        best = Some(index);
                    }
                }
                best
            } {
                let sender = state.senders.remove(sender_index);
                let slot = Rc::clone(&sender.slot);
                let entry = slot.borrow_mut().take();
                if let Some(entry) = entry {
                    state.messages.push_back(BufferMessage {
                        priority: sender.priority,
                        sender: sender.id,
                        values: sender.values,
                    });
                    sender_entry = Some(entry);
                }
            }
            (message.values, message.sender, sender_entry)
        } else {
            let sender_index = {
                let mut best: Option<usize> = None;
                for (index, sender) in state.senders.iter().enumerate() {
                    if best
                        .map(|best| sender.priority > state.senders[best].priority)
                        .unwrap_or(true)
                    {
                        best = Some(index);
                    }
                }
                best
            }?;
            let sender = state.senders.remove(sender_index);
            let slot = Rc::clone(&sender.slot);
            let entry = slot.borrow_mut().take()?;
            (sender.values, sender.id, Some(entry))
        };
        if let Some(entry) = sender_entry {
            scheduler.ready.push_back(Runnable {
                input: ProcessInput::None,
                entry,
            });
        }
        let _ = name;
        Some((received, sender))
    }

    fn task_object_key(&self, name: &str) -> Option<usize> {
        let cell = self.find_var_cell(name)?;
        let Type::Moreta(id) = cell.borrow().ty else {
            return None;
        };
        if self.modes.get_moreta(id).kind != crate::ast::MoretaKind::Task {
            return None;
        }
        Some(Rc::as_ptr(&cell) as usize)
    }

    fn event_location(&self, name: &str, span: Span) -> Result<(usize, Option<usize>), Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::coded("E0505", span, format!("unknown event `{name}`")))?;
        let Type::Event { length } = cell.borrow().ty else {
            return Err(Error::coded(
                "E0507",
                span,
                format!("`{name}` is not an EVENT location"),
            ));
        };
        let key = Rc::as_ptr(&cell) as usize;
        Ok((key, length))
    }

    fn target_type(&self, target: &ir::AssignTarget, span: Span) -> Result<Type, Error> {
        match target {
            ir::AssignTarget::Var(name) => {
                let cell = self.find_var_cell(name).ok_or_else(|| {
                    Error::coded("E0505", span, format!("unknown variable `{name}`"))
                })?;
                let ty = cell.borrow().ty;
                Ok(ty)
            }
            ir::AssignTarget::Field { ty, .. } => Ok(*ty),
            ir::AssignTarget::Index { ty, .. } => Ok(*ty),
            ir::AssignTarget::Slice { ty, .. }
            | ir::AssignTarget::SliceDynamic { ty, .. }
            | ir::AssignTarget::Deref { ty, .. }
            | ir::AssignTarget::DerefField { ty, .. } => Ok(*ty),
        }
    }

    fn read_target(&mut self, target: &ir::AssignTarget, span: Span) -> Result<Value, Error> {
        match target {
            ir::AssignTarget::Var(name) => self.read_var(name, span),
            ir::AssignTarget::Field {
                root, index, path, ..
            } => {
                let value = match index {
                    Some(index) => {
                        let value = self.read_var(root, span)?;
                        let index_value = self.eval_ir(index)?;
                        let (index, _) = self.expect_int(index_value, index.span())?;
                        read_array_element(value, index, span)?
                    }
                    None => self.read_var(root, span)?,
                };
                read_path(value, path, span)
            }
            ir::AssignTarget::Index {
                name,
                indices,
                array,
                ..
            } => {
                let value = self.read_var(name, span)?;
                let mut index_values = Vec::with_capacity(indices.len());
                for index in indices {
                    let index_value = self.eval_ir(index)?;
                    let (index, _) = self.expect_int(index_value, index.span())?;
                    index_values.push(index);
                }
                let offset = array_offset(*array, &index_values, span)?;
                read_array_at(value, offset, span)
            }
            ir::AssignTarget::Slice {
                name,
                offset,
                count,
                ty,
            } => {
                let Type::Array(array) = ty else {
                    return Err(Error::coded(
                        "E0500",
                        span,
                        "internal: slice is not an array",
                    ));
                };
                let value = self.read_var(name, span)?;
                read_array_slice(value, *offset, *count, array.low, span)
            }
            ir::AssignTarget::SliceDynamic {
                name,
                first,
                last,
                up,
                low,
                ..
            } => {
                let value = self.read_var(name, span)?;
                let len = array_len(&value, span)?;
                let first_value = self.eval_ir(first)?;
                let (first_value, _) = self.expect_int(first_value, first.span())?;
                let last_value = self.eval_ir(last)?;
                let (last_value, _) = self.expect_int(last_value, last.span())?;
                let (offset, count) =
                    dynamic_slice_range(len, *low, first_value, last_value, *up, span)?;
                read_array_slice(value, offset, count, *low, span)
            }
            ir::AssignTarget::Deref { reference, ty } => {
                let cell = self.eval_reference(reference, span)?;
                let (value, cell_ty) = {
                    let var = cell.borrow();
                    (var.value.clone(), var.ty)
                };
                if cell_ty != *ty {
                    return Err(Error::coded(
                        "E0507",
                        span,
                        "reference mode does not match the referenced location",
                    ));
                }
                if matches!(value, Value::Undefined) {
                    return Err(Error::coded("E0504", span, "use of undefined value"));
                }
                self.convert(value, *ty, span)
            }
            ir::AssignTarget::DerefField {
                reference, path, ..
            } => {
                let cell = self.eval_reference(reference, span)?;
                let value = cell.borrow().value.clone();
                read_path(value, path, span)
            }
        }
    }

    fn assign_target(
        &mut self,
        target: &ir::AssignTarget,
        value: Value,
        span: Span,
    ) -> Result<(), Error> {
        match target {
            ir::AssignTarget::Var(name) => self.assign_var(name, value, span),
            ir::AssignTarget::Field {
                root,
                index,
                path,
                ty,
            } => {
                let object = self.task_object_key(root);
                if let Some(object) = object {
                    self.acquire_object(object)?;
                }
                let result = (|| {
                    let value = self.convert(value, *ty, span)?;
                    let index = match index {
                        Some(index) => {
                            let index_value = self.eval_ir(index)?;
                            let (index, _) = self.expect_int(index_value, index.span())?;
                            Some(index)
                        }
                        None => None,
                    };
                    let cell = self.find_var_cell(root).ok_or_else(|| {
                        Error::coded("E0505", span, format!("unknown variable `{root}`"))
                    })?;
                    let mut var = cell.borrow_mut();
                    let target = match index {
                        Some(index) => array_element_mut(&mut var.value, index, span)?,
                        None => &mut var.value,
                    };
                    assign_path(target, path, value, span)
                })();
                if let Some(object) = object {
                    self.release_object(object);
                }
                result
            }
            ir::AssignTarget::Index {
                name,
                indices,
                array,
                ty,
            } => {
                let value = self.convert(value, *ty, span)?;
                let mut index_values = Vec::with_capacity(indices.len());
                for index in indices {
                    let index_value = self.eval_ir(index)?;
                    let (index, _) = self.expect_int(index_value, index.span())?;
                    index_values.push(index);
                }
                let offset = array_offset(*array, &index_values, span)?;
                let cell = self.find_var_cell(name).ok_or_else(|| {
                    Error::coded("E0505", span, format!("unknown variable `{name}`"))
                })?;
                let mut cell = cell.borrow_mut();
                assign_array_at(&mut cell.value, offset, value, span)
            }
            ir::AssignTarget::Slice {
                name,
                offset,
                count,
                ty,
            } => {
                let value = self.convert(value, *ty, span)?;
                let cell = self.find_var_cell(name).ok_or_else(|| {
                    Error::coded("E0505", span, format!("unknown variable `{name}`"))
                })?;
                let mut var = cell.borrow_mut();
                assign_array_slice(&mut var.value, *offset, *count, value, span)
            }
            ir::AssignTarget::SliceDynamic {
                name,
                first,
                last,
                up,
                low,
                ty,
            } => {
                let value = self.convert(value, *ty, span)?;
                let first_value = self.eval_ir(first)?;
                let (first_value, _) = self.expect_int(first_value, first.span())?;
                let last_value = self.eval_ir(last)?;
                let (last_value, _) = self.expect_int(last_value, last.span())?;
                let cell = self.find_var_cell(name).ok_or_else(|| {
                    Error::coded("E0505", span, format!("unknown variable `{name}`"))
                })?;
                let mut var = cell.borrow_mut();
                let len = array_len(&var.value, span)?;
                let (offset, count) =
                    dynamic_slice_range(len, *low, first_value, last_value, *up, span)?;
                assign_array_slice(&mut var.value, offset, count, value, span)
            }
            ir::AssignTarget::Deref { reference, ty } => {
                let value = self.convert(value, *ty, span)?;
                let cell = self.eval_reference(reference, span)?;
                if cell.borrow().ty != *ty {
                    return Err(Error::coded(
                        "E0507",
                        span,
                        "reference mode does not match the referenced location",
                    ));
                }
                write_var_value(&mut cell.borrow_mut(), value);
                Ok(())
            }
            ir::AssignTarget::DerefField {
                reference,
                path,
                ty,
            } => {
                let value = self.convert(value, *ty, span)?;
                let cell = self.eval_reference(reference, span)?;
                let mut var = cell.borrow_mut();
                assign_path(&mut var.value, path, value, span)
            }
        }
    }

    fn read_var(&self, name: &str, span: Span) -> Result<Value, Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::coded("E0505", span, format!("unknown variable `{name}`")))?;
        let var = cell.borrow();
        if let Some(text) = &var.text {
            let record = text.borrow().record.clone();
            let value = record.borrow().value.clone();
            return match value {
                Value::Undefined => Err(Error::coded(
                    "E0504",
                    span,
                    format!("use of undefined value `{name}`"),
                )),
                value => Ok(value),
            };
        }
        match &var.value {
            Value::Undefined => Err(Error::coded(
                "E0504",
                span,
                format!("use of undefined value `{name}`"),
            )),
            value => Ok(value.clone()),
        }
    }

    fn assign_var(&mut self, name: &str, value: Value, span: Span) -> Result<(), Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::coded("E0505", span, format!("unknown variable `{name}`")))?;
        let ty = cell.borrow().ty;
        let value = self.convert(value, ty, span)?;
        write_var_value(&mut cell.borrow_mut(), value);
        Ok(())
    }

    fn find_var_cell(&self, name: &str) -> Option<VarCell> {
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(cell) = scope.vars.get(name) {
                return Some(cell.clone());
            }
            if let Some(exports) = scope.exports.get(name) {
                if let [ReachBinding {
                    kind: ReachKind::Var(cell),
                    meta,
                    ..
                }] = exports.as_slice()
                {
                    if meta.prefix.is_empty() {
                        return Some(cell.clone());
                    }
                }
            }
        }
        let module = self.active_module_scope()?;
        if let Some(cell) = module.vars.get(name) {
            return Some(cell.clone());
        }
        if let Some(exports) = module.exports.get(name) {
            if let [ReachBinding {
                kind: ReachKind::Var(cell),
                ..
            }] = exports.as_slice()
            {
                return Some(cell.clone());
            }
        }
        None
    }

    fn lookup_signal_modes(&self, name: &str) -> Option<Vec<Type>> {
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(modes) = scope.signals.get(name) {
                return Some(modes.clone());
            }
        }
        for scope in self.modules.iter().rev() {
            if let Some(modes) = scope.signals.get(name) {
                return Some(modes.clone());
            }
        }
        None
    }

    fn lookup_signal_target(&self, name: &str) -> Option<Option<String>> {
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(target) = scope.signal_targets.get(name) {
                return Some(target.clone());
            }
        }
        for scope in self.modules.iter().rev() {
            if let Some(target) = scope.signal_targets.get(name) {
                return Some(target.clone());
            }
        }
        None
    }

    fn lookup_process(&self, name: &str) -> Option<ProcessRef<'a>> {
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(process_ref) = scope.processes.get(name) {
                return Some(*process_ref);
            }
        }
        for scope in self.modules.iter().rev() {
            if let Some(process_ref) = scope.processes.get(name) {
                return Some(*process_ref);
            }
        }
        None
    }

    fn fork_for_process(&self, module: usize, scope_key: Option<Span>) -> Interpreter<'a> {
        Interpreter {
            profile: self.profile,
            modules: self.modules.clone(),
            locals: Vec::new(),
            call_bases: Vec::new(),
            active_module: module,
            active_scope_key: scope_key,
            module_floor: 0,
            depth: 0,
            yield_interval: self.yield_interval,
            yield_counter: 0,
            exports: self.exports.clone(),
            modes: Rc::clone(&self.modes),
            module_modes: self.module_modes.clone(),
            reaches: self.reaches.clone(),
            modules_by_span: self.modules_by_span.clone(),
            proc_defs: self.proc_defs.clone(),
            process_defs: self.process_defs.clone(),
            regions: self.regions.clone(),
            process_id: usize::MAX,
            process_name: String::new(),
            region_depths: HashMap::new(),
            object_depths: HashMap::new(),
            deadline: None,
            scheduler: Rc::clone(&self.scheduler),
            moreta_decls: self.moreta_decls.clone(),
            moreta_invariants: self.moreta_invariants.clone(),
            package_names: self.package_names.clone(),
            nested_scopes: self.nested_scopes.clone(),
            exprs: self.exprs.clone(),
            bodies: self.bodies.clone(),
            refs: Vec::new(),
            ref_allocated: Vec::new(),
            self_cells: Vec::new(),
            self_modes: Vec::new(),
        }
    }

    fn stop_task_worker(&mut self, object_key: usize) -> Result<(), Error> {
        self.acquire_object(object_key)?;
        {
            let mut scheduler = self.scheduler.borrow_mut();
            scheduler.task_stopped.insert(object_key);
            scheduler.task_jobs.remove(&object_key);
            if let Some(entry) = scheduler
                .task_waiters
                .get_mut(&object_key)
                .and_then(|waiters| waiters.pop())
            {
                scheduler.ready.push_back(Runnable {
                    input: ProcessInput::TaskReady,
                    entry,
                });
            }
        }
        self.release_object(object_key);
        Ok(())
    }

    fn start_task_worker(&mut self, cell: VarCell, span: Span) -> Result<(), Error> {
        let object_key = Rc::as_ptr(&cell) as usize;
        {
            let scheduler = self.scheduler.borrow();
            if scheduler.task_workers.contains(&object_key) {
                return Ok(());
            }
        }
        let id = {
            let mut scheduler = self.scheduler.borrow_mut();
            let id = scheduler.next_process_id;
            scheduler.next_process_id += 1;
            scheduler.task_workers.insert(object_key);
            id
        };
        let mut child = Box::new(self.fork_for_process(self.active_module, self.active_scope_key));
        child.process_id = id;
        child.process_name = "task".to_owned();
        let yielder = Rc::new(Cell::new(std::ptr::null()));
        let yielder_slot = Rc::clone(&yielder);
        let cell_slot = Rc::clone(&cell);
        // Safety: the task worker is owned by the scheduler and dropped before the
        // borrowed program/check data leaves `run_checked`.
        let coroutine = unsafe {
            Coroutine::with_stack_unchecked(
                DefaultStack::default(),
                move |current: &Yielder<ProcessInput, ProcessYield>, input: ProcessInput| {
                    let _ = input;
                    yielder_slot.set(current as *const _);
                    CURRENT_YIELDER.with(|slot| slot.set(current as *const _));
                    loop {
                        if child.scheduler.borrow().task_stopped.contains(&object_key) {
                            child
                                .scheduler
                                .borrow_mut()
                                .task_workers
                                .remove(&object_key);
                            return Ok(());
                        }
                        let job = {
                            let mut scheduler = child.scheduler.borrow_mut();
                            scheduler
                                .task_jobs
                                .get_mut(&object_key)
                                .and_then(|jobs| jobs.pop_front())
                        };
                        let Some(job) = job else {
                            let _ = cooperative_task_wait(object_key)?;
                            if child.scheduler.borrow().task_stopped.contains(&object_key) {
                                child
                                    .scheduler
                                    .borrow_mut()
                                    .task_workers
                                    .remove(&object_key);
                                return Ok(());
                            }
                            continue;
                        };
                        child.locals.push(Scope::default());
                        let temp = "\u{0}task-object".to_owned();
                        child
                            .locals
                            .last_mut()
                            .expect("task scope")
                            .vars
                            .insert(temp.clone(), Rc::clone(&cell_slot));
                        child.acquire_object(object_key)?;
                        let result = child.call_method(
                            &temp,
                            job.proc_span,
                            job.args.into_iter().map(CallArgument::Value).collect(),
                            span,
                            false,
                        );
                        child.release_object(object_key);
                        child.locals.pop();
                        result?;
                    }
                },
            )
        };
        let mut scheduler = self.scheduler.borrow_mut();
        scheduler.live_processes.insert(id);
        scheduler.process_names.insert(id, "task".to_owned());
        scheduler.ready.push_back(Runnable {
            input: ProcessInput::None,
            entry: ProcessEntry {
                id,
                coroutine,
                yielder,
            },
        });
        Ok(())
    }

    fn spawn_process(
        &mut self,
        process_ref: ProcessRef<'a>,
        values: Vec<CallArgument>,
        span: Span,
    ) -> Result<usize, Error> {
        if values.len() != process_ref.def.params.len() {
            return Err(Error::coded(
                "E0506",
                span,
                format!(
                    "process `{}` expects {} argument(s), found {}",
                    process_ref.def.name,
                    process_ref.def.params.len(),
                    values.len()
                ),
            ));
        }
        let id = {
            let mut scheduler = self.scheduler.borrow_mut();
            let id = scheduler.next_process_id;
            scheduler.next_process_id += 1;
            id
        };
        let mut child = Box::new(self.fork_for_process(process_ref.module, process_ref.scope_key));
        child.process_id = id;
        child.process_name = process_ref.def.name.clone();
        let yielder = Rc::new(Cell::new(std::ptr::null()));
        let yielder_slot = Rc::clone(&yielder);
        let def = process_ref.def;
        // Safety: the coroutine is owned by the scheduler and is dropped
        // before the borrowed program/check data leaves `run_checked`.
        let coroutine = unsafe {
            Coroutine::with_stack_unchecked(
                DefaultStack::default(),
                move |current: &Yielder<ProcessInput, ProcessYield>, input: ProcessInput| {
                    let _ = input;
                    yielder_slot.set(current as *const _);
                    CURRENT_YIELDER.with(|slot| slot.set(current as *const _));
                    child.run_process(def, values)
                },
            )
        };
        let mut scheduler = self.scheduler.borrow_mut();
        scheduler.live_processes.insert(id);
        scheduler
            .process_names
            .insert(id, process_ref.def.name.clone());
        scheduler.ready.push_back(Runnable {
            input: ProcessInput::None,
            entry: ProcessEntry {
                id,
                coroutine,
                yielder,
            },
        });
        Ok(id)
    }

    fn run_process(&mut self, def: &'a ProcessDef, args: Vec<CallArgument>) -> Result<(), Error> {
        self.locals.push(Scope::default());
        self.call_bases.push(self.locals.len() - 1);
        self.depth += 1;
        let result = (|| {
            for (param, arg) in def.params.iter().zip(args) {
                let ty = self.resolve_mode(&param.mode)?;
                let cell = match (param.passing, arg) {
                    (Passing::In, CallArgument::Value(value)) => Rc::new(RefCell::new(Var::new(
                        ty,
                        self.convert(value, ty, param.span)?,
                    ))),
                    (Passing::Loc, CallArgument::Location(cell)) => {
                        if cell.borrow().ty != ty {
                            return Err(Error::coded(
                                "E0507",
                                param.span,
                                format!(
                                    "LOC process argument for `{}` has the wrong mode",
                                    param.name
                                ),
                            ));
                        }
                        cell
                    }
                    _ => {
                        return Err(Error::coded(
                            "E0500",
                            param.span,
                            format!(
                                "internal: wrong argument kind for process parameter `{}`",
                                param.name
                            ),
                        ))
                    }
                };
                self.locals
                    .last_mut()
                    .expect("process scope")
                    .vars
                    .insert(param.name.clone(), cell);
            }
            self.declare_local(&def.declarations)?;
            self.initialize_declarations(&def.declarations)?;
            let body = self.bodies.get(def.span).ok_or_else(|| {
                Error::coded("E0500", def.span, "internal: missing process IR body")
            })?;
            let mut frame = Frame {
                result_type: None,
                result: None,
            };
            match self.exec_with_handler(def.span, &body, &mut frame)? {
                Flow::Normal | Flow::Return | Flow::Stop | Flow::Timeout | Flow::Exit(None) => {
                    Ok(())
                }
                Flow::Exit(Some(name)) => Err(Error::coded(
                    "E0508",
                    def.span,
                    format!("unknown EXIT label `{name}` in process `{}`", def.name),
                )),
            }
        })();
        self.depth -= 1;
        self.call_bases.pop();
        self.locals.pop();
        result
    }

    fn lookup_proc(&self, name: &str) -> Option<ProcRef<'a>> {
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(proc_ref) = scope.procs.get(name) {
                return Some(*proc_ref);
            }
        }
        let scope = self.active_module_scope()?;
        if let Some(proc_ref) = scope.procs.get(name) {
            return Some(*proc_ref);
        }
        if let Some(exports) = scope.exports.get(name) {
            if let [ReachBinding {
                kind: ReachKind::Proc(proc_ref),
                meta,
                ..
            }] = exports.as_slice()
            {
                if meta.prefix.is_empty() {
                    return Some(*proc_ref);
                }
            }
        }
        None
    }

    /// Scopes below this index belong to the caller and must not be visible
    /// to a module-level procedure body.
    fn local_floor(&self) -> usize {
        let call_floor = self.call_bases.last().copied().unwrap_or(0);
        call_floor.max(self.module_floor)
    }
}

fn collect_modules_by_span(program: &Program) -> HashMap<Span, &Module> {
    let mut map = HashMap::new();
    for module in &program.modules {
        collect_module_span(module, &mut map);
    }
    map
}

fn collect_process_defs(program: &Program) -> HashMap<Span, &ProcessDef> {
    fn collect<'a>(declarations: &'a [Declaration], map: &mut HashMap<Span, &'a ProcessDef>) {
        for declaration in declarations {
            match declaration {
                Declaration::Process(def) => {
                    map.insert(def.span, def);
                }
                Declaration::Mode(mode) => {
                    if let crate::ast::ModeDefinitionBody::Moreta { declarations, .. } = &mode.body
                    {
                        collect(declarations, map);
                    }
                }
                _ => {}
            }
        }
    }
    let mut map = HashMap::new();
    for module in &program.modules {
        collect(&module.declarations, &mut map);
    }
    map
}

fn collect_proc_defs(program: &Program) -> HashMap<Span, &ProcDef> {
    let mut map = HashMap::new();
    for module in &program.modules {
        collect_module_procs(module, &mut map);
    }
    map
}

fn collect_module_procs<'a>(module: &'a Module, map: &mut HashMap<Span, &'a ProcDef>) {
    collect_declaration_procs(&module.declarations, map);
    walk_action_modules(&module.actions, &mut |child| {
        collect_module_procs(child, map);
    });
}

fn collect_declaration_procs<'a>(
    declarations: &'a [Declaration],
    map: &mut HashMap<Span, &'a ProcDef>,
) {
    for declaration in declarations {
        match declaration {
            Declaration::Proc(proc_def) => {
                map.insert(proc_def.span, proc_def);
                collect_declaration_procs(&proc_def.declarations, map);
            }
            Declaration::Mode(mode_def) => {
                if let crate::ast::ModeDefinitionBody::Moreta { declarations, .. } = &mode_def.body
                {
                    collect_declaration_procs(declarations, map);
                }
            }
            _ => {}
        }
    }
}

fn collect_module_span<'a>(module: &'a Module, map: &mut HashMap<Span, &'a Module>) {
    map.insert(module.span, module);
    walk_action_modules(&module.actions, &mut |child| {
        collect_module_span(child, map);
    });
}

fn walk_action_modules<'a>(actions: &'a [Action], visit: &mut impl FnMut(&'a Module)) {
    for action in actions {
        match action {
            Action::Module(module) => visit(module),
            Action::If {
                branches,
                else_actions,
                ..
            } => {
                for branch in branches {
                    walk_action_modules(&branch.body, visit);
                }
                if let Some(actions) = else_actions {
                    walk_action_modules(actions, visit);
                }
            }
            Action::Do { body, .. } => walk_action_modules(body, visit),
            Action::Case {
                alternatives,
                else_actions,
                ..
            } => {
                for alternative in alternatives {
                    walk_action_modules(&alternative.body, visit);
                }
                if let Some(actions) = else_actions {
                    walk_action_modules(actions, visit);
                }
            }
            Action::Labelled { action, .. } => {
                walk_action_modules(std::slice::from_ref(action), visit)
            }
            Action::Begin { actions, .. } => walk_action_modules(actions, visit),
            Action::Handled { action, .. } => {
                walk_action_modules(std::slice::from_ref(action), visit)
            }
            _ => {}
        }
    }
}

fn declare_into_scope<'a>(
    scope: &mut Scope<'a>,
    declarations: &'a [Declaration],
    module: usize,
    mode_names: &HashMap<String, Type>,
    modes: &crate::modes::ModeTable,
    scope_key: Option<Span>,
) -> Result<(), Error> {
    for declaration in declarations {
        match declaration {
            Declaration::Var(vars) => {
                for var in vars {
                    let ty = resolve_mode_with(&var.mode, mode_names)?;
                    if scope.vars.contains_key(&var.name)
                        || scope.procs.contains_key(&var.name)
                        || scope.processes.contains_key(&var.name)
                    {
                        return Err(Error::coded(
                            "E0508",
                            var.span,
                            format!("duplicate declaration of `{}`", var.name),
                        ));
                    }
                    scope.vars.insert(
                        var.name.clone(),
                        Rc::new(RefCell::new(Var::new(ty, default_value(modes, ty)))),
                    );
                }
            }
            Declaration::Proc(proc_def) => {
                if scope.vars.contains_key(&proc_def.name)
                    || scope.procs.contains_key(&proc_def.name)
                {
                    return Err(Error::coded(
                        "E0508",
                        proc_def.span,
                        format!("duplicate declaration of `{}`", proc_def.name),
                    ));
                }
                scope.procs.insert(
                    proc_def.name.clone(),
                    ProcRef {
                        def: proc_def,
                        module,
                        scope_key,
                    },
                );
            }
            Declaration::Signal(defs) => {
                for def in defs {
                    let modes = def
                        .modes
                        .iter()
                        .map(|mode| resolve_mode_with(mode, mode_names))
                        .collect::<Result<Vec<_>, _>>()?;
                    scope.signals.insert(def.name.clone(), modes);
                    scope
                        .signal_targets
                        .insert(def.name.clone(), def.to_process.clone());
                }
            }
            Declaration::Process(process_def) => {
                if scope.vars.contains_key(&process_def.name)
                    || scope.procs.contains_key(&process_def.name)
                    || scope.processes.contains_key(&process_def.name)
                {
                    return Err(Error::coded(
                        "E0508",
                        process_def.span,
                        format!("duplicate declaration of `{}`", process_def.name),
                    ));
                }
                scope.processes.insert(
                    process_def.name.clone(),
                    ProcessRef {
                        def: process_def,
                        module,
                        scope_key,
                    },
                );
            }
            Declaration::ProcSignature(_)
            | Declaration::Visibility(_)
            | Declaration::Mode(_)
            | Declaration::SpecModule(_)
            | Declaration::GenericProc(_)
            | Declaration::GenericProcInstance(_)
            | Declaration::GenericProcess(_)
            | Declaration::GenericProcessInstance(_) => {}
        }
    }
    Ok(())
}

fn array_offset(array: Type, indices: &[i128], span: Span) -> Result<i128, Error> {
    match array {
        Type::Array(array) => {
            if indices.len() != 1 {
                return Err(Error::coded(
                    "E0507",
                    span,
                    "array element requires one index",
                ));
            }
            let index = indices[0];
            if index < array.low || index > array.high {
                return Err(Error::coded(
                    "E0500",
                    span,
                    format!("array index `{index}` is out of bounds"),
                ));
            }
            Ok(index - array.low)
        }
        Type::MultiArray(array) => {
            if indices.len() != array.rank() {
                return Err(Error::coded(
                    "E0507",
                    span,
                    format!("array element requires {} indices", array.rank()),
                ));
            }
            let mut offset = 0;
            for (index, dim) in indices.iter().zip(array.dimensions()) {
                if *index < dim.low || *index > dim.high {
                    return Err(Error::coded(
                        "E0500",
                        span,
                        format!("array index `{index}` is out of bounds"),
                    ));
                }
                offset = offset * dim.length() + (*index - dim.low);
            }
            Ok(offset)
        }
        _ => Err(Error::coded("E0507", span, "expected an array value")),
    }
}

fn array_len(value: &Value, span: Span) -> Result<usize, Error> {
    match value {
        Value::Array { values, .. } => Ok(values.len()),
        _ => Err(Error::coded("E0507", span, "expected an array value")),
    }
}

fn dynamic_slice_range(
    len: usize,
    low: i128,
    first: i128,
    last: i128,
    up: bool,
    span: Span,
) -> Result<(i128, i128), Error> {
    let upper = if up {
        if last < 1 {
            return Err(Error::coded(
                "E0500",
                span,
                "array slice size must be positive",
            ));
        }
        first
            .checked_add(last - 1)
            .ok_or_else(|| Error::coded("E0500", span, "array slice bounds overflow"))?
    } else {
        last
    };
    let high = low
        .checked_add(len as i128 - 1)
        .ok_or_else(|| Error::coded("E0500", span, "array slice bounds overflow"))?;
    if first > upper || first < low || upper > high {
        return Err(Error::coded(
            "E0500",
            span,
            "array slice range is out of bounds",
        ));
    }
    Ok((first - low, upper - first + 1))
}

fn read_array_slice(
    value: Value,
    offset: i128,
    count: i128,
    low: i128,
    span: Span,
) -> Result<Value, Error> {
    let Value::Array { values, .. } = value else {
        return Err(Error::coded("E0507", span, "expected an array value"));
    };
    if offset < 0 || count < 0 || offset + count > values.len() as i128 {
        return Err(Error::coded("E0500", span, "array slice is out of bounds"));
    }
    let start = offset as usize;
    let end = (offset + count) as usize;
    Ok(Value::Array {
        low,
        values: values[start..end].to_vec(),
    })
}

fn read_array_at(value: Value, offset: i128, span: Span) -> Result<Value, Error> {
    let Value::Array { values, .. } = value else {
        return Err(Error::coded("E0507", span, "expected an array value"));
    };
    if offset < 0 || offset as usize >= values.len() {
        return Err(Error::coded("E0500", span, "array index is out of bounds"));
    }
    match &values[offset as usize] {
        Value::Undefined => Err(Error::coded(
            "E0500",
            span,
            "use of undefined array element",
        )),
        value => Ok(value.clone()),
    }
}

fn assign_array_at(
    value: &mut Value,
    offset: i128,
    new_value: Value,
    span: Span,
) -> Result<(), Error> {
    let Value::Array { values, .. } = value else {
        return Err(Error::coded("E0507", span, "expected an array value"));
    };
    if offset < 0 || offset as usize >= values.len() {
        return Err(Error::coded("E0500", span, "array index is out of bounds"));
    }
    values[offset as usize] = new_value;
    Ok(())
}

fn assign_array_slice(
    value: &mut Value,
    offset: i128,
    count: i128,
    new_value: Value,
    span: Span,
) -> Result<(), Error> {
    let Value::Array { values, .. } = value else {
        return Err(Error::coded("E0507", span, "expected an array value"));
    };
    let Value::Array {
        values: new_values, ..
    } = new_value
    else {
        return Err(Error::coded("E0507", span, "expected an array value"));
    };
    if offset < 0
        || count < 0
        || offset + count > values.len() as i128
        || new_values.len() != count as usize
    {
        return Err(Error::coded("E0500", span, "array slice is out of bounds"));
    }
    let start = offset as usize;
    for (index, item) in new_values.into_iter().enumerate() {
        values[start + index] = item;
    }
    Ok(())
}

fn read_array_element(value: Value, index: i128, span: Span) -> Result<Value, Error> {
    let Value::Array { low, values } = value else {
        return Err(Error::coded("E0507", span, "expected an array value"));
    };
    let offset = index - low;
    if offset < 0 || offset as usize >= values.len() {
        return Err(Error::coded(
            "E0500",
            span,
            format!("array index `{index}` is out of bounds"),
        ));
    }
    match &values[offset as usize] {
        Value::Undefined => Err(Error::coded(
            "E0500",
            span,
            format!("use of undefined array element `{index}`"),
        )),
        value => Ok(value.clone()),
    }
}

fn array_element_mut(value: &mut Value, index: i128, span: Span) -> Result<&mut Value, Error> {
    let Value::Array { low, values } = value else {
        return Err(Error::coded("E0507", span, "expected an array value"));
    };
    let offset = index - *low;
    if offset < 0 || offset as usize >= values.len() {
        return Err(Error::coded(
            "E0500",
            span,
            format!("array index `{index}` is out of bounds"),
        ));
    }
    Ok(&mut values[offset as usize])
}

fn read_field(value: Value, field: &str, span: Span) -> Result<Value, Error> {
    let fields = match value {
        Value::Struct(fields) | Value::Moreta(fields) => fields,
        _ => {
            return Err(Error::coded(
                "E0507",
                span,
                "expected a structure or module mode value",
            ))
        }
    };
    let Some((_, value)) = fields.iter().find(|(name, _)| name == field) else {
        return Err(Error::coded(
            "E0500",
            span,
            format!("unknown field `{field}`"),
        ));
    };
    match value {
        Value::Undefined => Err(Error::coded(
            "E0504",
            span,
            format!("use of undefined value `{field}`"),
        )),
        value => Ok(value.clone()),
    }
}

fn read_path(value: Value, path: &[String], span: Span) -> Result<Value, Error> {
    let mut value = value;
    for field in path {
        value = read_field(value, field, span)?;
    }
    Ok(value)
}

fn assign_path(
    value: &mut Value,
    path: &[String],
    new_value: Value,
    span: Span,
) -> Result<(), Error> {
    let fields = match value {
        Value::Struct(fields) | Value::Moreta(fields) => fields,
        _ => {
            return Err(Error::coded(
                "E0507",
                span,
                "expected a structure or module mode value",
            ))
        }
    };
    let Some((_, slot)) = fields.iter_mut().find(|(name, _)| name == &path[0]) else {
        return Err(Error::coded(
            "E0500",
            span,
            format!("unknown field `{}`", path[0]),
        ));
    };
    if path.len() == 1 {
        *slot = new_value;
        Ok(())
    } else {
        assign_path(slot, &path[1..], new_value, span)
    }
}

fn resolve_mode_with(mode: &ModeRef, mode_names: &HashMap<String, Type>) -> Result<Type, Error> {
    types::resolve_mode_ref(mode, mode_names).ok_or_else(|| {
        Error::coded(
            "E0500",
            mode.span,
            format!("unknown or unsupported mode `{}`", mode.name),
        )
    })
}

fn powerset_member_rep(
    member: crate::types::PowerSetMember,
    value: Value,
    span: Span,
) -> Result<i128, Error> {
    match (member, value) {
        (crate::types::PowerSetMember::Int(_), Value::Int { value, .. }) => Ok(value),
        (
            crate::types::PowerSetMember::Char | crate::types::PowerSetMember::WChar,
            Value::Char(value),
        ) => Ok(value as i128),
        (crate::types::PowerSetMember::Bool, Value::Bool(value)) => Ok(i128::from(value)),
        _ => Err(Error::coded("E0500", span, "invalid powerset member value")),
    }
}

fn check_index_range(
    range: Option<types::ArrayDimension>,
    index: i128,
    span: Span,
) -> Result<(), Error> {
    let Some(range) = range else {
        return Ok(());
    };
    if index < range.low || index > range.high {
        return Err(Error::exception(
            "RANGEFAIL",
            span,
            format!("RANGEFAIL: record index `{index}` is out of range"),
        ));
    }
    Ok(())
}

fn write_line_at(path: &str, index: usize, record: &str) -> std::io::Result<()> {
    let contents = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = contents.lines().map(str::to_owned).collect();
    while lines.len() <= index {
        lines.push(String::new());
    }
    lines[index] = record.to_owned();
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(path, text)
}

fn textfail(span: Span, message: &str) -> Error {
    Error::exception("TEXTFAIL", span, format!("TEXTFAIL: {message}"))
}

fn write_text_at(
    chars: &mut Vec<char>,
    cursor: &mut i128,
    text: &str,
    span: Span,
) -> Result<(), Error> {
    if *cursor < 0 {
        return Err(textfail(span, "negative text index"));
    }
    let mut position = *cursor as usize;
    while chars.len() < position {
        chars.push(' ');
    }
    for ch in text.chars() {
        if position < chars.len() {
            chars[position] = ch;
        } else {
            chars.push(ch);
        }
        position += 1;
    }
    *cursor = position as i128;
    Ok(())
}

fn pad_to(chars: &mut Vec<char>, cursor: usize) {
    while chars.len() < cursor {
        chars.push(' ');
    }
}

fn apply_io_write(
    chars: &mut Vec<char>,
    cursor: &mut i128,
    control: crate::builtins::IoControl,
    span: Span,
) -> Result<(), Error> {
    match control {
        crate::builtins::IoControl::NextRecord | crate::builtins::IoControl::Prompt => {
            write_text_at(chars, cursor, "\n", span)
        }
        crate::builtins::IoControl::NextPage => write_text_at(chars, cursor, "\x0c", span),
        crate::builtins::IoControl::CurrentLine
        | crate::builtins::IoControl::Emit
        | crate::builtins::IoControl::EndPage => Ok(()),
    }
}

fn apply_io_read(
    chars: &[char],
    cursor: &mut i128,
    control: crate::builtins::IoControl,
    span: Span,
) -> Result<(), Error> {
    let expected = match control {
        crate::builtins::IoControl::NextRecord | crate::builtins::IoControl::Prompt => Some('\n'),
        crate::builtins::IoControl::NextPage => Some('\x0c'),
        crate::builtins::IoControl::CurrentLine
        | crate::builtins::IoControl::Emit
        | crate::builtins::IoControl::EndPage => None,
    };
    if let Some(expected) = expected {
        let position = *cursor as usize;
        if chars.get(position) != Some(&expected) {
            return Err(textfail(span, "expected a record separator"));
        }
        *cursor += 1;
    }
    Ok(())
}

fn apply_editing_write(
    chars: &mut Vec<char>,
    cursor: &mut i128,
    editing: &crate::builtins::Editing,
    width: usize,
    span: Span,
) -> Result<(), Error> {
    match editing.code {
        crate::builtins::EditingCode::Space => {
            write_text_at(chars, cursor, &" ".repeat(width), span)
        }
        crate::builtins::EditingCode::SkipRight => {
            *cursor += width as i128;
            pad_to(chars, *cursor as usize);
            Ok(())
        }
        crate::builtins::EditingCode::SkipLeft => {
            *cursor -= width as i128;
            if *cursor < 0 {
                return Err(textfail(span, "text index moved before the text"));
            }
            Ok(())
        }
        crate::builtins::EditingCode::Tab => {
            *cursor = width as i128;
            pad_to(chars, *cursor as usize);
            Ok(())
        }
    }
}

fn apply_editing_read(
    chars: &[char],
    cursor: &mut i128,
    editing: &crate::builtins::Editing,
    width: usize,
    span: Span,
) -> Result<(), Error> {
    match editing.code {
        crate::builtins::EditingCode::Space => {
            let start = *cursor as usize;
            let end = start.saturating_add(width);
            if end > chars.len() || !chars[start..end].iter().all(|ch| matches!(ch, ' ' | '\t')) {
                return Err(textfail(span, "expected space characters"));
            }
            *cursor = end as i128;
            Ok(())
        }
        crate::builtins::EditingCode::SkipRight => {
            *cursor += width as i128;
            if *cursor as usize > chars.len() {
                return Err(textfail(span, "text index moved past the text"));
            }
            Ok(())
        }
        crate::builtins::EditingCode::SkipLeft => {
            *cursor -= width as i128;
            if *cursor < 0 {
                return Err(textfail(span, "text index moved before the text"));
            }
            Ok(())
        }
        crate::builtins::EditingCode::Tab => {
            *cursor = width as i128;
            if *cursor as usize > chars.len() {
                return Err(textfail(span, "text index moved past the text"));
            }
            Ok(())
        }
    }
}

fn format_value(
    value: &Value,
    conversion: &crate::builtins::Conversion,
    width: usize,
    span: Span,
) -> Result<String, Error> {
    let representation = match (conversion.code, value) {
        (crate::builtins::ConversionCode::Default, Value::Int { value, .. }) => value.to_string(),
        (crate::builtins::ConversionCode::Binary, Value::Int { value, .. }) => {
            format_integer_base(*value, 2)
        }
        (crate::builtins::ConversionCode::Octal, Value::Int { value, .. }) => {
            format_integer_base(*value, 8)
        }
        (crate::builtins::ConversionCode::Hex, Value::Int { value, .. }) => {
            format_integer_base(*value, 16)
        }
        (crate::builtins::ConversionCode::Default, Value::Bool(value)) => {
            if *value { "TRUE" } else { "FALSE" }.to_owned()
        }
        (crate::builtins::ConversionCode::Default, Value::Char(value)) => value.to_string(),
        (crate::builtins::ConversionCode::Default, Value::String(value)) => value.clone(),
        (crate::builtins::ConversionCode::Default, Value::Float(value)) => {
            match conversion.fractional_width {
                Some(width) => format!("{value:.width$}"),
                None => value.to_string(),
            }
        }
        (crate::builtins::ConversionCode::Scientific, Value::Float(value)) => format_scientific(
            *value,
            conversion.fractional_width,
            conversion.exponent_width,
        ),
        _ => {
            return Err(Error::coded(
                "E0507",
                span,
                "format conversion is not compatible with the value",
            ));
        }
    };
    Ok(fit_width(representation, width, conversion))
}

fn format_scientific(
    value: f64,
    fractional_width: Option<usize>,
    exponent_width: Option<usize>,
) -> String {
    let formatted = match fractional_width {
        Some(width) => format!("{value:.width$e}"),
        None => format!("{value:e}"),
    };
    let Some((mantissa, exponent)) = formatted.split_once('e') else {
        return formatted;
    };
    let (sign, digits) = if let Some(digits) = exponent.strip_prefix('-') {
        ("-", digits)
    } else if let Some(digits) = exponent.strip_prefix('+') {
        ("+", digits)
    } else {
        ("", exponent)
    };
    let width = exponent_width.unwrap_or(3);
    format!("{mantissa}e{sign}{digits:0>width$}")
}

fn format_integer_base(value: i128, base: u32) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let magnitude = value.unsigned_abs();
    let digits = match base {
        2 => format!("{magnitude:b}"),
        8 => format!("{magnitude:o}"),
        _ => format!("{magnitude:x}"),
    };
    format!("{sign}{digits}")
}

fn fit_width(
    mut representation: String,
    width: usize,
    conversion: &crate::builtins::Conversion,
) -> String {
    if width == 0 {
        return representation;
    }
    let length = representation.chars().count();
    if length > width {
        if conversion.overflow {
            return "*".repeat(width);
        }
        let truncated: String = if conversion.left {
            representation.chars().take(width).collect()
        } else {
            representation.chars().skip(length - width).collect()
        };
        return truncated;
    }
    let padding = width - length;
    if conversion.left {
        representation.extend(std::iter::repeat_n(conversion.pad, padding));
        representation
    } else {
        let mut result: String = std::iter::repeat_n(conversion.pad, padding).collect();
        result.push_str(&representation);
        result
    }
}

fn read_text_exact(chars: &[char], cursor: &mut i128, text: &str, span: Span) -> Result<(), Error> {
    let start = *cursor as usize;
    let expected: Vec<char> = text.chars().collect();
    if start.saturating_add(expected.len()) > chars.len()
        || chars[start..start + expected.len()] != expected[..]
    {
        return Err(textfail(span, "text does not match the format"));
    }
    *cursor += expected.len() as i128;
    Ok(())
}

fn read_conversion(
    chars: &[char],
    cursor: &mut i128,
    conversion: &crate::builtins::Conversion,
    width: usize,
    target: Type,
    span: Span,
) -> Result<Value, Error> {
    let start = *cursor as usize;
    if start > chars.len() {
        return Err(textfail(span, "text index is past the text"));
    }
    if width > 0 {
        let end = start.saturating_add(width);
        if end > chars.len() {
            return Err(textfail(span, "text slice is out of bounds"));
        }
        let field: String = chars[start..end].iter().collect();
        *cursor = end as i128;
        let value = if is_string_type_for_io(target) {
            Value::String(field)
        } else {
            let trimmed = if conversion.left {
                field.trim_end_matches(conversion.pad)
            } else {
                field.trim_start_matches(conversion.pad)
            };
            parse_external(trimmed, conversion, target, span)?
        };
        return Ok(value);
    }

    let value = match target {
        Type::Bool => {
            let available: String = chars[start..].iter().collect();
            let text = available.trim_start_matches([' ', '\t']);
            let skipped = available.len() - text.len();
            *cursor = (start + skipped) as i128;
            let upper = text.to_ascii_uppercase();
            if upper.starts_with("TRUE") {
                *cursor += 4;
                Value::Bool(true)
            } else if upper.starts_with("FALSE") {
                *cursor += 5;
                Value::Bool(false)
            } else {
                return Err(textfail(span, "expected TRUE or FALSE"));
            }
        }
        Type::Char | Type::WChar => {
            let Some(ch) = chars.get(start) else {
                return Err(textfail(span, "expected a character"));
            };
            if target == Type::Char && (*ch as u32) > 0xFF {
                return Err(textfail(span, "character is outside the CHAR range"));
            }
            *cursor = (start + 1) as i128;
            Value::Char(*ch)
        }
        Type::String | Type::BoundedString { .. } | Type::WString | Type::WBoundedString { .. } => {
            let text: String = chars[start..].iter().collect();
            *cursor = chars.len() as i128;
            Value::String(text)
        }
        Type::Float => {
            let mut end = start;
            while end < chars.len() && matches!(chars[end], ' ' | '\t') {
                end += 1;
            }
            let mut token = String::new();
            if end < chars.len() && matches!(chars[end], '+' | '-') {
                token.push(chars[end]);
                end += 1;
            }
            while end < chars.len()
                && (chars[end].is_ascii_digit()
                    || matches!(chars[end], '.' | 'e' | 'E' | '+' | '-'))
            {
                token.push(chars[end]);
                end += 1;
            }
            *cursor = end as i128;
            parse_external(&token, conversion, target, span)?
        }
        Type::Int(_) | Type::IntLiteral => {
            let mut end = start;
            while end < chars.len() && matches!(chars[end], ' ' | '\t') {
                end += 1;
            }
            let mut token = String::new();
            if end < chars.len() && matches!(chars[end], '+' | '-') {
                token.push(chars[end]);
                end += 1;
            }
            while end < chars.len() && is_digit_for(chars[end], &conversion.code) {
                token.push(chars[end]);
                end += 1;
            }
            *cursor = end as i128;
            parse_external(&token, conversion, target, span)?
        }
        _ => {
            return Err(Error::coded(
                "E0507",
                span,
                "format conversion is not compatible with the target mode",
            ));
        }
    };
    Ok(value)
}

fn is_string_type_for_io(ty: Type) -> bool {
    matches!(
        ty,
        Type::String | Type::BoundedString { .. } | Type::WString | Type::WBoundedString { .. }
    )
}

fn is_digit_for(ch: char, code: &crate::builtins::ConversionCode) -> bool {
    match code {
        crate::builtins::ConversionCode::Binary => matches!(ch, '0' | '1'),
        crate::builtins::ConversionCode::Octal => ('0'..='7').contains(&ch),
        crate::builtins::ConversionCode::Hex => ch.is_ascii_hexdigit(),
        crate::builtins::ConversionCode::Default | crate::builtins::ConversionCode::Scientific => {
            ch.is_ascii_digit()
        }
    }
}

fn parse_external(
    text: &str,
    conversion: &crate::builtins::Conversion,
    target: Type,
    span: Span,
) -> Result<Value, Error> {
    match target {
        Type::Int(kind) => {
            let (negative, digits) = if let Some(rest) = text.strip_prefix('-') {
                (true, rest)
            } else if let Some(rest) = text.strip_prefix('+') {
                (false, rest)
            } else {
                (false, text)
            };
            let base = match conversion.code {
                crate::builtins::ConversionCode::Binary => 2,
                crate::builtins::ConversionCode::Octal => 8,
                crate::builtins::ConversionCode::Hex => 16,
                crate::builtins::ConversionCode::Default
                | crate::builtins::ConversionCode::Scientific => 10,
            };
            let magnitude = i128::from_str_radix(digits, base)
                .map_err(|_| textfail(span, "invalid integer in text"))?;
            let value = if negative { -magnitude } else { magnitude };
            Ok(Value::Int {
                value,
                kind: Some(kind),
            })
        }
        Type::Float => text
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| textfail(span, "invalid floating-point value in text")),
        Type::Bool => match text.to_ascii_uppercase().as_str() {
            "TRUE" => Ok(Value::Bool(true)),
            "FALSE" => Ok(Value::Bool(false)),
            _ => Err(textfail(span, "expected TRUE or FALSE")),
        },
        Type::Char | Type::WChar => {
            let mut chars = text.chars();
            let Some(ch) = chars.next() else {
                return Err(textfail(span, "expected a character"));
            };
            if chars.next().is_some() {
                return Err(textfail(span, "expected one character"));
            }
            if target == Type::Char && (ch as u32) > 0xFF {
                return Err(textfail(span, "character is outside the CHAR range"));
            }
            Ok(Value::Char(ch))
        }
        Type::String | Type::BoundedString { .. } | Type::WString | Type::WBoundedString { .. } => {
            Ok(Value::String(text.to_owned()))
        }
        _ => Err(Error::coded(
            "E0507",
            span,
            "unsupported text conversion target",
        )),
    }
}

fn bits_binary(op: &str, left: &str, right: &str, span: Span) -> Result<String, Error> {
    if left.len() != right.len() {
        return Err(Error::coded(
            "E0507",
            span,
            "bit strings must have the same length",
        ));
    }
    Ok(left
        .chars()
        .zip(right.chars())
        .map(|(left, right)| {
            let value = match op {
                "and" => left == '1' && right == '1',
                "or" => left == '1' || right == '1',
                "xor" => left != right,
                _ => false,
            };
            if value {
                '1'
            } else {
                '0'
            }
        })
        .collect())
}

fn unix_time_ms() -> i128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i128)
        .unwrap_or(0)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i128 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    (era * 146_097 + day_of_era - 719_468) as i128
}

fn default_value(modes: &crate::modes::ModeTable, ty: Type) -> Value {
    match ty {
        Type::Struct(id) => Value::Struct(
            modes
                .get(id)
                .fields
                .iter()
                .map(|field| (field.name.clone(), default_value(modes, field.ty)))
                .collect(),
        ),
        Type::Moreta(id) => Value::Moreta(
            modes
                .get_moreta(id)
                .components
                .iter()
                .filter_map(|component| match component {
                    crate::modes::MoretaComponent::Var { name, ty, .. } => {
                        Some((name.clone(), default_value(modes, *ty)))
                    }
                    crate::modes::MoretaComponent::Proc { .. }
                    | crate::modes::MoretaComponent::Mode { .. } => None,
                })
                .collect(),
        ),
        Type::Array(array) => {
            let element = array.element.ty();
            let value = if matches!(element, Type::Struct(_)) {
                default_value(modes, element)
            } else {
                Value::Undefined
            };
            Value::Array {
                low: array.low,
                values: vec![value; (array.high - array.low + 1).max(0) as usize],
            }
        }
        Type::Association => Value::Association(None),
        Type::Access(_) => Value::Access(None),
        Type::PowerSet(_) => Value::PowerSet(std::collections::BTreeSet::new()),
        Type::Text { .. } => Value::String(String::new()),
        Type::Set(_) => Value::Undefined,
        Type::MultiArray(array) => {
            let element = array.element.ty();
            let value = if matches!(element, Type::Struct(_)) {
                default_value(modes, element)
            } else {
                Value::Undefined
            };
            Value::Array {
                low: array.dimensions()[0].low,
                values: vec![value; array.total_len().max(0) as usize],
            }
        }
        _ => Value::Undefined,
    }
}

fn binary_for_assign(op: AssignOp) -> BinaryOp {
    match op {
        AssignOp::Assign => BinaryOp::Eq,
        AssignOp::Add => BinaryOp::Add,
        AssignOp::Sub => BinaryOp::Sub,
        AssignOp::Mul => BinaryOp::Mul,
        AssignOp::Div => BinaryOp::Div,
        AssignOp::Concat => BinaryOp::Concat,
    }
}

fn explicit_label_matches(label: CaseLabel, value: &Value) -> bool {
    match (label, value) {
        (CaseLabel::Value(label_value), Value::Int { value, .. }) => label_value == *value,
        (CaseLabel::Bool(label_value), Value::Bool(value)) => label_value == *value,
        (CaseLabel::Char(label_value), Value::Char(value)) => label_value == *value,
        (CaseLabel::Range { low, high }, Value::Int { value, .. }) => {
            low <= *value && *value <= high
        }
        (CaseLabel::CharRange { low, high }, Value::Char(value)) => low <= *value && *value <= high,
        _ => false,
    }
}

fn type_kind(ty: Type) -> Option<IntKind> {
    match ty {
        Type::Int(kind) => Some(kind),
        Type::IntLiteral => None,
        _ => None,
    }
}

fn combine_kinds(
    left: Option<IntKind>,
    right: Option<IntKind>,
    span: Span,
) -> Result<Option<IntKind>, Error> {
    match (left, right) {
        (Some(left), Some(right)) if left == right => Ok(Some(left)),
        (Some(left), Some(right)) => Err(Error::coded(
            "E0500",
            span,
            format!(
                "integer mode mismatch: {} and {}",
                left.name(),
                right.name()
            ),
        )),
        (Some(kind), None) | (None, Some(kind)) => Ok(Some(kind)),
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::manifest::WidthProfile;
    use crate::parser;

    fn run_source(source: &str) -> Result<(), Error> {
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(&tokens).unwrap();
        run(&program, WidthProfile::default())
    }

    #[test]
    fn runs_procedures_example() {
        run_source(include_str!("../examples/procedures/src/main.ch")).unwrap();
    }

    #[test]
    fn assert_failure_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  ASSERT 1 = 2;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0501");
        assert_eq!(error.message, "ASSERTFAIL");
    }

    #[test]
    fn integer_overflow_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  DCL x SHORT_INT INIT := 32767;
  ASSERT x + 1 = 32768;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("OVERFLOW"));
    }

    #[test]
    fn procedure_result_is_returned() {
        run_source(
            r#"
M: MODULE
  Add: PROC (a INT, b INT) RETURNS (INT);
    RETURN (a + b);
  END Add;
  ASSERT Add(2, 3) = 5;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_across_modules() {
        run_source(
            r#"
A: MODULE
  GRANT x, F;
  DCL x INT INIT := 41;
  DCL secret INT INIT := 1;
  F: PROC () RETURNS (INT);
    RETURN (x + secret);
  END F;
END A;

B: MODULE
  SEIZE x, F;
  ASSERT x = 41;
  ASSERT F() = 42;
  x := x + 1;
END B;

C: MODULE
  SEIZE x;
  ASSERT x = 42;
END C;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_prefixed_exports() {
        run_source(
            r#"
A: MODULE
  GRANT ALL PREFIXED P;
  DCL a INT INIT := 1;
END A;

B: MODULE
  SEIZE P!a;
  ASSERT a = 1;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_nested_modules_and_shares_seized_locations() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  Helper: MODULE
    SEIZE x;
    x := x + 1;
  END Helper;
  ASSERT x = 2;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_procedures_and_modes_can_be_granted_outward() {
        run_source(
            r#"
M: MODULE
  GRANT P, S;
  A: MODULE
    GRANT P, S;
    NEWMODE S = STRUCT(x INT);
    P: PROC () RETURNS (INT);
      RETURN 1;
    END P;
  END A;
END M;

N: MODULE
  SEIZE P, S;
  DCL s S;
  s.x := 1;
  ASSERT s.x = 1;
  ASSERT P() = 1;
END N;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_modules_can_define_procedures() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  A: MODULE
    SEIZE x;
    Add: PROC () RETURNS (INT);
      RETURN x + 1;
    END Add;
    ASSERT Add() = 2;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn program_normalization_is_idempotent() {
        let tokens = lexer::tokenize(
            r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    DCL x INT;
    GRANT x;
  END Inner;
END Outer;

Outer: MODULE
  Inner: MODULE
    DCL x INT INIT := 1;
  END Inner;
  ASSERT x = 1;
END Outer;
"#,
        )
        .unwrap();
        let program = parser::parse(&tokens).unwrap();
        let once = crate::program::normalize(&program).unwrap();
        let twice = crate::program::normalize(&once).unwrap();
        assert_eq!(once.modules.len(), twice.modules.len());
    }

    #[test]
    fn direct_entry_points_apply_virtual_grants() {
        run_source(
            r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    DCL x INT;
    GRANT x;
  END Inner;
END Outer;

Outer: MODULE
  Inner: MODULE
    DCL x INT INIT := 1;
  END Inner;
  ASSERT x = 1;
END Outer;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_modules_can_define_modes() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    NEWMODE S = STRUCT(x INT);
    DCL s S;
    s.x := 1;
    ASSERT s.x = 1;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_modules_can_seize_parent_modes() {
        run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  GRANT S;
  A: MODULE
    SEIZE S;
    DCL s S;
    s.x := 2;
    ASSERT s.x = 2;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_nested_modules_with_contexts() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 3;
  CONTEXT
    DCL x INT;
  FOR
  Helper: MODULE
    SEIZE x;
    ASSERT x = 3;
  END Helper;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn parent_regrants_a_child_grant_at_runtime() {
        run_source(
            r#"
M: MODULE
  GRANT x;
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
END M;

N: MODULE
  SEIZE x;
  ASSERT x = 1;
END N;
"#,
        )
        .unwrap();
    }

    #[test]
    fn parent_regrants_a_grandchild_grant_at_runtime() {
        run_source(
            r#"
M: MODULE
  GRANT x;
  A: MODULE
    GRANT x;
    B: MODULE
      GRANT x;
      DCL x INT INIT := 2;
    END B;
  END A;
END M;

N: MODULE
  SEIZE x;
  ASSERT x = 2;
END N;
"#,
        )
        .unwrap();
    }

    #[test]
    fn parent_reach_can_use_a_child_grant() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
  ASSERT x = 1;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_parent_can_use_a_grandchild_grant() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    B: MODULE
      GRANT x;
      DCL x INT INIT := 2;
    END B;
    ASSERT x = 2;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn modules_inside_a_begin_share_the_block_reach() {
        run_source(
            r#"
M: MODULE
  BEGIN
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
    B: MODULE
      SEIZE x;
      ASSERT x = 1;
    END B;
  END;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn modules_inside_a_do_loop_share_the_body_reach() {
        run_source(
            r#"
M: MODULE
  DO FOR i := 1 TO 1;
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
    B: MODULE
      SEIZE x;
      ASSERT x = 1;
    END B;
  OD;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn modules_inside_a_case_alternative_share_its_reach() {
        run_source(
            r#"
M: MODULE
  DCL k INT INIT := 1;
  CASE k OF
    (1):
      A: MODULE
        GRANT x;
        DCL x INT INIT := 1;
      END A;
      B: MODULE
        SEIZE x;
        ASSERT x = 1;
      END B;
    (ELSE): ;
  ESAC;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn modules_inside_an_if_branch_share_the_branch_reach() {
        run_source(
            r#"
M: MODULE
  DCL flag BOOL INIT := TRUE;
  IF flag THEN
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
    B: MODULE
      SEIZE x;
      ASSERT x = 1;
    END B;
  FI;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_grant_prefixes_are_seizable_at_runtime() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x PREFIXED P;
    DCL x INT INIT := 1;
  END A;
  B: MODULE
    SEIZE x PREFIXED P;
    ASSERT x = 1;
  END B;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_grant_to_friends_works_at_runtime() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x TO B;
    DCL x INT INIT := 1;
  END A;
  B: MODULE
    SEIZE x;
    ASSERT x = 1;
  END B;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_grants_are_shared_with_siblings() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;

  B: MODULE
    SEIZE x;
    x := 2;
  END B;

  C: MODULE
    SEIZE x;
    ASSERT x = 2;
  END C;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_module_locals_are_scoped() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 5;
  Helper: MODULE
    DCL y INT INIT := 2;
    ASSERT y = 2;
  END Helper;
  ASSERT x = 5;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn context_visibility_has_no_runtime_effect() {
        run_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 4;
END A;

CONTEXT
  DCL x INT;
FOR
CONTEXT
  SEIZE x;
FOR
B: MODULE
  SEIZE x;
  ASSERT x = 4;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn contexts_have_no_runtime_effect() {
        run_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 4;
END A;

CONTEXT
  DCL x INT;
FOR
B: MODULE
  SEIZE x;
  ASSERT x = 4;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn compares_structure_values_recursively() {
        run_source(
            r#"
M: MODULE
  NEWMODE Inner = STRUCT(v INT);
  NEWMODE Outer = STRUCT(inner Inner, tag BOOL);
  DCL a Outer, b Outer;
  a.inner.v := 1;
  a.tag := TRUE;
  b := a;
  ASSERT a = b;
  ASSERT NOT (a /= b);
  b.inner.v := 2;
  ASSERT a /= b;
  ASSERT NOT (a = b);
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn comparing_undefined_structure_field_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  DCL a S, b S;
  ASSERT a = b;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0504");
        assert!(error.message.contains("undefined value"));
    }

    #[test]
    fn seizes_with_prefixed_clause() {
        run_source(
            r#"
A: MODULE
  GRANT a PREFIXED P;
  DCL a INT INIT := 1;
END A;

B: MODULE
  SEIZE a PREFIXED P;
  ASSERT a = 1;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_with_old_prefix_rename() {
        run_source(
            r#"
A: MODULE
  GRANT a PREFIXED P;
  DCL a INT INIT := 5;
END A;

B: MODULE
  SEIZE ( P -> ) ! a;
  ASSERT a = 5;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_all_with_prefixed_clause() {
        run_source(
            r#"
A: MODULE
  GRANT a, b PREFIXED P;
  DCL a INT INIT := 1;
  DCL b INT INIT := 2;
END A;

B: MODULE
  SEIZE ALL PREFIXED P;
  ASSERT a + b = 3;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_qualified_exports_and_all() {
        run_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 1;
END A;

B: MODULE
  GRANT y;
  DCL y INT INIT := 2;
END B;

C: MODULE
  SEIZE A!x;
  ASSERT x = 1;
END C;

D: MODULE
  SEIZE ALL;
  ASSERT x = 1;
  ASSERT y = 2;
END D;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_spec_and_body() {
        run_source(
            r#"
Lib: SPEC MODULE
  GRANT x, F;
  DCL x INT;
  F: PROC () RETURNS (INT) END;
END Lib;

Lib: MODULE
  DCL x INT INIT := 41;
  F: PROC () RETURNS (INT);
    RETURN (x + 1);
  END F;
END Lib;

Client: MODULE
  SEIZE x, F;
  ASSERT x = 41;
  ASSERT F() = 42;
END Client;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_do_for_ascending_and_descending() {
        run_source(
            r#"
M: MODULE
  DCL sum INT INIT := 0;
  DCL count INT INIT := 0;
  DO FOR i := 1 TO 4;
    sum := sum + i;
  OD;
  ASSERT sum = 10;

  DO FOR i := 4 BY 1 DOWN TO 1;
    count := count + 1;
  OD;
  ASSERT count = 4;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_case_actions() {
        run_source(
            r#"
M: MODULE
  DCL grade INT INIT := 2;
  DCL score INT INIT := 0;

  CASE grade OF
    (1): score := 10;
    (2, 3): score := 20;
    (4:6): score := 30;
    (ELSE): score := 99;
  ESAC;
  ASSERT score = 20;

  grade := 5;
  CASE grade OF
    (1): score := 10;
    (4:6): score := 30;
  ESAC;
  ASSERT score = 30;

  grade := 7;
  CASE grade OF
    (1): score := 10;
    ELSE
      score := 0;
  ESAC;
  ASSERT score = 0;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn labelled_exit_leaves_outer_loop() {
        run_source(
            r#"
M: MODULE
  DCL count INT INIT := 0;
Outer: DO FOR i := 1 TO 3;
    DO FOR j := 1 TO 3;
      count := count + 1;
      IF count = 4 THEN
        EXIT Outer;
      FI;
    OD;
  OD;
  ASSERT count = 4;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn bare_exit_leaves_begin_not_loop() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 0;
  DO FOR i := 1 TO 3;
    BEGIN
      x := x + 1;
      EXIT;
    END;
    x := x + 10;
  OD;
  ASSERT x = 33;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn writetext_and_readtext_on_chars_locations() {
        run_source(
            r#"
M: MODULE
  DCL s CHARS INIT := "hello";
  DCL t CHARS INIT := "";
  DCL u CHARS INIT := "abc";
  WRITETEXT(s, 1, "i");
  ASSERT s = "hillo";
  READTEXT(s, 1, "i");
  WRITETEXT(t, 2, "x");
  ASSERT t = "  x";
  WRITETEXT(u, "xy");
  ASSERT u = "xyc";
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn readtext_mismatch_raises_textfail() {
        run_source(
            r#"
M: MODULE
  DCL s CHARS INIT := "abc";
  READTEXT(s, 1, "z");
  ASSERT FALSE;
END
ON (TEXTFAIL):
  ASSERT TRUE;
END;
"#,
        )
        .unwrap();
    }

    #[test]
    fn module_handler_catches_cause() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 0;
  CAUSE boom;
  x := 1;
END
ON (boom):
  x := 2;
  ASSERT x = 2;
END;
"#,
        )
        .unwrap();
    }

    #[test]
    fn procedure_handler_can_deliver_a_result() {
        run_source(
            r#"
M: MODULE
  P: PROC () RETURNS (INT);
    CAUSE fail;
    RETURN 1;
  END
  ON (fail):
    RESULT 7;
  END P;
  ASSERT P() = 7;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn handler_catches_overflow() {
        run_source(
            r#"
M: MODULE
  DCL a SHORT_INT INIT := 32767;
  a +:= 1;
  ASSERT FALSE;
END
ON (OVERFLOW):
  ASSERT TRUE;
END;
"#,
        )
        .unwrap();
    }

    #[test]
    fn action_handler_runs_in_the_begin_reach() {
        run_source(
            r#"
M: MODULE
  BEGIN
    DCL outer INT INIT := 1;
    BEGIN
      DCL inner INT INIT := 2;
      CAUSE fail;
    END
    ON (fail):
      ASSERT outer + inner = 3;
    END;
  END;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn action_handler_only_handles_matching_exceptions() {
        let error = run_source(
            r#"
M: MODULE
  BEGIN
    CAUSE unexpected;
  END
  ON (handled):
    ASSERT TRUE;
  END;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0509");
        assert!(error.message.contains("unexpected"));
    }

    #[test]
    fn unhandled_exception_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  CAUSE boom;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0509");
        assert!(error.message.contains("unhandled exception `boom`"));
    }

    #[test]
    fn nested_exception_propagates_to_the_enclosing_module_handler() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 0;
  A: MODULE
    CAUSE boom;
  END A;
  x := 1;
END
ON (boom):
  x := 3;
  ASSERT x = 3;
END;
"#,
        )
        .unwrap();
    }

    #[test]
    fn arrays_support_indexing_bounds_and_scalars() {
        run_source(
            r#"
M: MODULE
  DCL a ARRAY (1:3) INT;
  DCL s ARRAY (0:1) CHARS;
  DCL n ARRAY (-1:1) INT;
  a(1) := 10;
  a(2) := 20;
  a(3) := a(1) + a(2);
  ASSERT a(3) = 30;
  s(0) := "x";
  ASSERT s(0) = "x";
  n(-1) := 5;
  ASSERT n(-1) = 5;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn array_index_out_of_bounds_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  DCL a ARRAY (1:2) INT;
  a(0) := 1;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("out of bounds"), "{}", error.message);
    }

    #[test]
    fn char_and_chars_modes_work() {
        run_source(
            r#"
M: MODULE
  DCL c CHAR INIT := 'A';
  DCL s CHARS INIT := "hi";
  s := s // "!";
  ASSERT s = "hi!";
  ASSERT c = 'A';
  c := 'B';
  ASSERT c = 'B';
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn structure_fields_can_be_assigned_and_read() {
        run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT, y BOOL);
  DCL s S;
  s.x := 2;
  s.y := TRUE;
  ASSERT s.x = 2;
  ASSERT s.y;
  s.x := s.x + 3;
  ASSERT s.x = 5;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_structure_fields_and_whole_value_copy() {
        run_source(
            r#"
M: MODULE
  NEWMODE Inner = STRUCT(v INT);
  NEWMODE Outer = STRUCT(inner Inner, tag BOOL);
  DCL a Outer, b Outer;
  a.inner.v := 7;
  a.tag := TRUE;
  b := a;
  b.inner.v := 9;
  ASSERT a.inner.v = 7;
  ASSERT b.inner.v = 9;
  ASSERT b.tag;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seized_structure_mode_is_usable_at_runtime() {
        run_source(
            r#"
A: MODULE
  NEWMODE S = STRUCT(x INT);
  GRANT S;
END A;

B: MODULE
  SEIZE S;
  DCL s S;
  s.x := 4;
  ASSERT s.x = 4;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn procedures_can_return_structures() {
        run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);

  Make: PROC () RETURNS (S);
    DCL tmp S;
    tmp.x := 11;
    RETURN tmp;
  END Make;

  DCL s S INIT := Make();
  ASSERT s.x = 11;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn structure_field_compound_assignment() {
        run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  DCL s S;
  s.x := 1;
  s.x +:= 2;
  ASSERT s.x = 3;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn undefined_structure_field_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  DCL s S;
  ASSERT s.x = 1;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0504");
        assert!(error.message.contains("undefined value `x`"));
    }
}
