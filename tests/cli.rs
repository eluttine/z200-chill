use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

fn example_grant_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn z200c() -> Command {
    Command::new(env!("CARGO_BIN_EXE_z200c"))
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
fn run_text_output_matches_golden() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "z200c-golden-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(directory.join("src")).unwrap();
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
    let _ = std::fs::remove_dir_all(&directory);

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
fn run_hello_succeeds_without_output() {
    let output = z200c().arg("run").arg(example("hello")).output().unwrap();

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
    let dir = std::env::temp_dir().join(format!("z200c-dep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn run_allows_unused_dependency() {
    let dir = std::env::temp_dir().join(format!("z200c-unused-dep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_reports_multiple_semantic_errors() {
    let dir = std::env::temp_dir().join(format!("z200c-multi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
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

    let dir = std::env::temp_dir().join(format!("z200c-grant-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
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

    let dir = std::env::temp_dir().join(format!("z200c-grant-qualified-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn duplicate_module_names_across_packages_are_allowed() {
    let base = std::env::temp_dir().join(format!("z200c-packages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

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

    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn runs_with_structure_mode_from_grant_file() {
    let base = std::env::temp_dir().join(format!("z200c-struct-grant-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
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

    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn write_and_writeline_text_io() {
    let dir = std::env::temp_dir().join(format!("z200-text-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn readline_text_io() {
    let dir = std::env::temp_dir().join(format!("z200-text-in-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn minimal_manifest_defaults_name_entry_and_sources() {
    let dir = std::env::temp_dir().join(format!("z200-minimal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn remote_piece_must_exist() {
    let dir = std::env::temp_dir().join(format!("z200-piece-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn remote_piece_is_expanded() {
    let dir = std::env::temp_dir().join(format!("z200-piece-present-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn named_remote_module_expands_and_runs() {
    let dir = std::env::temp_dir().join(format!("z200-piece-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn remote_spec_module_expands() {
    let dir = std::env::temp_dir().join(format!("z200-piece-spec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn nested_spec_grants_become_virtual_grants() {
    let dir = std::env::temp_dir().join(format!("z200-nested-spec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn spec_region_pairs_with_a_region_body() {
    let dir = std::env::temp_dir().join(format!("z200-region-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn remote_region_expands() {
    let dir = std::env::temp_dir().join(format!("z200-region-remote-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn nested_spec_in_a_context_pairs_with_the_body_module() {
    let dir = std::env::temp_dir().join(format!("z200-ctx-spec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn nested_specs_are_checked_recursively() {
    let dir = std::env::temp_dir().join(format!("z200-deep-spec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn nested_spec_interface_must_match_its_body() {
    let dir = std::env::temp_dir().join(format!("z200-nested-spec-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn remote_context_is_merged_after_the_last_context_body() {
    let dir = std::env::temp_dir().join(format!("z200-piece-ctx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn context_module_remote_is_desugared() {
    let dir = std::env::temp_dir().join(format!("z200-piece-ctxmod-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn remote_context_target_must_be_a_context_list() {
    let dir = std::env::temp_dir().join(format!("z200-piece-ctxkind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn remote_piece_kind_mismatch_is_reported() {
    let dir = std::env::temp_dir().join(format!("z200-piece-kind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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

    std::fs::remove_dir_all(&dir).unwrap();
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
