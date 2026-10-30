//! Real-CLI sandbox integration and regression (network, ignored by default).
//!
//! Run explicitly:
//!
//! ```console
//! cd src-tauri
//! cargo test --test sandbox_real_cli -- --ignored --nocapture
//! ```
//!
//! The test creates a throwaway `HOME` / `XDG_STATE_HOME` and drives the
//! pinned `npx -y skills@1.7.4` against the small public repository
//! `mattpocock/skills`:
//!
//! install single -> install multiple -> copy install -> re-add (update /
//! copy-to-symlink) -> detach -> uninstall -> copy-only uninstall.
//!
//! After every mutation it runs the regression recipe from the acceptance
//! matrix (`list -g --json`), compares names and paths, asserts the canonical
//! store, agent directories and lock v3, and proves unmanaged entries survive.
//! Every recorded argv is scanned for the forbidden CLI forms.
//!
//! The real `~` is never read for mutations. Evidence (JSON, stderr, argv and
//! a summary) is written to `src-tauri/target/sandbox-evidence/<stamp>/` and
//! kept. The temp HOME is deleted afterwards unless `SKILLS_BOX_KEEP_SANDBOX=1`
//! (useful when running the read-only update-check test against the same
//! sandbox).

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use skills_box_lib::paths::AppPaths;
use skills_box_lib::tasks::install::validate_skill_names;
use skills_box_lib::update::UpdateService;

/// Small public repository with 38 Markdown-only skills. The names used below
/// are real upstream names (hyphenated, lowercase) so the ticket-10
/// metacharacter rejection is exercised against legitimate input.
const REPO: &str = "mattpocock/skills";
const PINNED: &str = "skills@1.7.4";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(180);

/// Names used by the sandbox flow. Kept in one place so the non-network test
/// below can prove they pass the app's validation.
const SANDBOX_SKILLS: &[&str] = &[
    "code-review",
    "implement",
    "grill-with-docs",
    "ask-matt",
    "diagnosing-bugs",
];

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_millis()
}

fn stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs();
    format!("{secs}-{}", std::process::id())
}

struct CommandResult {
    stdout: String,
    stderr: String,
}

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
    state: PathBuf,
    evidence: PathBuf,
    keep: bool,
    sequence: usize,
    command_log: Vec<String>,
}

impl Sandbox {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "skills-box-sandbox-{}-{}",
            std::process::id(),
            now_millis()
        ));
        let home = root.join("home");
        let state = root.join("state");
        let evidence = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/sandbox-evidence")
            .join(stamp());
        for dir in [
            home.clone(),
            state.clone(),
            root.join("tmp"),
            root.join("app-data"),
            evidence.clone(),
        ] {
            fs::create_dir_all(&dir)
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", dir.display()));
        }
        let sandbox = Self {
            root,
            home,
            state,
            evidence,
            keep: env::var("SKILLS_BOX_KEEP_SANDBOX").is_ok_and(|value| value == "1"),
            sequence: 0,
            command_log: Vec::new(),
        };
        sandbox.seed_unmanaged();
        println!("sandbox HOME: {}", sandbox.home.display());
        println!("evidence:     {}", sandbox.evidence.display());
        sandbox
    }

    /// Unmanaged canonical + agent entries must survive every mutation.
    fn seed_unmanaged(&self) {
        write_skill(
            &self.home.join(".agents/skills/unmanaged-keep"),
            "unmanaged-keep",
            "unmanaged canonical fixture",
        );
        write_skill(
            &self.home.join(".claude/skills/unmanaged-agent"),
            "unmanaged-agent",
            "unmanaged agent fixture",
        );
        // Agent detection mirrors the CLI: a `.qoder` directory makes qoder
        // an explicit, detected non-universal install target (cursor is a
        // universal agent and shares the canonical store).
        fs::create_dir_all(self.home.join(".qoder")).expect("create qoder home");
    }

    fn canonical(&self, name: &str) -> PathBuf {
        self.home.join(".agents/skills").join(name)
    }

    fn agent_skill(&self, agent_dir: &str, name: &str) -> PathBuf {
        self.home.join(agent_dir).join("skills").join(name)
    }

    fn lock_path(&self) -> PathBuf {
        // XDG_STATE_HOME moves the CLI lock out of ~/.agents. This is exactly
        // the regression recipe from the acceptance matrix.
        self.state.join("skills/.skill-lock.json")
    }

    fn cli(&mut self, args: &[&str], label: &str) -> CommandResult {
        self.cli_retry(args, label, 1)
    }

    /// Run in a project root. No `--dir` flag exists: the pinned CLI treats the
    /// process cwd as the project scope, exactly like the app's project tasks.
    fn cli_in(&mut self, cwd: &Path, args: &[&str], label: &str) -> CommandResult {
        self.cli_retry_in(cwd, args, label, 1)
    }

    /// Retries are only used for idempotent commands (version/list) because
    /// the sandbox npm cache may need a network fetch on a cold run.
    fn cli_retry(&mut self, args: &[&str], label: &str, attempts: usize) -> CommandResult {
        let cwd = self.root.join("tmp");
        self.cli_retry_in(&cwd, args, label, attempts)
    }

    fn cli_retry_in(
        &mut self,
        cwd: &Path,
        args: &[&str],
        label: &str,
        attempts: usize,
    ) -> CommandResult {
        for attempt in 0..attempts {
            let attempt_label = if attempt == 0 {
                label.to_string()
            } else {
                format!("{label}-retry{attempt}")
            };
            match self.try_cli_in(cwd, args, &attempt_label) {
                Ok(result) => return result,
                Err(message) if attempt + 1 < attempts => {
                    eprintln!(
                        "transient sandbox failure (attempt {}/{}): {message}; retrying",
                        attempt + 1,
                        attempts
                    );
                    std::thread::sleep(Duration::from_secs(2));
                }
                Err(message) => panic!("{message}"),
            }
        }
        unreachable!("attempts is at least one")
    }

    fn try_cli_in(
        &mut self,
        cwd: &Path,
        args: &[&str],
        label: &str,
    ) -> Result<CommandResult, String> {
        if let Some(reason) = forbidden_argv(args) {
            panic!("refusing to run a forbidden CLI form ({reason}): {args:?}");
        }
        self.sequence += 1;
        let sequence = self.sequence;
        let mut argv = vec!["-y".to_string(), PINNED.to_string()];
        argv.extend(args.iter().map(|arg| arg.to_string()));

        let display = format!("(cwd {}) npx {}", cwd.display(), argv.join(" "));
        println!("$ {display}");
        let started = Instant::now();
        let result = run_npx(&self.root, cwd, &argv);
        let duration = started.elapsed();

        let stdout_path = self
            .evidence
            .join(format!("{sequence:02}-{label}.stdout.txt"));
        let stderr_path = self
            .evidence
            .join(format!("{sequence:02}-{label}.stderr.txt"));
        fs::write(&stdout_path, &result.stdout).expect("write stdout evidence");
        fs::write(&stderr_path, &result.stderr).expect("write stderr evidence");
        let log_line = format!(
            "$ {display}\n# exit={:?} duration_ms={}\n",
            result.status.code(),
            duration.as_millis()
        );
        self.command_log.push(display.clone());
        append(&self.evidence.join("commands.log"), &log_line);

        if !result.status.success() {
            return Err(format!(
                "`{display}` failed with {:?}\nstderr tail:\n{}",
                result.status.code(),
                tail(&result.stderr, 40)
            ));
        }
        Ok(CommandResult {
            stdout: result.stdout,
            stderr: result.stderr,
        })
    }

    fn list(&mut self, label: &str) -> Vec<Value> {
        let result = self.cli_retry(&["list", "-g", "--json"], label, 3);
        let rows: Vec<Value> = serde_json::from_str(&result.stdout)
            .unwrap_or_else(|error| panic!("list output is not JSON ({error}): {}", result.stdout));
        for row in &rows {
            assert_eq!(
                row["scope"].as_str(),
                Some("global"),
                "list row must be global: {row}"
            );
        }
        rows
    }

    fn read_lock(&self) -> Option<Value> {
        match fs::read_to_string(self.lock_path()) {
            Ok(raw) => Some(serde_json::from_str(&raw).expect("lock is JSON")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("cannot read lock: {error}"),
        }
    }

    fn project_root(&self, name: &str) -> PathBuf {
        self.root.join("projects").join(name)
    }

    fn read_project_lock(&self, project: &Path) -> Option<Value> {
        match fs::read_to_string(project.join("skills-lock.json")) {
            Ok(raw) => Some(serde_json::from_str(&raw).expect("project lock is JSON")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("cannot read project lock: {error}"),
        }
    }

    /// Regression recipe: names, paths, lock v3, unmanaged survival.
    fn regress(
        &mut self,
        label: &str,
        expected_names: &[&str],
        expected_paths: &[(&str, PathBuf)],
        expected_lock_keys: &[&str],
    ) {
        let rows = self.list(&format!("list-{label}"));
        let names: BTreeSet<String> = rows
            .iter()
            .map(|row| {
                row["name"]
                    .as_str()
                    .expect("list row has a name")
                    .to_string()
            })
            .collect();
        let expected: BTreeSet<String> =
            expected_names.iter().map(|name| name.to_string()).collect();
        assert_eq!(
            names, expected,
            "{label}: `list -g --json` names changed unexpectedly"
        );

        for (name, expected_path) in expected_paths {
            let row = rows
                .iter()
                .find(|row| row["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("{label}: {name} missing from list"));
            let actual = PathBuf::from(
                row["path"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{label}: {name} has no path")),
            );
            assert_eq!(
                actual, *expected_path,
                "{label}: {name} list path does not match the physical path"
            );
        }

        assert!(
            self.canonical("unmanaged-keep").is_dir(),
            "{label}: unmanaged canonical skill was deleted"
        );
        assert!(
            self.agent_skill(".claude", "unmanaged-agent").is_dir(),
            "{label}: unmanaged agent skill was deleted"
        );

        self.assert_lock(label, expected_lock_keys);
        summary(
            &self.evidence,
            &format!("PASS {label}: names={names:?} lock={expected_lock_keys:?}"),
        );
    }

    fn assert_lock(&self, label: &str, expected_keys: &[&str]) {
        let expected: BTreeSet<&str> = expected_keys.iter().copied().collect();
        match self.read_lock() {
            Some(lock) => {
                fs::write(
                    self.evidence.join(format!("lock-{label}.json")),
                    lock.to_string(),
                )
                .expect("write lock evidence");
                assert_eq!(lock["version"].as_u64(), Some(3), "{label}: lock is not v3");
                let skills = lock["skills"]
                    .as_object()
                    .unwrap_or_else(|| panic!("{label}: lock.skills is not an object"));
                let actual: BTreeSet<&str> = skills.keys().map(String::as_str).collect();
                assert_eq!(
                    actual, expected,
                    "{label}: lock keys do not match the expected managed set"
                );
                for name in expected {
                    let entry = &skills[name];
                    let hash = entry["skillFolderHash"]
                        .as_str()
                        .expect("lock entry has skillFolderHash");
                    assert_eq!(hash.len(), 40, "{label}: {name} lock hash is not 40 hex");
                    assert!(
                        hash.chars().all(|character| character.is_ascii_hexdigit()),
                        "{label}: {name} lock hash is not hex"
                    );
                }
            }
            None => assert!(
                expected.is_empty(),
                "{label}: lock file is missing but {} keys are expected",
                expected.len()
            ),
        }
    }

    fn assert_symlink_to(&self, path: &Path, target: &Path) {
        let metadata = fs::symlink_metadata(path)
            .unwrap_or_else(|error| panic!("{} is missing: {error}", path.display()));
        assert!(
            metadata.file_type().is_symlink(),
            "{} must be a symlink",
            path.display()
        );
        let resolved = fs::canonicalize(path).expect("symlink resolves");
        let expected = fs::canonicalize(target).expect("target resolves");
        assert_eq!(
            resolved,
            expected,
            "{} does not point at {}",
            path.display(),
            target.display()
        );
    }

    fn assert_missing(&self, path: &Path) {
        assert!(
            fs::symlink_metadata(path).is_err(),
            "{} must not exist",
            path.display()
        );
    }

    fn summary_line(&self, line: &str) {
        summary(&self.evidence, line);
    }

    fn assert_no_forbidden_invocations(&self) {
        for line in &self.command_log {
            assert!(
                forbidden_argv(&line.split_whitespace().skip(2).collect::<Vec<_>>()).is_none(),
                "forbidden invocation recorded: {line}"
            );
        }
        self.summary_line(&format!(
            "PASS forbidden-argv scan across {} invocations",
            self.command_log.len()
        ));
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if self.keep {
            println!("keeping sandbox HOME at {}", self.root.display());
        } else if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("could not remove sandbox {}: {error}", self.root.display());
        }
    }
}

fn write_skill(dir: &Path, name: &str, description: &str) {
    fs::create_dir_all(dir).expect("create skill dir");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nfixture body\n"),
    )
    .expect("write SKILL.md");
}

fn append(path: &Path, line: &str) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open evidence log");
    file.write_all(line.as_bytes()).expect("append evidence");
}

fn summary(evidence: &Path, line: &str) {
    println!("{line}");
    append(&evidence.join("summary.txt"), &format!("{line}\n"));
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// The app must never send these forms (ticket 10 / acceptance M09 / M16).
fn forbidden_argv(args: &[&str]) -> Option<String> {
    for (index, arg) in args.iter().enumerate() {
        if matches!(*arg, "update" | "check" | "--all" | "--copy") {
            return Some(format!("forbidden token {arg}"));
        }
        if *arg == "-a" && args.get(index + 1) == Some(&"*") {
            return Some("wildcard -a '*'".to_string());
        }
        if *arg == "remove" && args.get(index + 1) == Some(&"--all") {
            return Some("remove --all".to_string());
        }
    }
    None
}

struct RawOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_npx(root: &Path, cwd: &Path, argv: &[String]) -> RawOutput {
    let npx = find_on_path("npx").expect("npx must be on PATH to run the real-CLI sandbox test");
    let path = env::var("PATH").unwrap_or_default();
    let tmp = root.join("tmp");
    let mut command = Command::new(&npx);
    command
        .args(argv)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", &path)
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("TMPDIR", &tmp)
        .env("LANG", "en_US.UTF-8")
        .env("CI", "true")
        .env("DISABLE_TELEMETRY", "1")
        .env("DO_NOT_TRACK", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("npm_config_yes", "true")
        .env("npm_config_update_notifier", "false")
        .env("npm_config_fund", "false")
        .env("npm_config_audit", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in [
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "NODE_USE_ENV_PROXY",
    ] {
        if let Ok(value) = env::var(key) {
            command.env(key, value);
        }
    }

    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("cannot spawn {}: {error}", npx.display()));
    let status = wait_with_timeout(&mut child, COMMAND_TIMEOUT);
    let stdout = read_pipe(child.stdout.take());
    let stderr = read_pipe(child.stderr.take());
    RawOutput {
        status,
        stdout,
        stderr,
    }
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let status = child.wait().expect("reap timed out child");
                panic!("skills CLI exceeded {timeout:?} and was killed ({status:?})");
            }
            Err(error) => panic!("wait failed: {error}"),
        }
    }
}

fn read_pipe(pipe: Option<impl Read>) -> String {
    let Some(mut pipe) = pipe else {
        return String::new();
    };
    let mut buffer = Vec::new();
    pipe.read_to_end(&mut buffer).expect("read child pipe");
    String::from_utf8_lossy(&buffer).into_owned()
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn install_json(result: &CommandResult) -> Vec<Value> {
    serde_json::from_str(&result.stdout).unwrap_or_else(|error| {
        panic!(
            "install --json is not JSON ({error})\nstdout:\n{}\nstderr:\n{}",
            result.stdout,
            tail(&result.stderr, 20)
        )
    })
}

fn assert_installed(rows: &[Value], name: &str, mode: &str, path: &Path) {
    let row = rows
        .iter()
        .find(|row| row["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("{name} missing from add --json: {rows:?}"));
    assert_eq!(row["status"].as_str(), Some("installed"), "{name}: {row}");
    assert_eq!(row["mode"].as_str(), Some(mode), "{name}: {row}");
    assert_eq!(
        PathBuf::from(row["path"].as_str().expect("install path")),
        path,
        "{name}: install path"
    );
    assert_eq!(row["scope"].as_str(), Some("global"), "{name}: {row}");
    validate_skill_names(&[name.to_string()]).unwrap_or_else(|error| {
        panic!("ticket-10 validation rejected real name {name}: {error:?}")
    });
}

#[test]
#[ignore = "network + real npx skills@1.7.4 sandbox; run explicitly"]
fn real_cli_sandbox_install_update_detach_uninstall_and_regression() {
    let mut sbx = Sandbox::new();
    let started = Instant::now();
    sbx.summary_line(&format!(
        "sandbox real CLI: repo={REPO} pinned={PINNED} home={}",
        sbx.home.display()
    ));

    // Probe the pinned CLI once so the sandbox npm cache is warm; this also
    // proves npx resolves the pinned package inside the temp HOME.
    let version = sbx.cli_retry(&["--version"], "version", 3);
    assert_eq!(version.stdout.trim(), "1.7.4");

    // Baseline: only the two unmanaged fixtures exist.
    sbx.regress(
        "initial",
        &["unmanaged-keep", "unmanaged-agent"],
        &[
            ("unmanaged-keep", sbx.canonical("unmanaged-keep").clone()),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &[],
    );

    // M05: install one skill with an explicit agent + universal.
    let single = sbx.cli(
        &[
            "add",
            REPO,
            "-g",
            "-s",
            "code-review",
            "-a",
            "claude-code",
            "-a",
            "universal",
            "-y",
            "--json",
        ],
        "add-single",
    );
    let single_json = install_json(&single);
    assert_installed(
        &single_json,
        "code-review",
        "symlink",
        &sbx.canonical("code-review"),
    );
    sbx.assert_symlink_to(
        &sbx.agent_skill(".claude", "code-review"),
        &sbx.canonical("code-review"),
    );
    sbx.regress(
        "after-single",
        &["code-review", "unmanaged-keep", "unmanaged-agent"],
        &[
            ("code-review", sbx.canonical("code-review")),
            ("unmanaged-keep", sbx.canonical("unmanaged-keep")),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &["code-review"],
    );

    // M06: two skills, two explicit agents + universal.
    let multi = sbx.cli(
        &[
            "add",
            REPO,
            "-g",
            "-s",
            "implement",
            "-s",
            "grill-with-docs",
            "-a",
            "claude-code",
            "-a",
            "qoder",
            "-a",
            "universal",
            "-y",
            "--json",
        ],
        "add-multiple",
    );
    let multi_json = install_json(&multi);
    for name in ["implement", "grill-with-docs"] {
        assert_installed(&multi_json, name, "symlink", &sbx.canonical(name));
        sbx.assert_symlink_to(&sbx.agent_skill(".claude", name), &sbx.canonical(name));
        sbx.assert_symlink_to(&sbx.agent_skill(".qoder", name), &sbx.canonical(name));
    }
    sbx.regress(
        "after-multiple",
        &[
            "code-review",
            "implement",
            "grill-with-docs",
            "unmanaged-keep",
            "unmanaged-agent",
        ],
        &[
            ("code-review", sbx.canonical("code-review")),
            ("implement", sbx.canonical("implement")),
            ("grill-with-docs", sbx.canonical("grill-with-docs")),
            ("unmanaged-keep", sbx.canonical("unmanaged-keep")),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &["code-review", "implement", "grill-with-docs"],
    );

    // M10 step 1: a single non-universal agent installs a copy and writes no
    // canonical folder.
    let copy = sbx.cli(
        &[
            "add",
            REPO,
            "-g",
            "-s",
            "ask-matt",
            "-a",
            "claude-code",
            "-y",
            "--json",
        ],
        "add-copy",
    );
    let copy_json = install_json(&copy);
    assert_installed(
        &copy_json,
        "ask-matt",
        "copy",
        &sbx.agent_skill(".claude", "ask-matt"),
    );
    assert!(
        fs::symlink_metadata(sbx.canonical("ask-matt")).is_err(),
        "copy install must not create a canonical folder"
    );
    sbx.regress(
        "after-copy",
        &[
            "ask-matt",
            "code-review",
            "implement",
            "grill-with-docs",
            "unmanaged-keep",
            "unmanaged-agent",
        ],
        &[
            ("ask-matt", sbx.agent_skill(".claude", "ask-matt")),
            ("code-review", sbx.canonical("code-review")),
            ("implement", sbx.canonical("implement")),
            ("grill-with-docs", sbx.canonical("grill-with-docs")),
            ("unmanaged-keep", sbx.canonical("unmanaged-keep")),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &["code-review", "implement", "grill-with-docs", "ask-matt"],
    );

    // M08 / M10 step 2: the app's update path is a re-run of `add` with the
    // same explicit agents. Adding universal converts the copy into a symlink
    // while preserving installedAt and the folder hash.
    let before_convert = sbx.read_lock().expect("lock exists before update");
    let before_entry = before_convert["skills"]["ask-matt"].clone();
    let convert = sbx.cli(
        &[
            "add",
            REPO,
            "-g",
            "-s",
            "ask-matt",
            "-a",
            "claude-code",
            "-a",
            "universal",
            "-y",
            "--json",
        ],
        "update-convert-copy",
    );
    let convert_json = install_json(&convert);
    assert_installed(
        &convert_json,
        "ask-matt",
        "symlink",
        &sbx.canonical("ask-matt"),
    );
    sbx.assert_symlink_to(
        &sbx.agent_skill(".claude", "ask-matt"),
        &sbx.canonical("ask-matt"),
    );
    let after_convert = sbx.read_lock().expect("lock exists after update");
    let after_entry = &after_convert["skills"]["ask-matt"];
    assert_eq!(
        after_entry["skillFolderHash"], before_entry["skillFolderHash"],
        "update must keep the same folder hash for unchanged upstream content"
    );
    assert_eq!(
        after_entry["installedAt"], before_entry["installedAt"],
        "update must preserve installedAt"
    );
    sbx.regress(
        "after-update",
        &[
            "ask-matt",
            "code-review",
            "implement",
            "grill-with-docs",
            "unmanaged-keep",
            "unmanaged-agent",
        ],
        &[
            ("ask-matt", sbx.canonical("ask-matt")),
            ("code-review", sbx.canonical("code-review")),
            ("implement", sbx.canonical("implement")),
            ("grill-with-docs", sbx.canonical("grill-with-docs")),
            ("unmanaged-keep", sbx.canonical("unmanaged-keep")),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &["code-review", "implement", "grill-with-docs", "ask-matt"],
    );

    // M11: detach is a native unlink; canonical and lock stay byte-identical.
    let lock_before_detach = fs::read(sbx.lock_path()).expect("lock exists before detach");
    fs::remove_file(sbx.agent_skill(".claude", "code-review")).expect("detach claude link");
    assert_eq!(
        fs::read(sbx.lock_path()).expect("lock exists after detach"),
        lock_before_detach,
        "detach must not modify the lock"
    );
    assert!(
        sbx.canonical("code-review").is_dir(),
        "detach must not touch the canonical folder"
    );
    sbx.assert_symlink_to(
        &sbx.agent_skill(".claude", "implement"),
        &sbx.canonical("implement"),
    );
    sbx.assert_symlink_to(
        &sbx.agent_skill(".qoder", "implement"),
        &sbx.canonical("implement"),
    );
    let detached_rows = sbx.list("list-after-detach");
    let detached = detached_rows
        .iter()
        .find(|row| row["name"].as_str() == Some("code-review"))
        .expect("detached skill remains listed");
    assert_eq!(
        detached["agents"].as_array().map(Vec::len),
        Some(0),
        "detached skill must not report agent mounts"
    );
    sbx.regress(
        "after-detach",
        &[
            "ask-matt",
            "code-review",
            "implement",
            "grill-with-docs",
            "unmanaged-keep",
            "unmanaged-agent",
        ],
        &[
            ("ask-matt", sbx.canonical("ask-matt")),
            ("code-review", sbx.canonical("code-review")),
            ("implement", sbx.canonical("implement")),
            ("grill-with-docs", sbx.canonical("grill-with-docs")),
            ("unmanaged-keep", sbx.canonical("unmanaged-keep")),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &["code-review", "implement", "grill-with-docs", "ask-matt"],
    );

    // M13: uninstall one managed skill; canonical, agent link and lock key go.
    sbx.cli(
        &["remove", "-g", "-s", "code-review", "-y"],
        "remove-code-review",
    );
    sbx.assert_missing(&sbx.canonical("code-review"));
    sbx.assert_missing(&sbx.agent_skill(".claude", "code-review"));
    assert!(
        sbx.canonical("implement").is_dir(),
        "uninstall must not touch other canonical folders"
    );
    sbx.regress(
        "after-remove-single",
        &[
            "ask-matt",
            "implement",
            "grill-with-docs",
            "unmanaged-keep",
            "unmanaged-agent",
        ],
        &[
            ("ask-matt", sbx.canonical("ask-matt")),
            ("implement", sbx.canonical("implement")),
            ("grill-with-docs", sbx.canonical("grill-with-docs")),
            ("unmanaged-keep", sbx.canonical("unmanaged-keep")),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &["implement", "grill-with-docs", "ask-matt"],
    );

    // M14: a copy-only skill (never canonical) is installed and fully removed.
    let copy_only = sbx.cli(
        &[
            "add",
            REPO,
            "-g",
            "-s",
            "diagnosing-bugs",
            "-a",
            "claude-code",
            "-y",
            "--json",
        ],
        "add-copy-only",
    );
    let copy_only_json = install_json(&copy_only);
    assert_installed(
        &copy_only_json,
        "diagnosing-bugs",
        "copy",
        &sbx.agent_skill(".claude", "diagnosing-bugs"),
    );
    assert!(
        fs::symlink_metadata(sbx.canonical("diagnosing-bugs")).is_err(),
        "copy-only install must not create a canonical folder"
    );
    sbx.regress(
        "after-copy-only",
        &[
            "ask-matt",
            "diagnosing-bugs",
            "implement",
            "grill-with-docs",
            "unmanaged-keep",
            "unmanaged-agent",
        ],
        &[
            ("ask-matt", sbx.canonical("ask-matt")),
            (
                "diagnosing-bugs",
                sbx.agent_skill(".claude", "diagnosing-bugs"),
            ),
            ("implement", sbx.canonical("implement")),
            ("grill-with-docs", sbx.canonical("grill-with-docs")),
            ("unmanaged-keep", sbx.canonical("unmanaged-keep")),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &[
            "implement",
            "grill-with-docs",
            "ask-matt",
            "diagnosing-bugs",
        ],
    );
    sbx.cli(
        &["remove", "-g", "-s", "diagnosing-bugs", "-y"],
        "remove-copy-only",
    );
    sbx.assert_missing(&sbx.agent_skill(".claude", "diagnosing-bugs"));
    sbx.assert_missing(&sbx.canonical("diagnosing-bugs"));
    sbx.regress(
        "after-remove-copy-only",
        &[
            "ask-matt",
            "implement",
            "grill-with-docs",
            "unmanaged-keep",
            "unmanaged-agent",
        ],
        &[
            ("ask-matt", sbx.canonical("ask-matt")),
            ("implement", sbx.canonical("implement")),
            ("grill-with-docs", sbx.canonical("grill-with-docs")),
            ("unmanaged-keep", sbx.canonical("unmanaged-keep")),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &["implement", "grill-with-docs", "ask-matt"],
    );

    // Remove the copy-converted skill too; only upstream symlinks remain.
    sbx.cli(&["remove", "-g", "-s", "ask-matt", "-y"], "remove-ask-matt");
    sbx.assert_missing(&sbx.canonical("ask-matt"));
    sbx.assert_missing(&sbx.agent_skill(".claude", "ask-matt"));
    sbx.regress(
        "final",
        &[
            "implement",
            "grill-with-docs",
            "unmanaged-keep",
            "unmanaged-agent",
        ],
        &[
            ("implement", sbx.canonical("implement")),
            ("grill-with-docs", sbx.canonical("grill-with-docs")),
            ("unmanaged-keep", sbx.canonical("unmanaged-keep")),
            (
                "unmanaged-agent",
                sbx.agent_skill(".claude", "unmanaged-agent"),
            ),
        ],
        &["implement", "grill-with-docs"],
    );

    sbx.assert_no_forbidden_invocations();
    sbx.summary_line(&format!(
        "PASS sandbox real-CLI closure in {} ms ({} npx invocations)",
        started.elapsed().as_millis(),
        sbx.sequence
    ));
    sbx.summary_line(
        "NOTE GUI render/startup timings require the release .app and are not covered by this test.",
    );
}

/// Real pinned CLI in project scope: the process cwd is the project root, so
/// `add`/`remove` must write and clean only `<project>/.agents/skills`,
/// `<project>/skills-lock.json` and the agent's project skills directory.
/// A second project and the global fixtures prove isolation. The real HOME is
/// never read because `run_npx` clears the environment and points HOME and
/// XDG_STATE_HOME inside the sandbox.
#[test]
#[ignore = "network + real npx skills@1.7.4 sandbox; run explicitly"]
fn real_cli_project_scope_add_and_remove_stay_inside_the_project() {
    let mut sbx = Sandbox::new();
    let started = Instant::now();
    sbx.summary_line(&format!(
        "sandbox project scope: repo={REPO} pinned={PINNED} home={}",
        sbx.home.display()
    ));

    let version = sbx.cli_retry(&["--version"], "project-version", 3);
    assert_eq!(version.stdout.trim(), "1.7.4");

    let project_raw = sbx.project_root("alpha");
    let other_raw = sbx.project_root("beta");
    for root in [&project_raw, &other_raw] {
        fs::create_dir_all(root.join(".agents/skills")).expect("project canonical dir");
        fs::create_dir_all(root.join(".claude/skills")).expect("project agent dir");
        write_skill(
            &root.join(".agents/skills/project-keep"),
            "project-keep",
            "unmanaged project fixture",
        );
        write_skill(
            &root.join(".claude/skills/project-keep-agent"),
            "project-keep-agent",
            "unmanaged project agent fixture",
        );
    }
    // macOS temp dirs are reached through /var -> /private/var; the CLI prints
    // resolved paths, so compare against canonicalized project roots.
    let project = fs::canonicalize(&project_raw).expect("resolve project root");
    let other = fs::canonicalize(&other_raw).expect("resolve other project root");

    // Project add: no `-g`, no `-s`-less prompt, explicit catalog agent plus
    // universal. `add --json` must report the canonical project path.
    let add = sbx.cli_in(
        &project,
        &[
            "add",
            REPO,
            "-s",
            "code-review",
            "-a",
            "claude-code",
            "-a",
            "universal",
            "-y",
            "--json",
        ],
        "project-add",
    );
    let rows = install_json(&add);
    let row = rows
        .iter()
        .find(|row| row["name"].as_str() == Some("code-review"))
        .unwrap_or_else(|| panic!("code-review missing from project add --json: {rows:?}"));
    assert_eq!(row["status"].as_str(), Some("installed"), "{row}");
    assert_eq!(row["mode"].as_str(), Some("symlink"), "{row}");
    assert_eq!(
        PathBuf::from(row["path"].as_str().expect("install path")),
        project.join(".agents/skills/code-review"),
        "{row}"
    );
    // The CLI reports the cwd scope; the app's project tasks rely on this.
    assert_eq!(row["scope"].as_str(), Some("project"), "{row}");

    sbx.assert_symlink_to(
        &project.join(".claude/skills/code-review"),
        &project.join(".agents/skills/code-review"),
    );
    assert!(
        project
            .join(".agents/skills/code-review/SKILL.md")
            .is_file(),
        "canonical project skill must exist"
    );
    let lock = sbx
        .read_project_lock(&project)
        .expect("project add must write skills-lock.json");
    assert_eq!(lock["version"].as_u64(), Some(1), "project lock is v1");
    let entry = lock["skills"]
        .get("code-review")
        .unwrap_or_else(|| panic!("project lock is missing code-review: {lock}"));
    assert_eq!(entry["source"].as_str(), Some(REPO));
    assert_eq!(entry["sourceType"].as_str(), Some("github"));
    let hash = entry["computedHash"]
        .as_str()
        .expect("project lock entry has computedHash");
    assert_eq!(hash.len(), 64, "project computedHash is 64 hex: {hash}");
    assert!(
        hash.chars().all(|character| character.is_ascii_hexdigit()),
        "project computedHash is hex: {hash}"
    );
    // The app's SHA-256 fingerprint must equal the pinned CLI's output for the
    // same installed folder; otherwise every project check would report a
    // phantom update.
    assert_eq!(
        row["hash"].as_str(),
        Some(hash),
        "add --json hash must match the project lock computedHash"
    );
    let local_hash = skills_box_lib::update::content_hash::folder_hash(
        &project.join(".agents/skills/code-review"),
    )
    .expect("hash the installed project skill");
    assert_eq!(
        local_hash, hash,
        "app folder_hash must match the pinned CLI computedHash"
    );
    sbx.summary_line(&format!("PASS project-add: paths + v1 lock ({hash})"));

    // A second project installs a different skill so removal can prove
    // isolation between projects.
    let other_add = sbx.cli_in(
        &other,
        &[
            "add",
            REPO,
            "-s",
            "implement",
            "-a",
            "claude-code",
            "-a",
            "universal",
            "-y",
            "--json",
        ],
        "project-other-add",
    );
    let other_rows = install_json(&other_add);
    let other_row = other_rows
        .iter()
        .find(|row| row["name"].as_str() == Some("implement"))
        .unwrap_or_else(|| panic!("implement missing from other project: {other_rows:?}"));
    assert_eq!(
        PathBuf::from(other_row["path"].as_str().expect("other path")),
        other.join(".agents/skills/implement")
    );
    sbx.assert_symlink_to(
        &other.join(".claude/skills/implement"),
        &other.join(".agents/skills/implement"),
    );

    // Project remove: no `-g`. Canonical folder, agent link and lock entry all
    // go; unmanaged project entries, the other project and global scope stay.
    sbx.cli_in(
        &project,
        &["remove", "-s", "code-review", "-y"],
        "project-remove",
    );
    sbx.assert_missing(&project.join(".agents/skills/code-review"));
    sbx.assert_missing(&project.join(".claude/skills/code-review"));
    match sbx.read_project_lock(&project) {
        Some(lock) => assert!(
            !lock["skills"]
                .as_object()
                .expect("project lock skills is an object")
                .contains_key("code-review"),
            "project remove must delete the lock entry: {lock}"
        ),
        None => {}
    }
    assert!(
        project
            .join(".agents/skills/project-keep/SKILL.md")
            .is_file(),
        "project remove must not delete unmanaged canonical skills"
    );
    assert!(
        project
            .join(".claude/skills/project-keep-agent/SKILL.md")
            .is_file(),
        "project remove must not delete unmanaged agent skills"
    );
    sbx.assert_symlink_to(
        &other.join(".claude/skills/implement"),
        &other.join(".agents/skills/implement"),
    );
    assert!(
        other.join(".agents/skills/implement/SKILL.md").is_file(),
        "removing in one project must not touch another project"
    );
    sbx.assert_missing(&sbx.canonical("code-review"));
    assert!(
        sbx.canonical("unmanaged-keep").is_dir(),
        "global canonical fixtures must survive project mutations"
    );
    assert!(
        sbx.read_lock().is_none(),
        "project add/remove must not create the global lock"
    );

    sbx.assert_no_forbidden_invocations();
    sbx.summary_line(&format!(
        "PASS project scope closure in {} ms ({} npx invocations)",
        started.elapsed().as_millis(),
        sbx.sequence
    ));
}

#[test]
fn chosen_public_repo_skill_names_pass_ticket_10_validation() {
    // Real upstream names must not be rejected by the shell-metacharacter
    // hardening from ticket 10. This is static (no network) so the normal test
    // suite guards the chosen sandbox repository.
    let validated = validate_skill_names(
        &SANDBOX_SKILLS
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>(),
    )
    .expect("legitimate upstream skill names must pass validation");
    assert_eq!(
        validated,
        SANDBOX_SKILLS
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(validated.len(), SANDBOX_SKILLS.len());
}

#[tokio::test]
#[ignore = "network: read-only GitHub update check; run explicitly"]
async fn real_update_check_is_read_only() {
    let root = env::temp_dir().join(format!(
        "skills-box-update-check-{}-{}",
        std::process::id(),
        now_millis()
    ));
    let home = root.join("home");
    let app_data = root.join("app-data");
    fs::create_dir_all(home.join(".agents")).expect("seed home");
    let paths = AppPaths::new(&home, &app_data);
    paths.ensure_dirs().expect("app data dirs");
    let lock = json!({
        "version": 3,
        "skills": {
            "code-review": {
                "source": REPO,
                "sourceType": "github",
                "sourceUrl": "https://github.com/mattpocock/skills.git",
                "skillPath": "skills/engineering/code-review/SKILL.md",
                "skillFolderHash": "fee7bb5fdddc069d58cbea09b2c36b5a859eac05",
                "installedAt": "2026-10-28T09:23:08.834Z",
                "updatedAt": "2026-10-28T09:23:08.834Z"
            }
        },
        "dismissed": {}
    });
    let lock_path = paths.lock_path();
    let raw = serde_json::to_string_pretty(&lock).expect("serialize lock");
    fs::write(&lock_path, &raw).expect("write lock");

    let report = tokio::time::timeout(
        Duration::from_secs(90),
        UpdateService::new(paths.clone()).check(None),
    )
    .await
    .expect("update check finished before the test deadline")
    .expect("update check returned a report");

    assert_eq!(report.sources.len(), 1, "one source expected: {report:?}");
    let source = &report.sources[0];
    assert_eq!(source.source, REPO);
    assert_eq!(
        source.status,
        skills_box_lib::update::UpdateSourceStatus::Ok,
        "network/rate-limit failure: {report:?}"
    );
    let skill = report
        .skills
        .iter()
        .find(|skill| skill.name == "code-review")
        .expect("code-review result");
    assert!(
        matches!(
            skill.state,
            skills_box_lib::inventory::UpdateState::Current
                | skills_box_lib::inventory::UpdateState::Update
        ),
        "update check must compare hashes, got {skill:?}"
    );
    assert!(skill.remote_hash.is_some(), "remote tree hash expected");

    assert_eq!(
        fs::read_to_string(&lock_path).expect("lock still exists"),
        raw,
        "read-only update check must not rewrite the lock"
    );
    assert!(
        !paths.canonical_skills_dir().exists(),
        "update check must not create a canonical skills folder"
    );
    let cache_files = fs::read_dir(paths.cache_dir())
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert!(
        cache_files >= 1,
        "expected the 6h update cache under {}",
        paths.cache_dir().display()
    );
    println!(
        "update check: state={:?} remote_hash={:?} cache_files={cache_files}",
        skill.state, skill.remote_hash
    );

    fs::remove_dir_all(&root).expect("remove update-check sandbox");
}
