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
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Int(IntKind),
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
    Bool,
    Char,
    String,
    Struct(ModeId),
    PowerSet(types::PowerSetMember),
}

impl ScalarKind {
    fn c_type(self) -> String {
        match self {
            ScalarKind::Int(_) => "int64_t".to_owned(),
            ScalarKind::Bool => "int".to_owned(),
            ScalarKind::Char => "int".to_owned(),
            ScalarKind::String => "const char *".to_owned(),
            ScalarKind::Struct(id) => format!("z_struct_{}", id.0),
            ScalarKind::PowerSet(_) => "ZPowerSet".to_owned(),
        }
    }

    fn kind(self) -> Kind {
        match self {
            ScalarKind::Int(kind) => Kind::Int(kind),
            ScalarKind::Bool => Kind::Bool,
            ScalarKind::Char => Kind::Char,
            ScalarKind::String => Kind::String,
            ScalarKind::Struct(id) => Kind::Struct(id),
            ScalarKind::PowerSet(member) => Kind::PowerSet(member),
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
            Kind::Bool => "int".to_owned(),
            Kind::Char => "int".to_owned(),
            Kind::String => "const char *".to_owned(),
            Kind::Array { .. } => {
                unreachable!("array kinds are emitted through c_declaration")
            }
            Kind::Ref(element) => match element {
                ScalarKind::Int(_) => "int64_t *".to_owned(),
                ScalarKind::Bool => "int *".to_owned(),
                ScalarKind::Char => "int *".to_owned(),
                ScalarKind::String => "const char **".to_owned(),
                ScalarKind::Struct(id) => format!("z_struct_{} *", id.0),
                ScalarKind::PowerSet(_) => "ZPowerSet *".to_owned(),
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
    labels: Vec<(String, String)>,
    prelude: Vec<String>,
    temporaries: Vec<(String, Kind)>,
    scopes: Vec<Vec<(String, Kind)>>,
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

    let mut out = String::new();
    out.push_str("/* generated by z200c */\n");
    out.push_str(
        r#"/* generated by z200c */
#include <assert.h>
#include <stdbool.h>
#include <setjmp.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct ZRcEntry {
    void *pointer;
    size_t references;
    struct ZRcEntry *next;
} ZRcEntry;

static ZRcEntry *z_rc_entries = NULL;
static size_t z_rc_live = 0;

static void z_rc_register(void *pointer) {
    if (pointer == NULL) { return; }
    ZRcEntry *entry = (ZRcEntry *)malloc(sizeof(ZRcEntry));
    if (entry == NULL) { abort(); }
    entry->pointer = pointer;
    entry->references = 1;
    entry->next = z_rc_entries;
    z_rc_entries = entry;
    z_rc_live++;
}

static void *z_rc_alloc(size_t size) {
    void *pointer = malloc(size);
    if (pointer == NULL) { abort(); }
    z_rc_register(pointer);
    return pointer;
}

static void *z_rc_realloc(void *pointer, size_t size) {
    void *resized = realloc(pointer, size);
    if (resized == NULL) { abort(); }
    if (resized != pointer) {
        for (ZRcEntry *entry = z_rc_entries; entry != NULL; entry = entry->next) {
            if (entry->pointer == pointer) {
                entry->pointer = resized;
                break;
            }
        }
    }
    return resized;
}

static void z_rc_retain(void *pointer) {
    if (pointer == NULL) { return; }
    for (ZRcEntry *entry = z_rc_entries; entry != NULL; entry = entry->next) {
        if (entry->pointer == pointer) {
            entry->references++;
            return;
        }
    }
}

static void z_rc_release(void *pointer) {
    if (pointer == NULL) { return; }
    ZRcEntry **link = &z_rc_entries;
    while (*link != NULL) {
        ZRcEntry *entry = *link;
        if (entry->pointer == pointer) {
            if (--entry->references == 0) {
                *link = entry->next;
                free(entry);
                free(pointer);
                z_rc_live--;
            }
            return;
        }
        link = &entry->next;
    }
}

static void z_string_assign(const char **target, const char *value) {
    if (*target == value) { return; }
    z_rc_retain((void *)value);
    z_rc_release((void *)*target);
    *target = value;
}

static void z_string_drop(const char *value) {
    z_rc_release((void *)value);
}

static const char *z_concat(const char *a, const char *b) {
    size_t la = strlen(a);
    size_t lb = strlen(b);
    char *out = (char *)z_rc_alloc(la + lb + 1);
    memcpy(out, a, la);
    memcpy(out + la, b, lb);
    out[la + lb] = '\0';
    return out;
}

static const char *z_readline(void) {
    size_t cap = 256;
    size_t len = 0;
    char *buffer = (char *)z_rc_alloc(cap);
    int ch;
    while ((ch = getchar()) != EOF && ch != '\n') {
        if (len + 1 >= cap) {
            cap *= 2;
            buffer = (char *)z_rc_realloc(buffer, cap);
        }
        buffer[len++] = (char)ch;
    }
    if (len > 0 && buffer[len - 1] == '\r') { len--; }
    buffer[len] = '\0';
    return buffer;
}

static const char *z_exception_name = "";

typedef struct ZHandler {
    jmp_buf env;
    struct ZHandler *prev;
} ZHandler;

static ZHandler *z_handlers = NULL;

static void z_throw(const char *name) {
    z_exception_name = name;
    if (z_handlers != NULL) { longjmp(z_handlers->env, 1); }
    fprintf(stderr, "UNHANDLED: %s\n", name);
    exit(1);
}

static void z_fail(const char *code, const char *message) {
    if (z_handlers != NULL) {
        z_exception_name = code;
        longjmp(z_handlers->env, 1);
    }
    fprintf(stderr, "%s: %s\n", code, message);
    exit(1);
}

static int64_t z_check_index(int64_t index, int64_t low, int64_t high) {
    if (index < low || index > high) { z_fail("RANGEFAIL", "array index out of bounds"); }
    return index;
}

static int64_t z_div(int64_t a, int64_t b) {
    if (b == 0) { z_fail("OVERFLOW", "division by zero"); }
    if (a == INT64_MIN && b == -1) { z_fail("OVERFLOW", "integer overflow"); }
    return a / b;
}

static int64_t z_mod(int64_t a, int64_t b) {
    if (b == 0) { z_fail("OVERFLOW", "division by zero"); }
    if (a == INT64_MIN && b == -1) { z_fail("OVERFLOW", "integer overflow"); }
    return a % b;
}

static int64_t z_add(int64_t a, int64_t b) {
    int64_t result;
    if (__builtin_add_overflow(a, b, &result)) { z_fail("OVERFLOW", "integer overflow"); }
    return result;
}

static int64_t z_sub(int64_t a, int64_t b) {
    int64_t result;
    if (__builtin_sub_overflow(a, b, &result)) { z_fail("OVERFLOW", "integer overflow"); }
    return result;
}

static int64_t z_mul(int64_t a, int64_t b) {
    int64_t result;
    if (__builtin_mul_overflow(a, b, &result)) { z_fail("OVERFLOW", "integer overflow"); }
    return result;
}

static int64_t z_neg(int64_t a) {
    if (a == INT64_MIN) { z_fail("OVERFLOW", "integer overflow"); }
    return -a;
}

static int64_t z_check_int(int64_t value, int64_t low, int64_t high) {
    if (value < low || value > high) {
        z_fail("OVERFLOW", "integer value does not fit its mode");
    }
    return value;
}

static void *z_check_ref(const void *pointer) {
    if (pointer == NULL) { z_fail("EMPTY", "NULL dereference"); }
    return (void *)pointer;
}

#define Z_DEREF(type, pointer) (*((type *)z_check_ref((const void *)(pointer))))

typedef struct {
    int64_t *items;
    size_t len;
} ZPowerSet;

typedef struct {
    int64_t low;
    int64_t high;
} ZPSRange;

static ZPowerSet z_ps_empty(void) {
    ZPowerSet set = { NULL, 0 };
    return set;
}

static void z_ps_assign(ZPowerSet *target, ZPowerSet value) {
    if (target->items == value.items) { return; }
    z_rc_retain(value.items);
    z_rc_release(target->items);
    *target = value;
}

static void z_ps_drop(ZPowerSet *value) {
    z_rc_release(value->items);
    value->items = NULL;
    value->len = 0;
}

static int z_ps_int_compare(const void *left, const void *right) {
    int64_t a = *(const int64_t *)left;
    int64_t b = *(const int64_t *)right;
    return (a > b) - (a < b);
}

static ZPowerSet z_ps_literal(size_t count, const ZPSRange *ranges) {
    if (count == 0) { return z_ps_empty(); }
    size_t cap = 16;
    size_t len = 0;
    int64_t *items = (int64_t *)z_rc_alloc(cap * sizeof(int64_t));
    for (size_t index = 0; index < count; index++) {
        int64_t low = ranges[index].low;
        int64_t high = ranges[index].high;
        if (low > high) { continue; }
        for (int64_t value = low;; value++) {
            if (len == cap) {
                cap *= 2;
                items = (int64_t *)z_rc_realloc(items, cap * sizeof(int64_t));
            }
            items[len++] = value;
            if (value == high) { break; }
        }
    }
    qsort(items, len, sizeof(int64_t), z_ps_int_compare);
    size_t unique = 0;
    for (size_t index = 0; index < len; index++) {
        if (unique == 0 || items[unique - 1] != items[index]) {
            items[unique++] = items[index];
        }
    }
    ZPowerSet set = { items, unique };
    return set;
}

static int z_ps_equal(ZPowerSet left, ZPowerSet right) {
    return left.len == right.len
        && (left.len == 0 || memcmp(left.items, right.items, left.len * sizeof(int64_t)) == 0);
}

static int z_ps_contains(ZPowerSet set, int64_t value) {
    size_t low = 0;
    size_t high = set.len;
    while (low < high) {
        size_t middle = low + (high - low) / 2;
        if (set.items[middle] == value) { return 1; }
        if (set.items[middle] < value) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    return 0;
}

static int z_ps_subset(ZPowerSet left, ZPowerSet right) {
    size_t i = 0;
    size_t j = 0;
    while (i < left.len && j < right.len) {
        if (left.items[i] == right.items[j]) {
            i++;
            j++;
        } else if (left.items[i] > right.items[j]) {
            j++;
        } else {
            return 0;
        }
    }
    return i == left.len;
}

static void z_ps_require_ordered(ZPowerSet left, ZPowerSet right) {
    if (!z_ps_subset(left, right) && !z_ps_subset(right, left)) {
        z_fail("E0500", "powersets are not ordered");
    }
}

static int z_ps_le(ZPowerSet left, ZPowerSet right) {
    z_ps_require_ordered(left, right);
    return z_ps_subset(left, right);
}

static int z_ps_ge(ZPowerSet left, ZPowerSet right) {
    z_ps_require_ordered(left, right);
    return z_ps_subset(right, left);
}

static int z_ps_lt(ZPowerSet left, ZPowerSet right) {
    z_ps_require_ordered(left, right);
    return z_ps_subset(left, right) && !z_ps_equal(left, right);
}

static int z_ps_gt(ZPowerSet left, ZPowerSet right) {
    z_ps_require_ordered(left, right);
    return z_ps_subset(right, left) && !z_ps_equal(left, right);
}

static ZPowerSet z_ps_union(ZPowerSet left, ZPowerSet right) {
    size_t cap = left.len + right.len;
    if (cap == 0) { return z_ps_empty(); }
    int64_t *items = (int64_t *)z_rc_alloc(cap * sizeof(int64_t));
    size_t i = 0;
    size_t j = 0;
    size_t len = 0;
    while (i < left.len || j < right.len) {
        if (j == right.len || (i < left.len && left.items[i] < right.items[j])) {
            items[len++] = left.items[i++];
        } else if (i == left.len || right.items[j] < left.items[i]) {
            items[len++] = right.items[j++];
        } else {
            items[len++] = left.items[i++];
            j++;
        }
    }
    ZPowerSet set = { items, len };
    return set;
}

static ZPowerSet z_ps_intersection(ZPowerSet left, ZPowerSet right) {
    size_t cap = left.len < right.len ? left.len : right.len;
    if (cap == 0) { return z_ps_empty(); }
    int64_t *items = (int64_t *)z_rc_alloc(cap * sizeof(int64_t));
    size_t i = 0;
    size_t j = 0;
    size_t len = 0;
    while (i < left.len && j < right.len) {
        if (left.items[i] == right.items[j]) {
            items[len++] = left.items[i++];
            j++;
        } else if (left.items[i] < right.items[j]) {
            i++;
        } else {
            j++;
        }
    }
    ZPowerSet set = { items, len };
    return set;
}

static ZPowerSet z_ps_difference(ZPowerSet left, ZPowerSet right) {
    if (left.len == 0) { return z_ps_empty(); }
    int64_t *items = (int64_t *)z_rc_alloc(left.len * sizeof(int64_t));
    size_t i = 0;
    size_t j = 0;
    size_t len = 0;
    while (i < left.len) {
        if (j == right.len || left.items[i] < right.items[j]) {
            items[len++] = left.items[i++];
        } else if (left.items[i] == right.items[j]) {
            i++;
            j++;
        } else {
            j++;
        }
    }
    ZPowerSet set = { items, len };
    return set;
}

static int z_ps_array_equal(ZPowerSet *left, ZPowerSet *right, size_t count) {
    for (size_t index = 0; index < count; index++) {
        if (!z_ps_equal(left[index], right[index])) { return 0; }
    }
    return 1;
}

static int z_string_array_equal(const char **left, const char **right, size_t count) {
    for (size_t index = 0; index < count; index++) {
        if (strcmp(left[index], right[index]) != 0) { return 0; }
    }
    return 1;
}

typedef struct {
    void *data;
    int64_t len;
    int64_t low;
} ZSlice;

static ZSlice z_slice_make(
    void *data,
    int64_t low,
    int64_t high,
    int64_t first,
    int64_t last,
    int up,
    size_t element_size
) {
    int64_t upper;
    if (up) {
        if (last < 1) { z_fail("RANGEFAIL", "array slice size must be positive"); }
        if (first > INT64_MAX - (last - 1)) {
            z_fail("RANGEFAIL", "array slice bounds overflow");
        }
        upper = first + last - 1;
    } else {
        upper = last;
    }
    if (first > upper || first < low || upper > high) {
        z_fail("RANGEFAIL", "array slice is outside the array bounds");
    }
    ZSlice slice;
    slice.data = (char *)data + (size_t)(first - low) * element_size;
    slice.len = upper - first + 1;
    slice.low = low;
    return slice;
}

static void *z_slice_check_len(ZSlice slice, int64_t expected) {
    if (slice.len != expected) {
        z_fail("RANGEFAIL", "array slice length mismatch");
    }
    return slice.data;
}

static int z_slice_equal_bytes(
    const void *left,
    int64_t left_len,
    const void *right,
    int64_t right_len,
    size_t element_size
) {
    return left_len == right_len
        && (left_len == 0
            || memcmp(left, right, (size_t)left_len * element_size) == 0);
}

static int z_slice_equal_strings(
    const char **left,
    int64_t left_len,
    const char **right,
    int64_t right_len
) {
    if (left_len != right_len) { return 0; }
    for (int64_t index = 0; index < left_len; index++) {
        if (strcmp(left[index], right[index]) != 0) { return 0; }
    }
    return 1;
}

static int z_slice_equal_powersets(
    ZPowerSet *left,
    int64_t left_len,
    ZPowerSet *right,
    int64_t right_len
) {
    if (left_len != right_len) { return 0; }
    for (int64_t index = 0; index < left_len; index++) {
        if (!z_ps_equal(left[index], right[index])) { return 0; }
    }
    return 1;
}

typedef struct {
    const char *path;
    int associated;
    int existing;
    int indexable;
} ZAssociation;

typedef struct {
    const char *path;
    int connected;
    int usage;
    int indexed;
    int64_t position;
    int out_of_file;
} ZAccess;

static void z_association_assign(ZAssociation *target, ZAssociation value) {
    z_string_assign(&target->path, value.path);
    target->associated = value.associated;
    target->existing = value.existing;
    target->indexable = value.indexable;
}

static void z_association_drop(ZAssociation *value) {
    z_string_drop(value->path);
    value->path = NULL;
    value->associated = 0;
    value->existing = 0;
    value->indexable = 0;
}

static void z_access_assign(ZAccess *target, ZAccess value) {
    z_string_assign(&target->path, value.path);
    target->connected = value.connected;
    target->usage = value.usage;
    target->indexed = value.indexed;
    target->position = value.position;
    target->out_of_file = value.out_of_file;
}

static void z_access_drop(ZAccess *value) {
    z_string_drop(value->path);
    value->path = NULL;
    value->connected = 0;
    value->usage = 0;
    value->indexed = 0;
    value->position = 0;
    value->out_of_file = 0;
}

static int z_file_exists(const char *path) {
    FILE *file = fopen(path, "rb");
    if (file == NULL) { return 0; }
    fclose(file);
    return 1;
}

static char *z_file_contents(const char *path, size_t *length) {
    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        z_fail("READFAIL", "cannot open file");
    }
    if (fseek(file, 0, SEEK_END) != 0) { fclose(file); z_fail("READFAIL", "cannot seek file"); }
    long size = ftell(file);
    if (size < 0) { fclose(file); z_fail("READFAIL", "cannot size file"); }
    rewind(file);
    char *contents = (char *)malloc((size_t)size + 1);
    if (contents == NULL) { fclose(file); z_fail("STORAGE", "out of memory"); }
    size_t read = fread(contents, 1, (size_t)size, file);
    fclose(file);
    contents[read] = '\0';
    *length = read;
    return contents;
}

typedef struct {
    char **items;
    size_t count;
    size_t cap;
} ZLines;

static void z_lines_push(ZLines *lines, const char *start, size_t length) {
    if (length > 0 && start[length - 1] == '\r') { length--; }
    char *line = (char *)malloc(length + 1);
    if (line == NULL) { z_fail("STORAGE", "out of memory"); }
    memcpy(line, start, length);
    line[length] = '\0';
    if (lines->count == lines->cap) {
        lines->cap = lines->cap == 0 ? 8 : lines->cap * 2;
        lines->items = (char **)realloc(lines->items, lines->cap * sizeof(char *));
        if (lines->items == NULL) { z_fail("STORAGE", "out of memory"); }
    }
    lines->items[lines->count++] = line;
}

static ZLines z_lines_load(const char *path, int missing_ok) {
    ZLines lines = { NULL, 0, 0 };
    if (!z_file_exists(path)) {
        if (missing_ok) { return lines; }
        z_fail("READFAIL", "cannot open file");
    }
    size_t length = 0;
    char *contents = z_file_contents(path, &length);
    size_t start = 0;
    for (size_t index = 0; index < length; index++) {
        if (contents[index] == '\n') {
            z_lines_push(&lines, contents + start, index - start);
            start = index + 1;
        }
    }
    if (start < length) {
        z_lines_push(&lines, contents + start, length - start);
    }
    free(contents);
    return lines;
}

static void z_lines_free(ZLines *lines) {
    for (size_t index = 0; index < lines->count; index++) {
        free(lines->items[index]);
    }
    free(lines->items);
    lines->items = NULL;
    lines->count = 0;
    lines->cap = 0;
}

static size_t z_file_line_count(const char *path) {
    ZLines lines = z_lines_load(path, 0);
    size_t count = lines.count;
    z_lines_free(&lines);
    return count;
}

static const char *z_file_read_line(const char *path, int64_t position) {
    if (position < 0) {
        z_fail("RANGEFAIL", "negative record index");
    }
    ZLines lines = z_lines_load(path, 0);
    if ((size_t)position >= lines.count) {
        z_lines_free(&lines);
        return NULL;
    }
    size_t length = strlen(lines.items[position]);
    char *record = (char *)z_rc_alloc(length + 1);
    memcpy(record, lines.items[position], length + 1);
    z_lines_free(&lines);
    return record;
}

static void z_file_write_line(const char *path, int64_t position, const char *record) {
    if (position < 0) {
        z_fail("RANGEFAIL", "negative record index");
    }
    ZLines lines = z_lines_load(path, 1);
    while (lines.count <= (size_t)position) {
        z_lines_push(&lines, "", 0);
    }
    free(lines.items[position]);
    size_t length = strlen(record);
    lines.items[position] = (char *)malloc(length + 1);
    if (lines.items[position] == NULL) { z_fail("STORAGE", "out of memory"); }
    memcpy(lines.items[position], record, length + 1);
    FILE *file = fopen(path, "wb");
    if (file == NULL) { z_lines_free(&lines); z_fail("WRITEFAIL", "cannot open file"); }
    for (size_t index = 0; index < lines.count; index++) {
        fputs(lines.items[index], file);
        fputc('\n', file);
    }
    fclose(file);
    z_lines_free(&lines);
}

static void z_associate(ZAssociation *association, const char *path) {
    if (association->associated) {
        z_fail("ASSOCIATEFAIL", "association already exists");
    }
    z_string_assign(&association->path, path);
    association->associated = 1;
    association->existing = z_file_exists(path);
    association->indexable = 0;
}

static void z_create_association(ZAssociation *association) {
    if (!association->associated) {
        z_fail("NOTASSOCIATED", "empty association");
    }
    if (association->existing) {
        z_fail("CREATEFAIL", "file already exists");
    }
    FILE *file = fopen(association->path, "wb");
    if (file == NULL) { z_fail("CREATEFAIL", "cannot create file"); }
    fclose(file);
    association->existing = 1;
}

static void z_delete_association(ZAssociation *association) {
    if (!association->associated) {
        z_fail("NOTASSOCIATED", "empty association");
    }
    if (!association->existing) {
        z_fail("DELETEFAIL", "file does not exist");
    }
    if (remove(association->path) != 0) {
        z_fail("DELETEFAIL", "cannot delete file");
    }
    association->existing = 0;
}

static void z_dissociate(ZAssociation *association) {
    if (!association->associated) {
        z_fail("NOTASSOCIATED", "empty association");
    }
    z_string_drop(association->path);
    association->path = NULL;
    association->associated = 0;
    association->existing = 0;
    association->indexable = 0;
}

static int z_association_is_associated(const ZAssociation *association) {
    return association->associated;
}

static int64_t z_access_usage(const ZAccess *access) {
    if (!access->connected) {
        z_fail("NOTCONNECTED", "access location is not connected");
    }
    return access->usage;
}

static int z_access_out_of_file(const ZAccess *access) {
    if (!access->connected) {
        z_fail("NOTCONNECTED", "access location is not connected");
    }
    return access->out_of_file;
}

static int z_association_attribute(const ZAssociation *association, int attribute) {
    if (!association->associated) {
        z_fail("NOTASSOCIATED", "empty association");
    }
    switch (attribute) {
        case 0: return association->existing;
        case 1: return association->existing;
        case 2: return association->existing;
        case 3: return association->indexable;
        case 4: return 1;
        case 5: return 0;
        default: return 0;
    }
}

static void z_connect_access(
    ZAccess *access,
    ZAssociation *association,
    int usage,
    int where,
    int indexed,
    int has_index,
    int64_t index,
    int64_t low,
    int64_t high
) {
    if (!association->associated) {
        z_fail("NOTASSOCIATED", "empty association");
    }
    if (!association->existing) {
        z_fail("CONNECTFAIL", "association has no existing file");
    }
    size_t count = z_file_line_count(association->path);
    int64_t base;
    if (where == 0) {
        base = 0;
    } else if (where == 1) {
        if (!access->connected) {
            z_fail("CONNECTFAIL", "SAME requires an existing connection");
        }
        base = access->position;
    } else if (where == 2) {
        base = (int64_t)count;
    } else {
        z_fail("CONNECTFAIL", "invalid WHERE value");
        base = 0;
    }
    int64_t position = base;
    if (has_index) {
        if (indexed && (index < low || index > high)) {
            z_fail("RANGEFAIL", "record index is out of range");
        }
        position = base + index - low;
    }
    z_string_assign(&access->path, association->path);
    access->connected = 1;
    access->usage = usage;
    access->indexed = indexed;
    access->position = position;
    access->out_of_file = 0;
    association->indexable = indexed;
}

static void z_disconnect_access(ZAccess *access) {
    if (!access->connected) {
        z_fail("NOTCONNECTED", "access location is not connected");
    }
    z_string_drop(access->path);
    access->path = NULL;
    access->connected = 0;
    access->out_of_file = 0;
}

static const char *z_read_record(
    ZAccess *access,
    int has_index,
    int64_t index,
    int64_t low,
    int64_t high
) {
    if (!access->connected) {
        z_fail("NOTCONNECTED", "access location is not connected");
    }
    if (access->usage == 1) {
        z_fail("READFAIL", "access location is write-only");
    }
    if (!access->indexed && access->out_of_file) {
        z_fail("READFAIL", "access location is at end of file");
    }
    int64_t position = has_index ? index : access->position;
    if (access->indexed && (position < low || position > high)) {
        z_fail("RANGEFAIL", "record index is out of range");
    }
    if (position < 0) {
        z_fail("RANGEFAIL", "negative record index");
    }
    const char *record = z_file_read_line(access->path, position);
    if (record == NULL) {
        access->out_of_file = 1;
        char *empty = (char *)z_rc_alloc(1);
        empty[0] = '\0';
        return empty;
    }
    access->position = position + 1;
    access->out_of_file = 0;
    return record;
}

static void z_write_record(
    ZAccess *access,
    int has_index,
    int64_t index,
    const char *record,
    int64_t low,
    int64_t high
) {
    if (!access->connected) {
        z_fail("NOTCONNECTED", "access location is not connected");
    }
    if (access->usage == 0) {
        z_fail("WRITEFAIL", "access location is read-only");
    }
    int64_t position = has_index ? index : access->position;
    if (access->indexed || has_index) {
        if (position < low || position > high) {
            z_fail("RANGEFAIL", "record index is out of range");
        }
        if (position < 0) {
            z_fail("RANGEFAIL", "negative record index");
        }
        z_file_write_line(access->path, position, record);
        access->position = position + 1;
    } else {
        ZLines lines = z_lines_load(access->path, 1);
        size_t length = strlen(record);
        char *copy = (char *)malloc(length + 1);
        if (copy == NULL) { z_fail("STORAGE", "out of memory"); }
        memcpy(copy, record, length + 1);
        z_lines_push(&lines, copy, length);
        free(copy);
        FILE *file = fopen(access->path, "ab");
        if (file == NULL) { z_lines_free(&lines); z_fail("WRITEFAIL", "cannot open file"); }
        fputs(record, file);
        fputc('\n', file);
        fclose(file);
        z_lines_free(&lines);
        access->position++;
    }
}

typedef struct {
    char *record;
    size_t len;
    int64_t index;
    const char *path;
    int connected;
    int usage;
    int indexed;
    int64_t position;
    int out_of_file;
} ZText;

static const char *z_text_view(const ZText *text) {
    return text->record == NULL ? "" : text->record;
}

static ZText z_text_from_string(const char *value) {
    size_t len = strlen(value);
    char *copy = (char *)z_rc_alloc(len + 1);
    memcpy(copy, value, len + 1);
    ZText text = { copy, len, 0, NULL, 0, 0, 0, 0, 0 };
    return text;
}

static void z_text_assign(ZText *text, const char *value, size_t limit);

static int64_t z_text_usage(const ZText *text) {
    if (!text->connected) {
        z_fail("NOTCONNECTED", "text location is not connected");
    }
    return text->usage;
}

static int z_text_out_of_file(const ZText *text) {
    if (!text->connected) {
        z_fail("NOTCONNECTED", "text location is not connected");
    }
    return text->out_of_file;
}

static void z_connect_text(
    ZText *text,
    ZAssociation *association,
    int usage,
    int where,
    int indexed,
    int has_index,
    int64_t index,
    int64_t low,
    int64_t high
) {
    if (!association->associated) {
        z_fail("NOTASSOCIATED", "empty association");
    }
    if (!association->existing) {
        z_fail("CONNECTFAIL", "association has no existing file");
    }
    size_t count = z_file_line_count(association->path);
    int64_t base;
    if (where == 0) {
        base = 0;
    } else if (where == 1) {
        if (!text->connected) {
            z_fail("CONNECTFAIL", "SAME requires an existing connection");
        }
        base = text->position;
    } else if (where == 2) {
        base = (int64_t)count;
    } else {
        z_fail("CONNECTFAIL", "invalid WHERE value");
        base = 0;
    }
    int64_t position = base;
    if (has_index) {
        if (indexed && (index < low || index > high)) {
            z_fail("RANGEFAIL", "record index is out of range");
        }
        position = base + index - low;
    }
    z_string_assign(&text->path, association->path);
    text->connected = 1;
    text->usage = usage;
    text->indexed = indexed;
    text->position = position;
    text->out_of_file = 0;
    association->indexable = indexed;
}

static void z_disconnect_text(ZText *text) {
    if (!text->connected) {
        z_fail("NOTCONNECTED", "text location is not connected");
    }
    z_string_drop(text->path);
    text->path = NULL;
    text->connected = 0;
    text->out_of_file = 0;
}

static int z_text_begin_read(
    ZText *text,
    int has_index,
    int64_t index,
    int64_t low,
    int64_t high,
    size_t limit
) {
    if (!text->connected) {
        if (has_index) {
            if (index < 0 || (size_t)index > limit) {
                z_fail("TEXTFAIL", "text index is outside the text length");
            }
            text->index = index;
        }
        return -1;
    }
    if (text->usage == 1) {
        z_fail("READFAIL", "access location is write-only");
    }
    if (!text->indexed && text->out_of_file) {
        return 0;
    }
    int64_t position = has_index ? index : text->position;
    if (text->indexed && (position < low || position > high)) {
        z_fail("RANGEFAIL", "record index is out of range");
    }
    if (position < 0) {
        z_fail("RANGEFAIL", "negative record index");
    }
    const char *record = z_file_read_line(text->path, position);
    if (record == NULL) {
        text->out_of_file = 1;
        return 0;
    }
    z_text_assign(text, record, limit);
    z_rc_release((void *)record);
    text->position = position + 1;
    text->out_of_file = 0;
    text->index = 0;
    return 1;
}

static void z_text_begin_write(
    ZText *text,
    int has_index,
    int64_t index,
    int64_t low,
    int64_t high,
    size_t limit
) {
    if (!text->connected) {
        if (has_index) {
            if (index < 0 || (size_t)index > limit) {
                z_fail("TEXTFAIL", "text index is outside the text length");
            }
            text->index = index;
        }
        return;
    }
    if (text->usage == 0) {
        z_fail("WRITEFAIL", "access location is read-only");
    }
    int64_t position = has_index ? index : text->position;
    if (text->indexed && (position < low || position > high)) {
        z_fail("RANGEFAIL", "record index is out of range");
    }
    if (position < 0) {
        z_fail("RANGEFAIL", "negative record index");
    }
    text->position = position;
    text->index = 0;
}

static void z_text_end_write(ZText *text) {
    if (!text->connected) { return; }
    z_file_write_line(text->path, text->position, z_text_view(text));
    text->position++;
}

static void z_text_assign(ZText *text, const char *value, size_t limit) {
    size_t len = strlen(value);
    if (len > limit) {
        z_fail("TEXTFAIL", "text record exceeds the text length");
    }
    char *copy = (char *)z_rc_alloc(len + 1);
    memcpy(copy, value, len + 1);
    z_rc_release(text->record);
    text->record = copy;
    text->len = len;
}

static void z_text_assign_value(ZText *target, ZText value) {
    z_rc_retain(value.record);
    z_rc_release(target->record);
    z_string_assign(&target->path, value.path);
    target->record = value.record;
    target->len = value.len;
    target->index = value.index;
    target->connected = value.connected;
    target->usage = value.usage;
    target->indexed = value.indexed;
    target->position = value.position;
    target->out_of_file = value.out_of_file;
}

static void z_text_drop(ZText *value) {
    z_rc_release(value->record);
    z_string_drop(value->path);
    value->record = NULL;
    value->len = 0;
    value->index = 0;
    value->path = NULL;
    value->connected = 0;
    value->usage = 0;
    value->indexed = 0;
    value->position = 0;
    value->out_of_file = 0;
}

static void z_text_write(ZText *text, const char *value, size_t limit) {
    if (text->index < 0) {
        z_fail("TEXTFAIL", "negative text index");
    }
    size_t at = (size_t)text->index;
    size_t add = strlen(value);
    size_t end = at + add;
    if (end > limit) {
        z_fail("TEXTFAIL", "text record exceeds the text length");
    }
    size_t len = text->len > end ? text->len : end;
    if (len > limit) {
        z_fail("TEXTFAIL", "text record exceeds the text length");
    }
    char *copy = (char *)z_rc_alloc(len + 1);
    memset(copy, ' ', len);
    if (text->record != NULL && text->len > 0) {
        memcpy(copy, text->record, text->len);
    }
    memcpy(copy + at, value, add);
    copy[len] = '\0';
    z_rc_release(text->record);
    text->record = copy;
    text->len = len;
    text->index = (int64_t)end;
}

static void z_text_write_int(ZText *text, int64_t value, size_t limit) {
    char buffer[64];
    snprintf(buffer, sizeof(buffer), "%lld", (long long)value);
    z_text_write(text, buffer, limit);
}

static void z_text_write_bool(ZText *text, int value, size_t limit) {
    z_text_write(text, value ? "TRUE" : "FALSE", limit);
}

static void z_text_write_char(ZText *text, int value, size_t limit) {
    char buffer[2];
    buffer[0] = (char)value;
    buffer[1] = '\0';
    z_text_write(text, buffer, limit);
}

static void z_text_write_string(ZText *text, const char *value, size_t limit) {
    z_text_write(text, value, limit);
}

static void z_text_write_padded(
    ZText *text,
    const char *value,
    size_t limit,
    int64_t width,
    int left,
    char pad,
    int overflow
) {
    size_t len = strlen(value);
    if (width < 0) {
        z_fail("TEXTFAIL", "negative variable width");
    }
    if (width == 0) {
        z_text_write(text, value, limit);
        return;
    }
    size_t target = (size_t)width;
    if (len > target) {
        if (overflow) {
            char *stars = (char *)malloc(target + 1);
            if (stars == NULL) { z_fail("STORAGE", "out of memory"); }
            memset(stars, '*', target);
            stars[target] = '\0';
            z_text_write(text, stars, limit);
            free(stars);
        } else if (left) {
            char *copy = (char *)malloc(target + 1);
            if (copy == NULL) { z_fail("STORAGE", "out of memory"); }
            memcpy(copy, value, target);
            copy[target] = '\0';
            z_text_write(text, copy, limit);
            free(copy);
        } else {
            z_text_write(text, value + (len - target), limit);
        }
        return;
    }
    char *out = (char *)malloc(target + 1);
    if (out == NULL) { z_fail("STORAGE", "out of memory"); }
    memset(out, pad, target);
    if (left) {
        memcpy(out, value, len);
    } else {
        memcpy(out + (target - len), value, len);
    }
    out[target] = '\0';
    z_text_write(text, out, limit);
    free(out);
}

static void z_text_write_int_format(
    ZText *text,
    int64_t value,
    size_t limit,
    int base,
    int64_t width,
    int left,
    char pad,
    int overflow
) {
    char raw[80];
    int position = 0;
    if (value < 0) {
        raw[position++] = '-';
    }
    uint64_t magnitude = value < 0
        ? (uint64_t)(-(value + 1)) + 1
        : (uint64_t)value;
    char digits[64];
    int count = 0;
    if (magnitude == 0) {
        digits[count++] = '0';
    } else {
        while (magnitude != 0) {
            unsigned digit = (unsigned)(magnitude % (uint64_t)base);
            digits[count++] = (char)(digit < 10 ? '0' + digit : 'a' + (digit - 10));
            magnitude /= (uint64_t)base;
        }
    }
    while (count > 0) {
        raw[position++] = digits[--count];
    }
    raw[position] = '\0';
    z_text_write_padded(text, raw, limit, width, left, pad, overflow);
}

static void z_text_write_bool_format(
    ZText *text,
    int value,
    size_t limit,
    int64_t width,
    int left,
    char pad,
    int overflow
) {
    z_text_write_padded(text, value ? "TRUE" : "FALSE", limit, width, left, pad, overflow);
}

static void z_text_write_char_format(
    ZText *text,
    int value,
    size_t limit,
    int64_t width,
    int left,
    char pad,
    int overflow
) {
    char buffer[2];
    buffer[0] = (char)value;
    buffer[1] = '\0';
    z_text_write_padded(text, buffer, limit, width, left, pad, overflow);
}

static void z_text_write_string_format(
    ZText *text,
    const char *value,
    size_t limit,
    int64_t width,
    int left,
    char pad,
    int overflow
) {
    z_text_write_padded(text, value, limit, width, left, pad, overflow);
}

static void z_text_read_literal(ZText *text, const char *expected, size_t limit) {
    (void)limit;
    if (text->index < 0) {
        z_fail("TEXTFAIL", "negative text index");
    }
    size_t at = (size_t)text->index;
    size_t len = strlen(expected);
    if (at + len > text->len
        || memcmp(z_text_view(text) + at, expected, len) != 0) {
        z_fail("TEXTFAIL", "text does not match the format");
    }
    text->index = (int64_t)(at + len);
}

static int64_t z_text_read_int(ZText *text, size_t limit) {
    (void)limit;
    if (text->index < 0) {
        z_fail("TEXTFAIL", "negative text index");
    }
    const char *view = z_text_view(text);
    size_t at = (size_t)text->index;
    if (at > text->len) {
        z_fail("TEXTFAIL", "text index is past the text");
    }
    while (at < text->len && view[at] == ' ') {
        at++;
    }
    char *end = NULL;
    long long value = strtoll(view + at, &end, 10);
    if (end == view + at) {
        z_fail("TEXTFAIL", "invalid numeric text");
    }
    text->index = (int64_t)(end - view);
    return (int64_t)value;
}

static int64_t z_text_read_char(ZText *text, size_t limit) {
    (void)limit;
    if (text->index < 0 || (size_t)text->index >= text->len) {
        z_fail("TEXTFAIL", "text index is out of bounds");
    }
    unsigned char value = (unsigned char)text->record[text->index];
    text->index++;
    return (int64_t)value;
}

static int z_text_read_bool(ZText *text, size_t limit) {
    (void)limit;
    if (text->index < 0) {
        z_fail("TEXTFAIL", "negative text index");
    }
    size_t at = (size_t)text->index;
    if (at + 4 <= text->len && memcmp(z_text_view(text) + at, "TRUE", 4) == 0) {
        text->index = (int64_t)(at + 4);
        return 1;
    }
    if (at + 5 <= text->len && memcmp(z_text_view(text) + at, "FALSE", 5) == 0) {
        text->index = (int64_t)(at + 5);
        return 0;
    }
    z_fail("TEXTFAIL", "invalid boolean text");
    return 0;
}

static void z_text_pad_to(ZText *text, int64_t width, size_t limit) {
    if (width < 0) {
        z_fail("TEXTFAIL", "negative variable width");
    }
    if ((size_t)width > limit) {
        z_fail("TEXTFAIL", "text record exceeds the text length");
    }
    if (text->len < (size_t)width) {
        char *copy = (char *)z_rc_alloc((size_t)width + 1);
        memset(copy, ' ', (size_t)width);
        if (text->record != NULL && text->len > 0) {
            memcpy(copy, text->record, text->len);
        }
        copy[(size_t)width] = '\0';
        z_rc_release(text->record);
        text->record = copy;
        text->len = (size_t)width;
    }
    text->index = width;
}

static void z_text_edit_write(ZText *text, int code, int64_t width, size_t limit) {
    if (width < 0) {
        z_fail("TEXTFAIL", "negative variable width");
    }
    switch (code) {
        case 0: {
            size_t count = (size_t)width;
            char *spaces = (char *)malloc(count + 1);
            if (spaces == NULL) { z_fail("STORAGE", "out of memory"); }
            memset(spaces, ' ', count);
            spaces[count] = '\0';
            z_text_write(text, spaces, limit);
            free(spaces);
            break;
        }
        case 1: {
            if (width > INT64_MAX - text->index) {
                z_fail("TEXTFAIL", "text index overflow");
            }
            int64_t target = text->index + width;
            if ((size_t)target > limit) {
                z_fail("TEXTFAIL", "text record exceeds the text length");
            }
            if (text->len < (size_t)target) {
                z_text_pad_to(text, target, limit);
            } else {
                text->index = target;
            }
            break;
        }
        case 2:
            text->index -= width;
            if (text->index < 0) {
                z_fail("TEXTFAIL", "text index moved before the text");
            }
            break;
        case 3:
            z_text_pad_to(text, width, limit);
            break;
        default:
            z_fail("TEXTFAIL", "unknown editing clause");
    }
}

static void z_text_edit_read(ZText *text, int code, int64_t width, size_t limit) {
    (void)limit;
    if (width < 0) {
        z_fail("TEXTFAIL", "negative variable width");
    }
    size_t start = (size_t)text->index;
    size_t count = (size_t)width;
    switch (code) {
        case 0:
            if (start + count > text->len) {
                z_fail("TEXTFAIL", "text slice is out of bounds");
            }
            for (size_t index = start; index < start + count; index++) {
                if (text->record[index] != ' ' && text->record[index] != '\t') {
                    z_fail("TEXTFAIL", "expected space characters");
                }
            }
            text->index = (int64_t)(start + count);
            break;
        case 1:
            text->index += width;
            if ((size_t)text->index > text->len) {
                z_fail("TEXTFAIL", "text index moved past the text");
            }
            break;
        case 2:
            text->index -= width;
            if (text->index < 0) {
                z_fail("TEXTFAIL", "text index moved before the text");
            }
            break;
        case 3:
            if ((size_t)width > text->len) {
                z_fail("TEXTFAIL", "text index moved past the text");
            }
            text->index = width;
            break;
        default:
            z_fail("TEXTFAIL", "unknown editing clause");
    }
}

static void z_text_io_write(ZText *text, int control, size_t limit) {
    switch (control) {
        case 0:
        case 3:
            z_text_write(text, "\n", limit);
            break;
        case 1:
            z_text_write(text, "\f", limit);
            break;
        default:
            break;
    }
}

static void z_text_io_read(ZText *text, int control, size_t limit) {
    (void)limit;
    char expected = '\0';
    switch (control) {
        case 0:
        case 3:
            expected = '\n';
            break;
        case 1:
            expected = '\f';
            break;
        default:
            return;
    }
    if (text->index < 0 || (size_t)text->index >= text->len
        || text->record[text->index] != expected) {
        z_fail("TEXTFAIL", "expected a record separator");
    }
    text->index++;
}

static char z_text_upper(char value) {
    return value >= 'a' && value <= 'z' ? (char)(value - 'a' + 'A') : value;
}

static int64_t z_text_read_int_format(
    ZText *text,
    size_t limit,
    int base,
    int64_t width,
    int left,
    char pad
) {
    (void)limit;
    if (width < 0) {
        z_fail("TEXTFAIL", "negative variable width");
    }
    const char *view = z_text_view(text);
    size_t start = (size_t)text->index;
    if (width == 0) {
        size_t at = start;
        while (at < text->len && (view[at] == ' ' || view[at] == '\t')) { at++; }
        char *end = NULL;
        long long value = strtoll(view + at, &end, base);
        if (end == view + at) {
            z_fail("TEXTFAIL", "invalid numeric text");
        }
        text->index = (int64_t)(end - view);
        return (int64_t)value;
    }
    if (start + (size_t)width > text->len) {
        z_fail("TEXTFAIL", "text slice is out of bounds");
    }
    size_t count = (size_t)width;
    char *field = (char *)malloc(count + 1);
    if (field == NULL) { z_fail("STORAGE", "out of memory"); }
    memcpy(field, view + start, count);
    field[count] = '\0';
    char *begin = field;
    char *finish = field + count;
    if (left) {
        while (finish > begin && finish[-1] == pad) { finish--; }
    } else {
        while (begin < finish && *begin == pad) { begin++; }
    }
    *finish = '\0';
    char *end = NULL;
    long long value = strtoll(begin, &end, base);
    if (end == begin || *end != '\0') {
        free(field);
        z_fail("TEXTFAIL", "invalid integer in text");
    }
    free(field);
    text->index = (int64_t)(start + count);
    return (int64_t)value;
}

static int z_text_read_bool_format(
    ZText *text,
    size_t limit,
    int64_t width,
    int left,
    char pad
) {
    (void)limit;
    if (width < 0) {
        z_fail("TEXTFAIL", "negative variable width");
    }
    const char *view = z_text_view(text);
    size_t start = (size_t)text->index;
    size_t count = width == 0 ? 0 : (size_t)width;
    if (count > 0 && start + count > text->len) {
        z_fail("TEXTFAIL", "text slice is out of bounds");
    }
    char *field = NULL;
    const char *begin;
    size_t length;
    if (count == 0) {
        begin = view + start;
        length = text->len > start ? text->len - start : 0;
    } else {
        field = (char *)malloc(count + 1);
        if (field == NULL) { z_fail("STORAGE", "out of memory"); }
        memcpy(field, view + start, count);
        field[count] = '\0';
        begin = field;
        length = count;
        if (left) {
            while (length > 0 && field[length - 1] == pad) { length--; }
        } else {
            size_t skip = 0;
            while (skip < length && field[skip] == pad) { skip++; }
            begin = field + skip;
            length -= skip;
        }
    }
    while (length > 0 && (*begin == ' ' || *begin == '\t')) {
        begin++;
        length--;
    }
    int result;
    if (length >= 4 && z_text_upper(begin[0]) == 'T' && z_text_upper(begin[1]) == 'R'
        && z_text_upper(begin[2]) == 'U' && z_text_upper(begin[3]) == 'E') {
        result = 1;
        if (count == 0) {
            text->index = (int64_t)(start + (size_t)(begin - view) + 4);
        } else {
            text->index = (int64_t)(start + count);
        }
    } else if (length >= 5 && z_text_upper(begin[0]) == 'F' && z_text_upper(begin[1]) == 'A'
        && z_text_upper(begin[2]) == 'L' && z_text_upper(begin[3]) == 'S'
        && z_text_upper(begin[4]) == 'E') {
        result = 0;
        if (count == 0) {
            text->index = (int64_t)(start + (size_t)(begin - view) + 5);
        } else {
            text->index = (int64_t)(start + count);
        }
    } else {
        free(field);
        z_fail("TEXTFAIL", "expected TRUE or FALSE");
        result = 0;
    }
    if (field != NULL) { free(field); }
    return result;
}

static int64_t z_text_read_char_format(
    ZText *text,
    size_t limit,
    int64_t width,
    int left,
    char pad
) {
    (void)limit;
    if (width < 0) {
        z_fail("TEXTFAIL", "negative variable width");
    }
    const char *view = z_text_view(text);
    size_t start = (size_t)text->index;
    if (width == 0) {
        if (start >= text->len) {
            z_fail("TEXTFAIL", "expected a character");
        }
        text->index++;
        return (unsigned char)view[start];
    }
    size_t count = (size_t)width;
    if (start + count > text->len) {
        z_fail("TEXTFAIL", "text slice is out of bounds");
    }
    size_t begin = start;
    size_t finish = start + count;
    if (left) {
        while (finish > begin && view[finish - 1] == pad) { finish--; }
    } else {
        while (begin < finish && view[begin] == pad) { begin++; }
    }
    if (finish - begin != 1) {
        z_fail("TEXTFAIL", "expected one character");
    }
    text->index = (int64_t)(start + count);
    return (unsigned char)view[begin];
}

static const char *z_text_read_string_format(
    ZText *text,
    size_t limit,
    int64_t width
) {
    (void)limit;
    if (width < 0) {
        z_fail("TEXTFAIL", "negative variable width");
    }
    const char *view = z_text_view(text);
    size_t start = (size_t)text->index;
    size_t count = width == 0
        ? (text->len > start ? text->len - start : 0)
        : (size_t)width;
    if (start + count > text->len) {
        z_fail("TEXTFAIL", "text slice is out of bounds");
    }
    char *result = (char *)z_rc_alloc(count + 1);
    memcpy(result, view + start, count);
    result[count] = ' ';
    text->index = (int64_t)(start + count);
    return result;
}

static void z_text_reset_index(ZText *text) {
    text->index = 0;
}

static int64_t z_text_get_index(ZText text) {
    return text.index;
}

static void z_text_set_index(ZText *text, int64_t index, size_t limit) {
    if (index < 0 || (size_t)index > limit) {
        z_fail("TEXTFAIL", "text index is outside the text length");
    }
    text->index = index;
}

static int z_text_equal_text(const ZText *left, const ZText *right) {
    return strcmp(z_text_view(left), z_text_view(right)) == 0;
}

static int z_text_equal_string(const ZText *left, const char *right) {
    return strcmp(z_text_view(left), right) == 0;
}

"#,
    );

    emit_struct_typedefs(&mut out, modes)?;
    emit_array_result_typedefs(&mut out, &modules, module_modes)?;
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
            prelude: Vec::new(),
            temporaries: Vec::new(),
            scopes: vec![Vec::new()],
        };
        generator.line(&mut out, "ZHandler *__z_entry_handlers = z_handlers;");
        generator.emit_initializers(&mut out, &module.declarations)?;
        if let Some(handler) = &module.handler {
            generator.emit_handled(&mut out, &module.actions, handler)?;
        } else {
            generator.emit_actions(&mut out, &module.actions)?;
        }
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
            prelude: Vec::new(),
            temporaries: Vec::new(),
            scopes: vec![Vec::new()],
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
    out.push_str("        printf(\"live-payloads: %zu\\n\", z_rc_live);\n");
    out.push_str("    }\n");
    out.push_str("    return 0;\n}\n");
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
        prelude: Vec::new(),
        temporaries: Vec::new(),
        scopes: vec![Vec::new()],
    };
    generator.line(out, "ZHandler *__z_entry_handlers = z_handlers;");
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
                    generator.record_owned_local(&local, kind);
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
            generator.record_owned_local(&local, kind);
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
        generator.line(out, "z_handlers = __z_entry_handlers;");
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
        self.temporaries.push((name.clone(), kind));
        Ok(name)
    }

    fn register_temporary(&mut self, name: String, kind: Kind) {
        self.temporaries.push((name, kind));
    }

    fn drop_temporaries(&mut self, out: &mut String) -> Result<(), String> {
        self.flush_prelude(out);
        let temporaries = std::mem::take(&mut self.temporaries);
        for (name, kind) in temporaries.into_iter().rev() {
            self.drop_kind(out, &name, kind)?;
        }
        Ok(())
    }

    fn record_owned_local(&mut self, name: &str, kind: Kind) {
        if self.kind_owns(kind) {
            if let Some(scope) = self.scopes.last_mut() {
                scope.push((name.to_owned(), kind));
            }
        }
    }

    fn drop_scope(&mut self, out: &mut String) -> Result<(), String> {
        let scope = self.scopes.pop().unwrap_or_default();
        for (name, kind) in scope.into_iter().rev() {
            self.drop_kind(out, &name, kind)?;
        }
        Ok(())
    }

    fn drop_live_scopes(&mut self, out: &mut String) -> Result<(), String> {
        while !self.scopes.is_empty() {
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
                            self.record_owned_local(&c_name, info.kind);
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
                            self.record_owned_local(&c_name, kind);
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
                            self.record_owned_local(&c_name, kind);
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
                        self.line(out, "z_handlers = __z_entry_handlers;");
                        self.line(out, &format!("return {name};"));
                    } else {
                        self.drop_temporaries(out)?;
                        self.drop_live_scopes(out)?;
                        self.line(out, "z_handlers = __z_entry_handlers;");
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
                    self.line(out, "break;");
                }
                Action::Exit {
                    label: Some(label), ..
                } => {
                    let Some((_, target)) =
                        self.labels.iter().rev().find(|(name, _)| name == label)
                    else {
                        return Err(format!(
                            "native backend: unknown labelled EXIT target `{label}`"
                        ));
                    };
                    self.line(out, &format!("goto {target};"));
                }
                Action::Labelled { label, action, .. } => {
                    let unique = self.temp_counter;
                    self.temp_counter += 1;
                    let c_label = format!("__label_{unique}_{}", c_ident(label));
                    self.labels.push((label.clone(), c_label.clone()));
                    self.line(out, "{");
                    self.indent += 1;
                    self.emit_actions(out, std::slice::from_ref(action.as_ref()))?;
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
                self.drop_temporaries(out)?;
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
        self.line(out, &format!("{state}.prev = z_handlers;"));
        self.line(out, &format!("z_handlers = &{state};"));
        self.line(out, &format!("if (setjmp({state}.env) == 0) {{"));
        self.indent += 1;
        self.emit_actions(out, body)?;
        self.indent -= 1;
        self.line(out, "} else {");
        self.indent += 1;
        self.line(out, &format!("const char *{exception} = z_exception_name;"));
        self.line(out, &format!("z_handlers = {state}.prev;"));
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
        self.line(out, &format!("z_handlers = {state}.prev;"));
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
                self.loop_depth += 1;
                self.emit_actions(out, body)?;
                self.loop_depth -= 1;
                self.indent -= 1;
                self.line(out, "}");
            }
            DoControl::ForEver | DoControl::None => {
                self.line(out, "for (;;) {");
                self.indent += 1;
                self.loop_depth += 1;
                self.emit_actions(out, body)?;
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
                self.loop_depth += 1;
                self.emit_actions(out, body)?;
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
                    let size = array_size_expression(target.kind)?;
                    self.line(
                        out,
                        &format!("memcpy({}, {}, {size});", target.c_name, info.c_name),
                    );
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
        self.line(out, "z_handlers = __z_entry_handlers;");
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
            ScalarKind::Int(_) | ScalarKind::Bool | ScalarKind::Char | ScalarKind::Struct(_) => {
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

    fn transfer_target(&self, expression: &Expr) -> Result<(String, Kind), String> {
        let Expr::Name { name, .. } = expression else {
            return Err("native backend: transfer operations require simple variables".to_owned());
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
                        ConversionCode::Default => 10,
                        ConversionCode::Scientific => unreachable!(),
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
                Kind::Int(_) | Kind::Bool | Kind::Char => {
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
                    UnaryOp::Neg => {
                        let result = format!("z_neg({code})");
                        if let Kind::Int(int_kind) = kind {
                            Ok((self.checked_int(&result, int_kind)?, Kind::Int(int_kind)))
                        } else {
                            Ok((result, Kind::Int(IntKind::Long)))
                        }
                    }
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
                ) {
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
            Expr::Apply { name, args, span } => match self.check.resolutions.get(span) {
                Some(ApplyResolution::ArrayIndex(Type::Array(_)))
                | Some(ApplyResolution::ArrayIndex(Type::MultiArray(_))) => {
                    let Some(info) = self.vars.get(name).cloned() else {
                        return Err(format!("native backend: unknown array `{name}`"));
                    };
                    self.array_index_expr(&info, args, *span)
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
            },
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
    conversion: &crate::builtins::Conversion,
    _kind: TextIo,
) -> Result<(), String> {
    if !matches!(
        conversion.code,
        ConversionCode::Default
            | ConversionCode::Binary
            | ConversionCode::Octal
            | ConversionCode::Hex
    ) {
        return Err("native backend: scientific text conversions are not supported yet".to_owned());
    }
    if conversion.fractional_width.is_some() || conversion.exponent_width.is_some() {
        return Err(
            "native backend: text fractional/exponent widths are not supported yet".to_owned(),
        );
    }
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
        Type::Bool => Ok(ScalarKind::Bool),
        Type::Char | Type::WChar => Ok(ScalarKind::Char),
        Type::String | Type::BoundedString { .. } | Type::WString | Type::WBoundedString { .. } => {
            Ok(ScalarKind::String)
        }
        Type::Struct(id) => Ok(ScalarKind::Struct(id)),
        Type::PowerSet(set) => Ok(ScalarKind::PowerSet(set.member)),
        other => Err(format!(
            "native backend: array element mode `{}` is not supported yet",
            other.describe()
        )),
    }
}

fn scalar_kind(kind: Kind) -> Option<ScalarKind> {
    match kind {
        Kind::Int(kind) => Some(ScalarKind::Int(kind)),
        Kind::Bool => Some(ScalarKind::Bool),
        Kind::Char => Some(ScalarKind::Char),
        Kind::String => Some(ScalarKind::String),
        Kind::Struct(id) => Some(ScalarKind::Struct(id)),
        Kind::PowerSet(member) => Some(ScalarKind::PowerSet(member)),
        _ => None,
    }
}

fn scalar_ref_target(target: types::RefTarget) -> Result<ScalarKind, String> {
    use types::RefTarget;
    match target {
        RefTarget::Int(kind) => Ok(ScalarKind::Int(kind)),
        RefTarget::Bool => Ok(ScalarKind::Bool),
        RefTarget::Struct(id) => Ok(ScalarKind::Struct(id)),
        RefTarget::PowerSet(set) => Ok(ScalarKind::PowerSet(set.member)),
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
                Type::Bool => ScalarKind::Bool,
                Type::Set(_) => ScalarKind::Int(IntKind::Long),
                Type::Struct(id) => ScalarKind::Struct(id),
                Type::PowerSet(set) => ScalarKind::PowerSet(set.member),
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
                Type::Bool => ScalarKind::Bool,
                Type::Set(_) => ScalarKind::Int(IntKind::Long),
                Type::Struct(id) => ScalarKind::Struct(id),
                Type::PowerSet(set) => ScalarKind::PowerSet(set.member),
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
