use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

fn example_grant_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn z200c() -> Command {
    Command::new(env!("CARGO_BIN_EXE_z200c"))
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("src")).unwrap();
        Self { path }
    }

    fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.path.join(path)
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<std::ffi::OsStr> for TempDir {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.path.as_os_str()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn example(name: &str) -> String {
    format!("{}/examples/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn check_hello_succeeds() {
    let output = z200c().arg("check").arg(example("hello")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("checked hello"));
}

#[test]
fn run_hello_prints_greeting() {
    let output = z200c().arg("run").arg(example("hello")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Hello, world!\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn run_text_output_matches_golden() {
    let directory = TempDir::new("z200c-golden");
    std::fs::write(
        directory.join("Chill.toml"),
        r#"
[package]
name = "golden"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        directory.join("src/main.ch"),
        r#"
M: MODULE
  WRITE("value=");
  WRITELINE("ok");
  WRITELINE();
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&directory).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"value=ok\n\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn run_procedures_example_succeeds_without_output() {
    let output = z200c()
        .arg("run")
        .arg(example("procedures"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn run_pipeline_example_succeeds() {
    let output = z200c()
        .arg("run")
        .arg(example("pipeline"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn pipeline_is_stable_under_yield_variations() {
    for interval in ["1", "2", "3", "5", "8", "13"] {
        let output = z200c()
            .env("Z200C_YIELD_INTERVAL", interval)
            .arg("run")
            .arg(example("pipeline"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "yield interval {interval}: stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn run_modules_example_succeeds() {
    let output = z200c().arg("run").arg(example("modules")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn run_spec_example_succeeds() {
    let output = z200c().arg("run").arg(example("spec")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn run_control_example_succeeds() {
    let output = z200c().arg("run").arg(example("control")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn run_multifile_example_succeeds() {
    let output = z200c()
        .arg("run")
        .arg(example("multifile"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_showcase_example_succeeds() {
    let output = z200c()
        .arg("run")
        .arg(example("showcase"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn compiles_native_hello_world() {
    let dir = TempDir::new("z200c-native-hello");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativehello"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Hello: MODULE
  WRITELINE("Hello, native!");
END Hello;
"#,
    )
    .unwrap();

    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativehello");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Hello, native!\n");
}

#[test]
fn compiles_native_integer_program() {
    let dir = TempDir::new("z200c-native-int");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeint"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL x INT INIT := 2;
  DCL y INT INIT := 3;
  DCL z INT;
  z := x + y * 2;
  ASSERT z = 8;
  IF z = 8 THEN
    WRITELINE("native ok");
  ELSE
    WRITELINE("bad");
  FI;
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeint");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "native ok\n");
}

#[test]
fn compiles_native_procedures_example() {
    let output = z200c()
        .arg("compile")
        .arg(example("procedures"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = format!(
        "{}/examples/procedures/target/z200c/procedures",
        env!("CARGO_MANIFEST_DIR")
    );
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert!(run.stdout.is_empty());
}

#[test]
fn compiles_native_array_program() {
    let dir = TempDir::new("z200c-native-array");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearray"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT;
  a(1) := 4;
  a(2) := 5;
  a(3) := a(1) + a(2);
  ASSERT a(3) = 9;
  WRITELINE("array ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearray");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "array ok\n");
}

#[test]
fn compiles_native_loop_program() {
    let dir = TempDir::new("z200c-native-loop");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeloop"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT;
  DCL sum INT INIT := 0;
  DO FOR i := 1 TO 3;
    a(i) := i * 2;
    sum := sum + a(i);
  OD;
  ASSERT sum = 12;
  DO WHILE sum > 10;
    sum := sum - 1;
  OD;
  ASSERT sum = 10;
  WRITELINE("loop ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeloop");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "loop ok\n");
}

#[test]
fn compiles_native_array_literals() {
    let dir = TempDir::new("z200c-native-arraylit");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearraylit"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT INIT := [1, 2, 3];
  DCL b ARRAY(1:3) INT;
  b := [4, 5, 6];
  ASSERT a(2) = 2;
  ASSERT b(3) = 6;
  WRITELINE("lit ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearraylit");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "lit ok\n");
}

#[test]
fn compiles_native_array_copy() {
    let dir = TempDir::new("z200c-native-arraycopy");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearraycopy"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT INIT := [1, 2, 3];
  DCL b ARRAY(1:3) INT;
  b := a;
  ASSERT b(1) = 1;
  ASSERT b(3) = 3;
  WRITELINE("copy ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearraycopy");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "copy ok\n");
}

#[test]
fn compiles_native_case_program() {
    let dir = TempDir::new("z200c-native-case");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativecase"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL x INT INIT := 2;
  DCL result INT INIT := 0;
  CASE x OF
    (1): result := 10;
    (2, 3): result := 20;
    (ELSE): result := 30;
  ESAC;
  ASSERT result = 20;
  CASE x OF
    (4): result := 40;
    ELSE
      result := 50;
  ESAC;
  ASSERT result = 50;
  WRITELINE("case ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativecase");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "case ok\n");
}

#[test]
fn compiles_native_multiple_case_selectors() {
    let dir = TempDir::new("z200c-native-case2");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativecase2"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL x INT INIT := 1;
  DCL y INT INIT := 3;
  DCL result INT INIT := 0;
  CASE x, y OF
    (1), (2): result := 12;
    (1), (3): result := 13;
    (ELSE), (ELSE): result := 99;
  ESAC;
  ASSERT result = 13;
  WRITELINE("case2 ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativecase2");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "case2 ok\n");
}

#[test]
fn compiles_native_begin_blocks() {
    let dir = TempDir::new("z200c-native-begin");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativebegin"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL x INT INIT := 1;
  BEGIN
    DCL x INT INIT := 5;
    DCL y INT INIT := x + 2;
    ASSERT y = 7;
    BEGIN
      DCL z ARRAY(1:2) INT INIT := [3, 4];
      ASSERT z(2) = 4;
    END;
  END;
  ASSERT x = 1;
  WRITELINE("begin ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativebegin");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "begin ok\n");
}

#[test]
fn compiles_native_multidimensional_arrays() {
    let dir = TempDir::new("z200c-native-md");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativemd"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:2, 1:2) INT INIT := [[1, 2], [3, 4]];
  DCL b ARRAY(1:2, 1:2) INT;
  b := a;
  ASSERT a(2, 1) = 3;
  ASSERT b(1, 2) = 2;
  b(2, 2) := 9;
  ASSERT b(2, 2) = 9;
  WRITELINE("md ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativemd");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "md ok\n");
}

#[test]
fn compiles_native_array_equality() {
    let dir = TempDir::new("z200c-native-arrayeq");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearrayeq"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT INIT := [1, 2, 3];
  DCL b ARRAY(1:3) INT INIT := [1, 2, 3];
  DCL c ARRAY(1:3) INT INIT := [1, 2, 4];
  ASSERT a = b;
  ASSERT a /= c;
  IF a = b THEN
    WRITELINE("eq ok");
  FI;
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearrayeq");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "eq ok\n");
}

#[test]
fn compiles_native_string_comparisons() {
    let dir = TempDir::new("z200c-native-str");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativestr"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a CHARS INIT := "alpha";
  DCL b CHARS INIT := "beta";
  ASSERT a = "alpha";
  ASSERT a /= b;
  ASSERT a < b;
  WRITELINE("str ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativestr");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "str ok\n");
}

#[test]
fn compiles_native_string_concatenation() {
    let dir = TempDir::new("z200c-native-concat");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeconcat"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a CHARS INIT := "foo";
  DCL b CHARS INIT := "bar";
  DCL c CHARS;
  c := a // b;
  ASSERT c = "foobar";
  a //:= "!";
  ASSERT a = "foo!";
  WRITELINE(c);
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeconcat");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "foobar\n");
}

#[test]
fn compiles_native_out_inout_parameters() {
    let dir = TempDir::new("z200c-native-params");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeparams"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  Calc: PROC (n INT, doubled INT OUT, total INT INOUT);
    doubled := n * 2;
    total := total + doubled;
  END Calc;

  DCL n INT INIT := 3;
  DCL d INT INIT := 0;
  DCL t INT INIT := 1;
  Calc(n, d, t);
  ASSERT d = 6;
  ASSERT t = 7;
  WRITELINE("params ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeparams");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "params ok\n");
}

#[test]
fn compiles_native_scalar_references() {
    let dir = TempDir::new("z200c-native-ref");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeref"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL x INT INIT := 3;
  DCL p REF INT;
  p := ->x;
  ASSERT p-> = 3;
  p-> := 7;
  ASSERT x = 7;
  WRITELINE("ref ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeref");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "ref ok\n");
}

#[test]
fn compiles_native_null_references() {
    let dir = TempDir::new("z200c-native-null");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativenull"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL x INT INIT := 1;
  DCL p REF INT INIT := NULL;
  DCL q REF INT;
  ASSERT p = NULL;
  p := ->x;
  ASSERT p /= NULL;
  q := NULL;
  ASSERT q = NULL;
  WRITELINE("null ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativenull");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "null ok\n");
}

#[test]
fn compiles_native_array_element_references() {
    let dir = TempDir::new("z200c-native-refarr");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativerefarr"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT INIT := [1, 2, 3];
  DCL p REF INT;
  p := ->a(2);
  ASSERT p-> = 2;
  p-> := 9;
  ASSERT a(2) = 9;
  WRITELINE("refarr ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativerefarr");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "refarr ok\n");
}

#[test]
fn compiles_native_set_modes() {
    let dir = TempDir::new("z200c-native-set");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeset"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  NEWMODE Color = SET(red, green, blue);
  DCL c Color INIT := red;
  DCL d Color;
  DCL arr ARRAY(1:2) Color;
  ASSERT c = red;
  c := blue;
  ASSERT c /= red;
  d := c;
  ASSERT d = blue;
  arr(1) := green;
  ASSERT arr(1) = green;
  WRITELINE("set ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeset");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "set ok\n");
}

#[test]
fn compiles_native_readline() {
    let dir = TempDir::new("z200c-native-read");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeread"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL line CHARS;
  line := READLINE();
  WRITELINE(line);
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeread");
    let mut child = std::process::Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"hello\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\n");
}

#[test]
fn compiles_native_labelled_exit() {
    let dir = TempDir::new("z200c-native-label");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativelabel"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL i INT INIT := 0;
  DCL j INT INIT := 0;
  DCL found INT INIT := 0;
  Outer: DO FOR a := 1 TO 3;
    DO FOR b := 1 TO 3;
      i := a;
      j := b;
      IF b = 2 THEN
        found := a;
        EXIT Outer;
      FI;
    OD;
  OD;
  ASSERT found = 1;
  ASSERT i = 1;
  ASSERT j = 2;
  WRITELINE("label ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativelabel");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "label ok\n");
}

#[test]
fn compiles_native_structures() {
    let dir = TempDir::new("z200c-native-struct");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativestruct"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  NEWMODE Point = STRUCT (x INT, y INT);
  NEWMODE Line = STRUCT (a Point, b Point);
  DCL p Point;
  DCL q Point;
  DCL l Line;
  p.x := 2;
  p.y := 3;
  q := p;
  ASSERT q.x = 2;
  q.y := 9;
  ASSERT q.y = 9;
  ASSERT p.y = 3;
  l.a := p;
  l.b := q;
  ASSERT l.a.x = 2;
  ASSERT l.b.y = 9;
  WRITELINE("struct ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativestruct");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "struct ok\n");
}

#[test]
fn compiles_native_structure_equality_and_field_references() {
    let dir = TempDir::new("z200c-native-structeq");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativestructeq"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  NEWMODE Point = STRUCT (x INT, y INT);
  DCL p Point;
  DCL q Point;
  DCL r REF INT;
  p.x := 2;
  p.y := 3;
  q := p;
  ASSERT p = q;
  q.y := 9;
  ASSERT p /= q;
  r := ->q.y;
  r-> := 7;
  ASSERT q.y = 7;
  WRITELINE("structeq ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativestructeq");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "structeq ok\n");
}

#[test]
fn native_runtime_checks_fail_cleanly() {
    let cases = [
        (
            "bounds",
            r#"
M: MODULE
  DCL a ARRAY(1:2) INT;
  a(3) := 1;
END M;
"#,
            "RANGEFAIL: array index out of bounds",
        ),
        (
            "divzero",
            r#"
M: MODULE
  DCL x INT INIT := 1;
  DCL y INT INIT := 0;
  x := x / y;
END M;
"#,
            "OVERFLOW: division by zero",
        ),
        (
            "overflow",
            r#"
M: MODULE
  DCL x LONG_INT INIT := 9223372036854775807;
  x := x + 1;
END M;
"#,
            "OVERFLOW: integer overflow",
        ),
        (
            "nullderef",
            r#"
M: MODULE
  DCL p REF INT;
  ASSERT p-> = 1;
END M;
"#,
            "EMPTY: NULL dereference",
        ),
    ];
    for (name, source, expected) in cases {
        let dir = TempDir::new(&format!("z200c-native-{name}"));
        let package = format!("native{name}");
        std::fs::write(
            dir.join("Chill.toml"),
            format!(
                r#"
[package]
name = "{package}"

[build]
entry = "src/main.ch"
"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src/main.ch"), source).unwrap();
        let output = z200c().arg("compile").arg(&dir).output().unwrap();
        assert!(
            output.status.success(),
            "compile {name}: stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let binary = dir.join(format!("target/z200c/{package}"));
        let run = std::process::Command::new(binary).output().unwrap();
        assert!(
            !run.status.success(),
            "native run `{name}` unexpectedly succeeded"
        );
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(stderr.contains(expected), "stderr: {stderr}");
    }
}

#[test]
fn compiles_native_arrays_of_structures() {
    let dir = TempDir::new("z200c-native-arrstruct");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearrstruct"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  NEWMODE Point = STRUCT (x INT, y INT);
  DCL points ARRAY(1:2) Point;
  DCL other ARRAY(1:2) Point;
  points(1).x := 1;
  points(1).y := 2;
  points(2).x := 3;
  points(2).y := 4;
  ASSERT points(2).x = 3;
  other := points;
  ASSERT other(1).y = 2;
  WRITELINE("arrstruct ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearrstruct");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "arrstruct ok\n");
}

#[test]
fn compiles_native_exceptions_and_handlers() {
    let dir = TempDir::new("z200c-native-exceptions");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeexc"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:2) INT;

  P: PROC (n INT) RETURNS (INT) EXCEPTIONS (fail);
    IF n < 0 THEN
      CAUSE fail;
    FI;
    RETURN n;
  END
  ON (fail):
    RESULT -1;
  END P;

  ASSERT P(-1) = -1;
  ASSERT P(3) = 3;

  BEGIN
    a(3) := 1;
  END
  ON (RANGEFAIL):
    WRITELINE("range caught");
  END;

  BEGIN
    ASSERT FALSE;
  END
  ON (ASSERTFAIL):
    WRITELINE("assert caught");
  END;

  BEGIN
    DCL x LONG_INT INIT := 9223372036854775807;
    x := x + 1;
  END
  ON (OVERFLOW):
    WRITELINE("overflow caught");
  END;

  CAUSE boom;
END
ON (boom):
  WRITELINE("boom caught");
END;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeexc");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "range caught
assert caught
overflow caught
boom caught
"
    );
}

#[test]
fn native_unhandled_exception_fails_cleanly() {
    let dir = TempDir::new("z200c-native-unhandled");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeunhandled"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  CAUSE boom;
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeunhandled");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(!run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("UNHANDLED: boom"),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn compiles_native_powersets() {
    let dir = TempDir::new("z200c-native-powersets");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeps"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  NEWMODE Flags = STRUCT (on POWERSET CHAR, count INT);
  DCL f Flags;
  DCL g Flags;
  DCL base POWERSET CHAR INIT := ['a':'c'];
  DCL extra POWERSET CHAR INIT := ['x'];
  DCL digits POWERSET INT;
  DCL two POWERSET INT INIT := [2];
  DCL middle POWERSET INT INIT := [3:4];
  DCL three POWERSET INT INIT := [3];
  DCL sets ARRAY(1:2) POWERSET INT;
  DCL copy ARRAY(1:2) POWERSET INT;

  F: PROC (left POWERSET CHAR, right POWERSET CHAR) RETURNS (POWERSET CHAR);
    RETURN left + right;
  END F;

  P: PROC (io POWERSET CHAR INOUT);
    io +:= ['z'];
  END P;

  base := ['a':'c'];
  ASSERT 'b' IN base;
  ASSERT NOT ('z' IN base);
  ASSERT base = ['a', 'b', 'c'];

  digits := [1, 3:5];
  ASSERT 3 IN digits;
  ASSERT NOT (2 IN digits);
  ASSERT digits = [1, 3, 4, 5];
  ASSERT digits + two = [1:5];
  ASSERT digits * middle = [3, 4];
  ASSERT digits - three = [1, 4, 5];

  f.on := base;
  f.count := 3;
  g := f;
  ASSERT g.on = ['a':'c'];
  g.on -:= ['b'];
  ASSERT g.on = ['a', 'c'];
  ASSERT f.on = ['a':'c'];

  ASSERT F(base, extra) = ['a':'c', 'x'];
  P(base);
  ASSERT base = ['a':'c', 'z'];
  ASSERT 'z' IN base;

  sets(1) := [1, 3];
  sets(2) := [2:4];
  copy := sets;
  ASSERT sets = copy;
  ASSERT 3 IN sets(1);
  copy(2) := [9];
  ASSERT sets /= copy;

  base := [];
  ASSERT base = [];
  ASSERT NOT ('a' IN base);
  WRITELINE("powerset ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeps");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "powerset ok
"
    );
}

#[test]
fn native_unordered_powerset_comparison_fails_cleanly() {
    let dir = TempDir::new("z200c-native-powerset-order");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativepsorder"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a POWERSET INT;
  DCL b POWERSET INT;
  a := [1];
  b := [2];
  ASSERT a < b;
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativepsorder");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(!run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("E0500: powersets are not ordered"),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn compiles_native_array_parameters() {
    let dir = TempDir::new("z200c-native-array-params");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearrparam"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT;
  DCL b ARRAY(1:3) INT;
  DCL m ARRAY(1:2, 1:2) INT;
  DCL n ARRAY(0:1, 2:3) INT;
  NEWMODE Point = STRUCT (x INT, y INT);
  DCL points ARRAY(1:2) Point;

  Sum: PROC (v ARRAY(1:3) INT) RETURNS (INT);
    RETURN v(1) + v(2) + v(3);
  END Sum;

  MutateIn: PROC (v ARRAY(1:3) INT);
    v(1) := 99;
  END MutateIn;

  Fill: PROC (v ARRAY(1:3) INT OUT);
    v(1) := 7;
    v(2) := 8;
    v(3) := 9;
  END Fill;

  Bump: PROC (v ARRAY(1:3) INT INOUT);
    v(1) +:= 10;
    v(2) +:= 10;
    v(3) +:= 10;
  END Bump;

  Alias: PROC (v ARRAY(1:3) INT LOC);
    v(3) := 42;
  END Alias;

  Sum2: PROC (v ARRAY(1:2, 1:2) INT) RETURNS (INT);
    RETURN v(1,1) + v(1,2) + v(2,1) + v(2,2);
  END Sum2;

  Fill2: PROC (v ARRAY(0:1, 2:3) INT OUT);
    v(0,2) := 1;
    v(0,3) := 2;
    v(1,2) := 3;
    v(1,3) := 4;
  END Fill2;

  MovePoint: PROC (p ARRAY(1:2) Point INOUT);
    p(1).x := 5;
    p(2).y := 6;
  END MovePoint;

  a := [1, 2, 3];
  ASSERT Sum(a) = 6;
  MutateIn(a);
  ASSERT a(1) = 1;
  ASSERT a(2) = 2;
  ASSERT a(3) = 3;

  Fill(b);
  ASSERT b(1) = 7;
  ASSERT b(2) = 8;
  ASSERT b(3) = 9;
  Bump(b);
  ASSERT b(1) = 17;
  ASSERT b(2) = 18;
  ASSERT b(3) = 19;
  Alias(b);
  ASSERT b(3) = 42;

  m := [[1,2],[3,4]];
  ASSERT Sum2(m) = 10;
  Fill2(n);
  ASSERT n(0,2) = 1;
  ASSERT n(0,3) = 2;
  ASSERT n(1,2) = 3;
  ASSERT n(1,3) = 4;

  MovePoint(points);
  ASSERT points(1).x = 5;
  ASSERT points(2).y = 6;
  WRITELINE("array params ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearrparam");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "array params ok
"
    );
}

#[test]
fn compiles_native_array_results() {
    let dir = TempDir::new("z200c-native-array-results");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearrret"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:2) INT;
  DCL b ARRAY(1:2) INT;
  NEWMODE Point = STRUCT (x INT, y INT);
  DCL points ARRAY(1:2) Point;

  Pair: PROC () RETURNS (ARRAY(1:2) INT);
    RETURN [11, 12];
  END Pair;

  Copy: PROC (v ARRAY(1:2) INT) RETURNS (ARRAY(1:2) INT);
    RETURN v;
  END Copy;

  Sum: PROC (v ARRAY(1:2) INT) RETURNS (INT);
    RETURN v(1) + v(2);
  END Sum;

  MakePoints: PROC () RETURNS (ARRAY(1:2) Point);
    DCL temp ARRAY(1:2) Point;
    temp(1).x := 1;
    temp(1).y := 2;
    temp(2).x := 3;
    temp(2).y := 4;
    RETURN temp;
  END MakePoints;

  WithHandler: PROC () RETURNS (ARRAY(1:2) INT) EXCEPTIONS (fail);
    CAUSE fail;
  END
  ON (fail):
    RESULT [7, 8];
  END WithHandler;

  a := Pair();
  ASSERT a(1) = 11;
  ASSERT a(2) = 12;
  ASSERT Sum(a) = 23;
  ASSERT Sum(Pair()) = 23;

  b := Copy(a);
  ASSERT b(1) = 11;
  b(2) := 20;
  ASSERT a(2) = 12;

  a := WithHandler();
  ASSERT a(1) = 7;
  ASSERT a(2) = 8;

  points := MakePoints();
  ASSERT points(1).x = 1;
  ASSERT points(1).y = 2;
  ASSERT points(2).x = 3;
  ASSERT points(2).y = 4;
  WRITELINE("array results ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearrret");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "array results ok
"
    );
}

#[test]
fn compiles_native_array_literal_arguments() {
    let dir = TempDir::new("z200c-native-array-literal-args");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearrlit"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  Sum: PROC (v ARRAY(1:2) INT) RETURNS (INT);
    RETURN v(1) + v(2);
  END Sum;

  Sum2: PROC (v ARRAY(1:2, 1:2) INT) RETURNS (INT);
    RETURN v(1,1) + v(1,2) + v(2,1) + v(2,2);
  END Sum2;

  StrSum: PROC (v ARRAY(1:2) CHARS) RETURNS (INT);
    IF v(1) = "a" AND v(2) = "b" THEN
      RETURN 1;
    FI;
    RETURN 0;
  END StrSum;

  ASSERT Sum([9, 10]) = 19;
  ASSERT Sum2([[1,2],[3,4]]) = 10;
  ASSERT StrSum(["a", "b"]) = 1;
  WRITELINE("array literal args ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearrlit");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "array literal args ok
"
    );
}

#[test]
fn compiles_native_composite_references() {
    let dir = TempDir::new("z200c-native-composite-refs");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativerefcomp"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  NEWMODE P = STRUCT (x INT, y INT);
  DCL p P;
  DCL a ARRAY(1:2) INT;
  DCL grid ARRAY(1:2, 1:2) P;
  DCL r REF INT;
  DCL rp REF P;
  DCL ra REF ARRAY(1:2) INT;
  DCL rg REF ARRAY(1:2, 1:2) P;
  DCL temp P;

  Sum: PROC (v ARRAY(1:2) INT) RETURNS (INT);
    RETURN v(1) + v(2);
  END Sum;

  Fill: PROC (v ARRAY(1:2) INT OUT);
    v(1) := 10;
    v(2) := 20;
  END Fill;

  rp := ->p;
  rp->.x := 8;
  rp->.y := 9;
  ASSERT p.x = 8;
  ASSERT p.y = 9;

  r := ->p.x;
  r-> := 7;
  ASSERT p.x = 7;

  a := [1, 2];
  r := ->a(2);
  r-> := 9;
  ASSERT a(2) = 9;

  rp := ->grid(2, 1);
  rp->.x := 4;
  rp->.y := 5;
  ASSERT grid(2, 1).x = 4;
  ASSERT grid(2, 1).y = 5;

  ra := ->a;
  ASSERT ra->(2) = 9;
  ra->(1) := 7;
  ASSERT a(1) = 7;
  ASSERT Sum(ra->) = 16;
  Fill(ra->);
  ASSERT a(1) = 10;
  ASSERT a(2) = 20;

  rg := ->grid;
  temp := rg->(2, 1);
  ASSERT temp.x = 4;
  temp.x := 6;
  rg->(2, 1) := temp;
  ASSERT grid(2, 1).x = 6;
  WRITELINE("composite refs ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativerefcomp");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "composite refs ok
"
    );
}

#[test]
fn compiles_native_cross_module_grants() {
    let dir = TempDir::new("z200c-native-grants");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativegrants"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Counter: SPEC MODULE
  GRANT value, Bump;
  DCL value INT;
  Bump: PROC () END;
END Counter;

Counter: MODULE
  DCL value INT INIT := 40;

  Bump: PROC ();
    value := value + 1;
  END Bump;
END Counter;

Driver: MODULE
  SEIZE value, Bump;
  ASSERT value = 40;
  Bump();
  ASSERT value = 41;
  Bump();
  ASSERT value = 42;
  WRITELINE("cross module ok");
END Driver;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativegrants");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "cross module ok
"
    );
}

#[test]
fn compiles_native_seized_modes() {
    let dir = TempDir::new("z200c-native-seized-modes");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativemodes"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Types: MODULE
  NEWMODE Point = STRUCT (x INT, y INT);
  GRANT Point;
END Types;

Use: MODULE
  SEIZE Point;
  DCL p Point;

  F: PROC (v Point) RETURNS (INT);
    RETURN v.x + v.y;
  END F;

  p.x := 1;
  p.y := 2;
  ASSERT F(p) = 3;
  WRITELINE("seized modes ok");
END Use;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativemodes");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "seized modes ok
"
    );
}

#[test]
fn compiles_native_imported_modes_in_declarations() {
    let dir = TempDir::new("z200c-native-imported-modes");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeimportedmodes"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Types: MODULE
  NEWMODE Point = STRUCT (x INT, y INT);
  NEWMODE Color = SET(red, green, blue);
  GRANT Point, Color;
END Types;

Use: MODULE
  SEIZE Point, Color;
  DCL p Point;
  DCL c Color;
  DCL grid ARRAY(1:2) Point;
  NEWMODE Line = STRUCT (a Point, b Point);
  DCL l Line;

  F: PROC (v Point) RETURNS (INT);
    RETURN v.x + v.y;
  END F;

  p.x := 1;
  p.y := 2;
  c := red;
  ASSERT c = red;
  ASSERT c /= blue;
  c := blue;

  grid(1).x := 3;
  grid(2).y := 4;
  l.a := p;
  l.b := grid(1);
  ASSERT F(l.a) = 3;
  ASSERT l.b.x = 3;
  ASSERT grid(2).y = 4;
  WRITELINE("imported modes ok");
END Use;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeimportedmodes");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "imported modes ok
"
    );
}

#[test]
fn compiles_native_forward_seized_modes() {
    let dir = TempDir::new("z200c-native-forward-modes");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeforwardmodes"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Use: MODULE
  SEIZE Point;
  DCL p Point;
  NEWMODE Line = STRUCT (a Point, b Point);
  DCL l Line;
  p.x := 1;
  l.a := p;
  ASSERT l.a.x = 1;
  WRITELINE("forward mode ok");
END Use;

Types: MODULE
  NEWMODE Point = STRUCT (x INT, y INT);
  GRANT Point;
END Types;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeforwardmodes");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "forward mode ok
"
    );
}

#[test]
fn compiles_native_text_phase_one() {
    let dir = TempDir::new("z200c-native-text");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativetext"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  NEWMODE Holder = STRUCT (name TEXT(10), n INT);
  DCL t TEXT(10);
  DCL b BOOL;
  DCL c CHAR;
  DCL n INT;
  DCL h Holder;

  Fill: PROC (x TEXT(10) OUT);
    WRITETEXT(x, "abc");
  END Fill;

  Check: PROC (x TEXT(10)) EXCEPTIONS (ASSERTFAIL);
    ASSERT x = "abc";
  END Check;

  SETTEXTINDEX(t, 2);
  ASSERT GETTEXTINDEX(t) = 2;
  WRITETEXT(t, "a");
  ASSERT t = "  a";
  ASSERT GETTEXTINDEX(t) = 0;

  SETTEXTINDEX(t, 3);
  WRITETEXT(t, "%C", 42);
  ASSERT t = "  a42";

  t := "";
  WRITETEXT(t, "%C:%C:%C", TRUE, "Z", 7);
  ASSERT t = "TRUE:Z:7";
  READTEXT(t, "%C:%C:%C", b, c, n);
  ASSERT b = TRUE;
  ASSERT c = 'Z';
  ASSERT n = 7;

  t := "   42";
  READTEXT(t, "%C", n);
  ASSERT n = 42;

  h.name := "hi";
  ASSERT h.name = "hi";

  t := "";
  Fill(t);
  ASSERT t = "abc";
  Check(t);

  BEGIN
    DCL local TEXT(10) INIT := "hi";
    ASSERT local = "hi";
    local := "bye";
    ASSERT local = "bye";
  END;

  BEGIN
    READTEXT(t, "no");
  END
  ON (TEXTFAIL):
    WRITELINE("textfail caught");
  END;
  WRITELINE("text ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativetext");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "textfail caught
text ok
"
    );
}

#[test]
fn native_text_failures_are_clean() {
    let cases = [
        (
            "length",
            r#"
M: MODULE
  DCL t TEXT(3);
  WRITETEXT(t, "abcd");
END M;
"#,
            "TEXTFAIL: text record exceeds the text length",
        ),
        (
            "index",
            r#"
M: MODULE
  DCL t TEXT(3);
  SETTEXTINDEX(t, 4);
END M;
"#,
            "TEXTFAIL: text index is outside the text length",
        ),
    ];
    for (name, source, expected) in cases {
        let dir = TempDir::new(&format!("z200c-native-text-{name}"));
        let package = format!("nativetext{name}");
        std::fs::write(
            dir.join("Chill.toml"),
            format!(
                r#"
[package]
name = "{package}"

[build]
entry = "src/main.ch"
"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src/main.ch"), source).unwrap();
        let output = z200c().arg("compile").arg(&dir).output().unwrap();
        assert!(
            output.status.success(),
            "compile {name}: stdout: {}
stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let binary = dir.join(format!("target/z200c/{package}"));
        let run = std::process::Command::new(binary).output().unwrap();
        assert!(!run.status.success());
        assert!(
            String::from_utf8_lossy(&run.stderr).contains(expected),
            "stderr: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
}

#[test]
fn compiles_native_integer_widths() {
    let dir = TempDir::new("z200c-native-widths");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativewidths"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:2) SHORT_INT;
  DCL s SHORT_INT;
  DCL i INT;

  F: PROC () RETURNS (SHORT_INT);
    RETURN 32767;
  END F;

  a := [32766, 32767];
  ASSERT a(2) = 32767;
  s := F();
  ASSERT s = 32767;
  i := 2147483647;
  ASSERT i = 2147483647;

  BEGIN
    DCL local SHORT_INT INIT := 123;
    ASSERT local = 123;
  END;
  WRITELINE("widths ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativewidths");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "widths ok
"
    );
}

#[test]
fn native_integer_width_failures_are_clean() {
    let cases = [
        (
            "short",
            r#"
M: MODULE
  DCL s SHORT_INT INIT := 32766;
  s := s + 1;
  s := s + 1;
END M;
"#,
            "OVERFLOW: integer value does not fit its mode",
        ),
        (
            "int",
            r#"
M: MODULE
  DCL i INT INIT := 2147483646;
  i := i + 1;
  i := i + 1;
END M;
"#,
            "OVERFLOW: integer value does not fit its mode",
        ),
        (
            "conversion",
            r#"
M: MODULE
  DCL s SHORT_INT;
  DCL l LONG_INT INIT := 40000;
  s := SHORT_INT(l);
END M;
"#,
            "OVERFLOW: integer value does not fit its mode",
        ),
    ];
    for (name, source, expected) in cases {
        let dir = TempDir::new(&format!("z200c-native-width-{name}"));
        let package = format!("nativewidth{name}");
        std::fs::write(
            dir.join("Chill.toml"),
            format!(
                r#"
[package]
name = "{package}"

[build]
entry = "src/main.ch"
"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src/main.ch"), source).unwrap();
        let output = z200c().arg("compile").arg(&dir).output().unwrap();
        assert!(
            output.status.success(),
            "compile {name}: stdout: {}
stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let binary = dir.join(format!("target/z200c/{package}"));
        let run = std::process::Command::new(binary).output().unwrap();
        assert!(!run.status.success());
        assert!(
            String::from_utf8_lossy(&run.stderr).contains(expected),
            "stderr: {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
}

#[test]
fn compiles_native_array_slices() {
    let dir = TempDir::new("z200c-native-slices");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeslices"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY (1:5) INT;
  DCL b ARRAY (1:3) INT;
  DCL c ARRAY (1:2) INT;
  DCL p ARRAY (1:3) INT INIT := [7, 8, 9];
  DCL s ARRAY (1:3) CHARS;
  DCL first INT INIT := 2;
  DCL upper INT INIT := 4;
  DCL size INT INIT := 2;

  Sum: PROC (v ARRAY (1:3) INT) RETURNS (INT);
    RETURN v(1) + v(2) + v(3);
  END Sum;

  a := [10, 20, 30, 40, 50];

  b := a(2:4);
  ASSERT b(1) = 20;
  ASSERT b(3) = 40;

  c := a(2 UP 2);
  ASSERT c(1) = 20;
  ASSERT c(2) = 30;

  ASSERT a(2:3) = [20, 30];
  ASSERT a(2:4) = b;
  ASSERT a(2:4) /= a(1:3);
  ASSERT Sum(a(first:upper)) = 90;
  ASSERT a(first:upper) = b;

  a(2:3) := p(1:2);
  ASSERT a(2) = 7;
  ASSERT a(3) = 8;
  a(4 UP 2) := p(1:2);
  ASSERT a(4) = 7;
  ASSERT a(5) = 8;

  a(first:upper) := p;
  ASSERT a(2) = 7;
  ASSERT a(3) = 8;
  ASSERT a(4) = 9;
  a(first UP size) := p(2:3);
  ASSERT a(2) = 8;
  ASSERT a(3) = 9;

  s := ["a", "b", "c"];
  s(2:3) := ["x", "y"];
  ASSERT s(1) = "a";
  ASSERT s(2) = "x";
  ASSERT s(3) = "y";
  WRITELINE("slices ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeslices");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "slices ok
"
    );
}

#[test]
fn native_slice_length_mismatch_fails_cleanly() {
    let dir = TempDir::new("z200c-native-slice-length");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeslicelen"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY (1:5) INT;
  DCL first INT INIT := 2;
  DCL upper INT INIT := 3;

  Sum: PROC (v ARRAY (1:3) INT) RETURNS (INT);
    RETURN v(1) + v(2) + v(3);
  END Sum;

  a := [1,2,3,4,5];
  ASSERT Sum(a(first:upper)) = 0;
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeslicelen");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(!run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("RANGEFAIL: array slice length mismatch"),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn compiles_native_multidimensional_slices() {
    let dir = TempDir::new("z200c-native-multislice");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativemslice"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3, 1:3) INT;
  DCL b ARRAY(2:3, 2:3) INT;
  DCL c ARRAY(1:2, 2:3) INT;

  Sum2: PROC (v ARRAY(2:3, 2:3) INT) RETURNS (INT);
    RETURN v(2,2) + v(2,3) + v(3,2) + v(3,3);
  END Sum2;

  a := [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
  b := a(2:3, 2:3);
  ASSERT b(2, 2) = 5;
  ASSERT b(2, 3) = 6;
  ASSERT b(3, 2) = 8;
  ASSERT b(3, 3) = 9;

  c := a(1:2, 2 UP 2);
  ASSERT c(1, 2) = 2;
  ASSERT c(2, 3) = 6;

  a(2:3, 2:3) := [[20, 21], [22, 23]];
  ASSERT a(2, 2) = 20;
  ASSERT a(2, 3) = 21;
  ASSERT a(3, 2) = 22;
  ASSERT a(3, 3) = 23;
  ASSERT a(1, 1) = 1;
  ASSERT Sum2(a(2:3, 2:3)) = 86;
  ASSERT a(2:3, 2:3) = a(2:3, 2:3);
  WRITELINE("multidim slices ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativemslice");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "multidim slices ok
"
    );
}

#[test]
fn compiles_native_text_formatting() {
    let dir = TempDir::new("z200c-native-text-format");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativetextfmt"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL t TEXT(40);

  t := "";
  WRITETEXT(t, "%C5", 42);
  ASSERT t = "   42";

  t := "";
  WRITETEXT(t, "%CL5", 42);
  ASSERT t = "42   ";

  t := "";
  WRITETEXT(t, "%CE3", 12345);
  ASSERT t = "***";

  t := "";
  WRITETEXT(t, "%B", 10);
  ASSERT t = "1010";

  t := "";
  WRITETEXT(t, "%O", 15);
  ASSERT t = "17";

  t := "";
  WRITETEXT(t, "%H", 255);
  ASSERT t = "ff";

  t := "";
  WRITETEXT(t, "%B5:%C:%H5", 3, "Z", 31);
  ASSERT t = "   11:Z:   1f";

  WRITELINE("text formats ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativetextfmt");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "text formats ok
"
    );
}

#[test]
fn native_refcounted_payloads_are_reclaimed() {
    let dir = TempDir::new("z200c-native-rc");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativrc"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL i INT;
  DCL gs CHARS INIT := "g";
  DCL gp POWERSET INT INIT := [1];
  DCL extra POWERSET INT INIT := [2];
  DCL gt TEXT(10) INIT := "t";
  NEWMODE Holder = STRUCT (s CHARS, p POWERSET INT, t TEXT(10));

  F: PROC (s CHARS) RETURNS (CHARS);
    DCL local CHARS;
    local := s // "!";
    RETURN local;
  END F;

  G: PROC (s CHARS OUT);
    s := "g";
  END G;

  H: PROC (h Holder) RETURNS (Holder);
    DCL copy Holder;
    copy := h;
    copy.s := copy.s // "?";
    RETURN copy;
  END H;

  Make: PROC () RETURNS (ARRAY (1:2) CHARS);
    RETURN ["a", "b"];
  END Make;

  MakeP: PROC () RETURNS (POWERSET INT);
    RETURN [1, 2];
  END MakeP;

  MakeT: PROC () RETURNS (TEXT(10));
    DCL t TEXT(10);
    t := "t";
    RETURN t;
  END MakeT;

  gs := gs // "!";
  gp := gp + extra;
  SETTEXTINDEX(gt, 1);
  WRITETEXT(gt, "x");

  DO FOR i := 1 TO 25;
    BEGIN
      DCL a ARRAY(1:2) CHARS;
      DCL s CHARS;
      DCL p POWERSET INT;
      DCL t TEXT(10);
      DCL h Holder;
      DCL h2 Holder;

      a := ["a", "b"];
      s := F(a(1));
      ASSERT s = "a!";
      a(2) := s;
      ASSERT a(2) = "a!";
      G(s);
      ASSERT s = "g";

      p := [1];
      p := p + extra;
      ASSERT 2 IN p;

      t := "q";
      SETTEXTINDEX(t, 1);
      WRITETEXT(t, "%C", 3);
      ASSERT t = "q3";

      h.s := "h";
      h.p := [1];
      h.t := "t";
      h2 := H(h);
      ASSERT h2.s = "h?";
      ASSERT 1 IN h2.p;
      ASSERT h2.t = "t";

      a := Make();
      ASSERT a(2) = "b";
      p := MakeP();
      ASSERT 2 IN p;
      t := MakeT();
      ASSERT t = "t";
    END;
  OD;
  WRITELINE("rc ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativrc");
    let run = std::process::Command::new(binary)
        .env("Z200C_RC_STATS", "1")
        .output()
        .unwrap();
    assert!(run.status.success());
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("rc ok") && stdout.contains("live-payloads: 0"),
        "stdout: {stdout}"
    );
}

#[test]
fn compiles_native_chars_text_transfer() {
    let dir = TempDir::new("z200c-native-chars-text");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativetextchars"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL s CHARS INIT := "hello";
  DCL v CHARS(10) VARYING;
  DCL n INT;

  WRITETEXT(s, 1, "i");
  ASSERT s = "hillo";
  READTEXT(s, 1, "i");

  v := "abc";
  WRITETEXT(v, 1, "X");
  ASSERT v = "aXc";

  WRITETEXT(v, "%C5", 42);
  ASSERT v = "   42";
  READTEXT(v, "%C", n);
  ASSERT n = 42;

  BEGIN
    DCL small CHARS(3) VARYING;
    small := "abc";
    WRITETEXT(small, "%C", 1234);
  END
  ON (TEXTFAIL):
    WRITELINE("chars textfail");
  END;
  WRITELINE("chars text ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativetextchars");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "chars textfail
chars text ok
"
    );
}

#[test]
fn compiles_native_text_format_fixture() {
    let dir = TempDir::new("z200c-native-text-format-fixture");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativetextformat"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    let source = std::fs::read_to_string(format!(
        "{}/tests/run_ok/text_format_io.ch",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), source).unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativetextformat");
    let run = std::process::Command::new(binary)
        .env("Z200C_RC_STATS", "1")
        .output()
        .unwrap();
    assert!(run.status.success());
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("live-payloads: 0"), "stdout: {stdout}");
}

#[test]
fn compiles_native_association_fixtures() {
    for name in [
        "associations",
        "association_indexed",
        "access_attributes",
        "text_connection",
        "text_indexed_access",
        "text_sub_locations",
    ] {
        let dir = TempDir::new(&format!("z200c-native-{name}"));
        let package = format!("native{}", name.replace('_', ""));
        std::fs::write(
            dir.join("Chill.toml"),
            format!(
                r#"
[package]
name = "{package}"

[build]
entry = "src/main.ch"
"#
            ),
        )
        .unwrap();
        let source = std::fs::read_to_string(format!(
            "{}/tests/run_ok/{name}.ch",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        std::fs::write(dir.join("src/main.ch"), source).unwrap();
        let output = z200c().arg("compile").arg(&dir).output().unwrap();
        assert!(
            output.status.success(),
            "{name}: stdout: {}
stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let binary = dir.join(format!("target/z200c/{package}"));
        let run = std::process::Command::new(binary)
            .env("Z200C_RC_STATS", "1")
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{name}: stdout: {}
stderr: {}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let stdout = String::from_utf8_lossy(&run.stdout);
        assert!(stdout.contains("live-payloads: 0"), "{name}: {stdout}");
    }
}

#[test]
fn native_unwind_cleanup_reclaims_payloads() {
    let dir = TempDir::new("z200c-native-unwind");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeunwind"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL i INT;
  DCL extra POWERSET INT INIT := [1];

  Outer: DO FOR i := 1 TO 5;
    BEGIN
      DCL s CHARS;
      DCL p POWERSET INT;
      DCL t TEXT(10);
      s := "a" // "b";
      p := extra;
      t := "x";
      EXIT Outer;
    END;
  OD;

  DO FOR i := 1 TO 5;
    BEGIN
      DCL s2 CHARS;
      s2 := "c" // "d";
      IF i = 2 THEN
        EXIT;
      FI;
    END;
  OD;

  DO FOR i := 1 TO 5;
    BEGIN
      DCL s3 CHARS;
      DCL p3 POWERSET INT;
      s3 := "e" // "f";
      p3 := extra;
      BEGIN
        DCL s4 CHARS;
        s4 := "g" // "h";
        CAUSE boom;
      END
      ON (boom):
        WRITELINE("caught");
      END;
    END;
  OD;

  WRITELINE("unwind ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeunwind");
    let run = std::process::Command::new(binary)
        .env("Z200C_RC_STATS", "1")
        .output()
        .unwrap();
    assert!(run.status.success());
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("unwind ok") && stdout.contains("live-payloads: 0"),
        "stdout: {stdout}"
    );
}

#[test]
fn compiles_native_float_fixture() {
    let dir = TempDir::new("z200c-native-float");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativefloat"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    let source = std::fs::read_to_string(format!(
        "{}/tests/run_ok/floats.ch",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), source).unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativefloat");
    let run = std::process::Command::new(binary)
        .env("Z200C_RC_STATS", "1")
        .output()
        .unwrap();
    assert!(run.status.success());
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("live-payloads: 0"), "stdout: {stdout}");
}

#[test]
fn native_dereferenced_text_sub_locations() {
    let dir = TempDir::new("z200c-native-textsub-deref");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativetextsubderef"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL t1 TEXT(20);
  DCL t2 TEXT(20);
  DCL s CHARS(20) VARYING INIT := "hello";
  DCL r REF CHARS(20) VARYING;
  DCL aref REF ACCESS CHARS;
  DCL assoc ASSOCIATION;

  SETTEXTRECORD(t1, s);
  ASSERT t1 = "hello";
  s := "world";
  ASSERT t1 = "world";

  r := GETTEXTRECORD(t1);
  SETTEXTRECORD(t2, r->);
  ASSERT t2 = "world";
  r-> := "ref!";
  ASSERT t1 = "ref!";
  ASSERT t2 = "ref!";
  ASSERT s = "ref!";

  ASSOCIATE(assoc, "/tmp/z200c-textsub-deref-test.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);
  aref := GETTEXTACCESS(t1);
  CONNECT(aref->, assoc, WRITEONLY);
  ASSERT GETUSAGE(aref->) = WRITEONLY;
  WRITETEXT(t1, "file");
  DISCONNECT(aref->);
  DELETE(assoc);
  DISSOCIATE(assoc);
  WRITELINE("text sub deref ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativetextsubderef");
    let run = std::process::Command::new(binary)
        .env("Z200C_RC_STATS", "1")
        .output()
        .unwrap();
    assert!(run.status.success());
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("text sub deref ok") && stdout.contains("live-payloads: 0"),
        "stdout: {stdout}"
    );
}

#[test]
fn compiles_native_association_arrays() {
    let dir = TempDir::new("z200c-native-assoc-array");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeassocarray"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL assocs ARRAY(1:2) ASSOCIATION;
  DCL copy ARRAY(1:2) ASSOCIATION;

  copy := assocs;
  WRITELINE("association arrays ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}
stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeassocarray");
    let run = std::process::Command::new(binary)
        .env("Z200C_RC_STATS", "1")
        .output()
        .unwrap();
    assert!(run.status.success());
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("association arrays ok") && stdout.contains("live-payloads: 0"),
        "stdout: {stdout}"
    );
}

#[test]
fn native_run_ok_fixture_gate() {
    let manifest = std::fs::read_to_string(format!(
        "{}/tests/native_run_ok.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    for line in manifest.lines() {
        let name = line.trim();
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        let source_path = format!("{}/tests/run_ok/{name}.ch", env!("CARGO_MANIFEST_DIR"));
        assert!(
            std::path::Path::new(&source_path).exists(),
            "missing native conformance fixture `{name}`"
        );
        let dir = TempDir::new(&format!("z200c-native-gate-{name}"));
        let package = format!("nativegate{}", name.replace('_', ""));
        std::fs::write(
            dir.join("Chill.toml"),
            format!(
                r#"
[package]
name = "{package}"

[build]
entry = "src/main.ch"
"#
            ),
        )
        .unwrap();
        let source = std::fs::read_to_string(source_path).unwrap();
        std::fs::write(dir.join("src/main.ch"), source).unwrap();
        let output = z200c().arg("compile").arg(&dir).output().unwrap();
        assert!(
            output.status.success(),
            "{name}: compile failed
stdout: {}
stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let binary = dir.join(format!("target/z200c/{package}"));
        let run = std::process::Command::new(binary)
            .env("Z200C_RC_STATS", "1")
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{name}: run failed
stdout: {}
stderr: {}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let stdout = String::from_utf8_lossy(&run.stdout);
        assert!(
            stdout.contains("live-payloads: 0"),
            "{name}: expected zero live payloads, got:
{stdout}"
        );
    }
}

#[test]
fn native_backend_rejects_unsupported_arrays() {
    let dir = TempDir::new("z200c-native-array");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearray"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL values ARRAY(1:2) REF INT;
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not supported") || stderr.contains("unsupported mode"),
        "stderr: {stderr}"
    );
}

#[test]
fn run_directory_dependency_succeeds() {
    let output = z200c().arg("run").arg(example("app")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn grant_writes_grant_file() {
    let _guard = example_grant_lock().lock().unwrap();
    let output = z200c()
        .arg("grant")
        .arg(example("modules"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let path = format!(
        "{}/examples/modules/target/z200c/modules.grt.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("name = \"Bump\""));
    assert!(text.contains("name = \"value\""));
    assert!(text.contains("mode = \"INT\""));
}

#[test]
fn checks_against_dependency_grant_file() {
    let dir = TempDir::new("z200c-dep");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("lib.grt.toml"),
        r#"
schema = 4
package = "lib"

[[exports]]
name = "x"
kind = "variable"
mode = "INT"

[[exports]]
name = "F"
kind = "procedure"
result = "INT"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

[dependencies]
lib = "lib.grt.toml"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE x, F;
  ASSERT x = 1;
  ASSERT F() = 2;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let run = z200c().arg("run").arg(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.stderr).contains("not linked"));
}

#[test]
fn run_allows_unused_dependency() {
    let dir = TempDir::new("z200c-unused-dep");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("lib.grt.toml"),
        r#"
schema = 4
package = "lib"

[[exports]]
name = "unused"
kind = "variable"
mode = "INT"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

[dependencies]
lib = "lib.grt.toml"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Client: MODULE
  ASSERT TRUE;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn check_reports_multiple_semantic_errors() {
    let dir = TempDir::new("z200c-multi");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "multi"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a SHORT_INT;
  DCL b LONG_INT;
  a := b;
  a := b;
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.matches("error[E0404]").count(), 2);
}

#[test]
fn run_links_grant_file_with_source_metadata() {
    let grant = z200c()
        .arg("grant")
        .arg(example("liblocal"))
        .output()
        .unwrap();
    assert!(
        grant.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&grant.stdout),
        String::from_utf8_lossy(&grant.stderr)
    );

    let grant_path = format!(
        "{}/examples/liblocal/target/z200c/liblocal.grt.toml",
        env!("CARGO_MANIFEST_DIR")
    );

    let dir = TempDir::new("z200c-grant-run");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

[dependencies]
liblocal = "{grant_path}"
"#
        ),
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE value;
  ASSERT value = 10;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn check_resolves_qualified_grant_exports() {
    let _guard = example_grant_lock().lock().unwrap();
    let grant = z200c()
        .arg("grant")
        .arg(example("modules"))
        .output()
        .unwrap();
    assert!(
        grant.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&grant.stdout),
        String::from_utf8_lossy(&grant.stderr)
    );

    let grant_path = format!(
        "{}/examples/modules/target/z200c/modules.grt.toml",
        env!("CARGO_MANIFEST_DIR")
    );

    let dir = TempDir::new("z200c-grant-qualified");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

[dependencies]
modules = "{grant_path}"
"#
        ),
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE Counter!value;
  ASSERT value = 40;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn duplicate_module_names_across_packages_are_allowed() {
    let base = TempDir::new("z200c-packages");

    for (name, variable, value) in [("one", "a", 1), ("two", "b", 2)] {
        let dir = base.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Chill.toml"),
            format!(
                r#"
[package]
name = "{name}"

[build]
entry = "src/main.ch"
"#
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.ch"),
            format!(
                r#"
Library: MODULE
  GRANT {variable};
  DCL {variable} INT INIT := {value};
END Library;
"#
            ),
        )
        .unwrap();
    }

    let client = base.join("client");
    std::fs::create_dir_all(client.join("src")).unwrap();
    std::fs::write(
        client.join("Chill.toml"),
        r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

[dependencies]
one = "../one"
two = "../two"
"#,
    )
    .unwrap();
    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE a, b;
  ASSERT a = 1;
  ASSERT b = 2;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&client).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn runs_with_structure_mode_from_grant_file() {
    let base = TempDir::new("z200c-struct-grant");
    let lib = base.join("libstruct");
    std::fs::create_dir_all(lib.join("src")).unwrap();

    std::fs::write(
        lib.join("Chill.toml"),
        r#"
[package]
name = "structlib"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        lib.join("src/main.ch"),
        r#"
A: MODULE
  NEWMODE S = STRUCT(x INT, hidden BOOL);
  GRANT S FORBID (hidden);
END A;
"#,
    )
    .unwrap();

    let grant = z200c().arg("grant").arg(&lib).output().unwrap();
    assert!(
        grant.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&grant.stdout),
        String::from_utf8_lossy(&grant.stderr)
    );
    let grant_path = lib.join("target/z200c/structlib.grt.toml");
    let text = std::fs::read_to_string(&grant_path).unwrap();
    assert!(text.contains("schema = 4"));
    assert!(text.contains("[[modes]]"));
    assert!(text.contains("mode = \"structlib!A!S\""));

    let client = base.join("client");
    std::fs::create_dir_all(client.join("src")).unwrap();
    std::fs::write(
        client.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

[dependencies]
structlib = "{}"
"#,
            grant_path.display()
        ),
    )
    .unwrap();
    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE S;
  DCL s S;
  s.x := 7;
  ASSERT s.x = 7;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&client).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE S;
  DCL s S;
  s.hidden := TRUE;
END Client;
"#,
    )
    .unwrap();
    let output = z200c().arg("check").arg(&client).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FORBIDden"), "stderr: {stderr}");
}

#[test]
fn cross_package_set_reference_and_moreta_components() {
    let base = TempDir::new("z200c-grant-modes");
    let lib = base.join("libmodes");
    std::fs::create_dir_all(lib.join("src")).unwrap();

    std::fs::write(
        lib.join("Chill.toml"),
        r#"
[package]
name = "libmodes"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        lib.join("src/main.ch"),
        r#"
Lib: MODULE
  GRANT S, P, shared;

  NEWMODE S = SET(low, high);

  NEWMODE P = MODULE SPEC
    GRANT value, Set;
    DCL value INT;
    Set: PROC (n INT) END;
    Reset: PROC () END;
  END P;
  NEWMODE P = MODULE BODY
    DCL value INT INIT := 2;
    Set: PROC (n INT);
      value := n;
    END Set;
    Reset: PROC ();
      value := 0;
    END Reset;
  END P;

  DCL shared S INIT := high;
END Lib;
"#,
    )
    .unwrap();

    let grant = z200c().arg("grant").arg(&lib).output().unwrap();
    assert!(
        grant.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&grant.stdout),
        String::from_utf8_lossy(&grant.stderr)
    );

    let client = base.join("client");
    std::fs::create_dir_all(client.join("src")).unwrap();
    std::fs::write(
        client.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

[dependencies]
libmodes = "{}"
"#,
            lib.display()
        ),
    )
    .unwrap();
    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE S, P, shared;
  DCL p P;
  DCL s S;
  DCL r REF P;

  p.Set(7);
  ASSERT p.value = 7;
  s := shared;
  r := NULL;
  ASSERT r = NULL;
  ASSERT s = shared;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&client).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE P;
  DCL p P;
  p.Reset();
END Client;
"#,
    )
    .unwrap();
    let output = z200c().arg("check").arg(&client).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not accessible") || stderr.contains("not public"),
        "stderr: {stderr}"
    );
}

#[test]
fn runs_single_file_script_without_manifest() {
    let dir = TempDir::new("z200c-script");
    let script = dir.join("hello.ch");
    std::fs::write(
        &script,
        r#"
Hello: MODULE
  DCL x INT INIT := 3;
  ASSERT x = 3;
END Hello;
"#,
    )
    .unwrap();
    // A sibling source must not be loaded in script mode.
    std::fs::write(dir.join("broken.ch"), "this is not CHILL").unwrap();

    let check = z200c().arg("check").arg(&script).output().unwrap();
    assert!(
        check.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("checked hello"));

    let run = z200c().arg("run").arg(&script).output().unwrap();
    assert!(
        run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn write_and_writeline_text_io() {
    let dir = TempDir::new("z200-text-out");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Chill.toml"), "").unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  WRITE("a");
  WRITE("b");
  WRITELINE("c");
  WRITELINE();
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "abc\n\n");
}

#[test]
fn readline_text_io() {
    let dir = TempDir::new("z200-text-in");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Chill.toml"), "").unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL line CHARS;
  line := READLINE();
  ASSERT line = "hello";
END M;
"#,
    )
    .unwrap();

    let mut child = z200c()
        .arg("run")
        .arg(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"hello\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn minimal_manifest_defaults_name_entry_and_sources() {
    let dir = TempDir::new("z200-minimal");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Chill.toml"), "").unwrap();
    std::fs::write(
        dir.join("src/lib.ch"),
        "Lib: MODULE\n  GRANT x;\n  DCL x INT INIT := 1;\nEND Lib;\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        "Main: MODULE\n  SEIZE x;\n  ASSERT x = 1;\nEND Main;\n",
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn remote_piece_must_exist() {
    let dir = TempDir::new("z200-piece-missing");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), "REMOTE \"pieces/missing.ch\";\n").unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E0700"), "stderr: {stderr}");
    assert!(stderr.contains("piece file not found"), "stderr: {stderr}");
}

#[test]
fn remote_piece_is_expanded() {
    let dir = TempDir::new("z200-piece-present");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(dir.join("pieces/counter.ch"), "M: MODULE END M;\n").unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"

[pieces]
counter = "pieces/counter.ch"
"#,
    )
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), "REMOTE counter;\n").unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn named_remote_module_expands_and_runs() {
    let dir = TempDir::new("z200-piece-run");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/helper.ch"),
        r#"
Helper: MODULE
  ASSERT 1 = 1;
END Helper;
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"

[pieces]
helper = "pieces/helper.ch"
"#,
    )
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), "Helper: REMOTE helper;\n").unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn remote_spec_module_expands() {
    let dir = TempDir::new("z200-piece-spec");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/spec.ch"),
        r#"
Counter: SPEC MODULE
  GRANT value;
  DCL value INT;
END Counter;
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Counter: SPEC REMOTE "pieces/spec.ch";

Counter: MODULE
  DCL value INT INIT := 1;
END Counter;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn nested_spec_grants_become_virtual_grants() {
    let dir = TempDir::new("z200-nested-spec");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nested"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
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

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn spec_region_pairs_with_a_region_body() {
    let dir = TempDir::new("z200-region");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "region"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
R: SPEC REGION
  GRANT x;
  DCL x INT;
END R;

R: REGION
  DCL x INT INIT := 1;
END R;

Client: MODULE
  SEIZE x;
  ASSERT x = 1;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn remote_region_expands() {
    let dir = TempDir::new("z200-region-remote");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/region.ch"),
        r#"
R: REGION
  DCL x INT INIT := 1;
END R;
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "region"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), "REMOTE \"pieces/region.ch\";\n").unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn nested_spec_in_a_context_pairs_with_the_body_module() {
    let dir = TempDir::new("z200-ctx-spec");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nested"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
CONTEXT
  Inner: SPEC MODULE
    DCL x INT;
    GRANT x;
  END Inner;
FOR
Outer: MODULE
  Inner: MODULE
    DCL x INT INIT := 1;
  END Inner;
  ASSERT x = 1;
END Outer;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn nested_specs_are_checked_recursively() {
    let dir = TempDir::new("z200-deep-spec");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nested"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    GRANT x;
    Deep: SPEC MODULE
      DCL x INT;
      GRANT x;
    END Deep;
  END Inner;
END Outer;

Outer: MODULE
  Inner: MODULE
    Deep: MODULE
      DCL x INT INIT := 1;
    END Deep;
  END Inner;
  ASSERT x = 1;
END Outer;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn nested_spec_interface_must_match_its_body() {
    let dir = TempDir::new("z200-nested-spec-bad");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nested"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    DCL x BOOL;
  END Inner;
END Outer;

Outer: MODULE
  Inner: MODULE
    DCL x INT;
  END Inner;
END Outer;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("mode"), "stderr: {stderr}");
}

#[test]
fn remote_context_is_merged_after_the_last_context_body() {
    let dir = TempDir::new("z200-piece-ctx");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/ctx.ch"),
        r#"
CONTEXT
  DCL x INT;
FOR
CONTEXT
FOR
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 1;
END A;

CONTEXT REMOTE "pieces/ctx.ch"
  SEIZE x;
FOR
M: MODULE
  SEIZE x;
  ASSERT x = 1;
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context_module_remote_is_desugared() {
    let dir = TempDir::new("z200-piece-ctxmod");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/ctx.ch"),
        r#"
CONTEXT
  DCL x INT;
FOR
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 1;
END A;

CONTEXT MODULE REMOTE "pieces/ctx.ch";
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn remote_context_target_must_be_a_context_list() {
    let dir = TempDir::new("z200-piece-ctxkind");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(dir.join("pieces/module.ch"), "M: MODULE END M;\n").unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
CONTEXT REMOTE "pieces/module.ch" FOR
M: MODULE
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E0700"), "stderr: {stderr}");
}

#[test]
fn remote_piece_kind_mismatch_is_reported() {
    let dir = TempDir::new("z200-piece-kind");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/spec.ch"),
        "Counter: SPEC MODULE END Counter;\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        "Counter: REMOTE \"pieces/spec.ch\";\n",
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E0700"), "stderr: {stderr}");
    assert!(stderr.contains("not a module"), "stderr: {stderr}");
}

#[test]
fn version_prints_package_version() {
    let output = z200c().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_command_fails() {
    let output = z200c().arg("frobnicate").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}
