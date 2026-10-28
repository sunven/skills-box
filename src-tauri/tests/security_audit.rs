//! Static hardening audit for configuration that cannot be exercised by a
//! normal unit test: plugin order, capability surface, CSP and debug-only
//! environment hooks. Runtime behavior is covered by the module tests.

use std::fs;
use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest_dir().parent().unwrap().to_path_buf()
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path.as_ref())
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.as_ref().display()))
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn capabilities_expose_only_core_default() {
    let capabilities = manifest_dir().join("capabilities");
    let mut files = 0;
    for entry in fs::read_dir(&capabilities).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "json") {
            files += 1;
            let json: serde_json::Value =
                serde_json::from_str(&read(&path)).expect("capability JSON parses");
            let permissions = json["permissions"]
                .as_array()
                .expect("permissions array");
            assert_eq!(
                permissions,
                &vec![serde_json::Value::String("core:default".to_string())],
                "{} must not expose shell/fs/opener permissions",
                path.display()
            );
        }
    }
    assert!(files >= 1, "at least one capability file is required");
}

#[test]
fn cargo_manifest_has_no_shell_or_fs_plugins_but_keeps_single_instance() {
    let cargo = read(manifest_dir().join("Cargo.toml"));
    assert!(cargo.contains("tauri-plugin-single-instance"));
    assert!(cargo.contains("tauri-plugin-opener"));
    for forbidden in ["tauri-plugin-shell", "tauri-plugin-fs", "tauri-plugin-dialog"] {
        assert!(!cargo.contains(forbidden), "{forbidden} must not be a dependency");
    }
    // The single-instance dependency is not optional: a release build must
    // always prevent a second writer process.
    assert!(!cargo.contains("single-instance = { version = \"2\", optional"));
}

#[test]
fn single_instance_plugin_is_registered_before_the_opener() {
    let lib = read(manifest_dir().join("src/lib.rs"));
    let single = lib
        .find(".plugin(tauri_plugin_single_instance::init")
        .expect("single-instance plugin must be registered");
    let opener = lib
        .find(".plugin(tauri_plugin_opener::init")
        .expect("opener plugin must be registered");
    assert!(single < opener, "single-instance must be registered first");
    assert!(lib.contains("fn focus_main_window"));
    assert!(lib.contains("set_focus"));
    assert!(lib.contains("RunEvent::ExitRequested"));
    assert!(lib.contains("WindowEvent::CloseRequested"));
    assert!(lib.contains("commands::quit_app"));
}

#[test]
fn csp_forbids_remote_scripts_images_and_frames() {
    let config: serde_json::Value =
        serde_json::from_str(&read(manifest_dir().join("tauri.conf.json"))).unwrap();
    let security = &config["app"]["security"];
    let csp = security["csp"].as_str().expect("production CSP");
    for directive in [
        "default-src 'self'",
        "script-src 'self'",
        "img-src 'self' data:",
        "object-src 'none'",
        "base-uri 'self'",
        "frame-ancestors 'none'",
        "form-action 'self'",
        "connect-src 'self' ipc: http://ipc.localhost",
    ] {
        assert!(csp.contains(directive), "CSP is missing {directive}: {csp}");
    }
    assert!(!csp.contains("unsafe-eval"));
    assert!(!csp.contains("http://localhost"));
    assert!(!csp.contains("https://"));
    assert!(!csp.contains(" *"));
    // Tailwind v4 and dynamic React widths need inline styles. This exception
    // is documented in docs/security.md and never extends to scripts.
    assert!(csp.contains("style-src 'self' 'unsafe-inline'"));
    let docs = read(repo_root().join("docs/security.md"));
    assert!(docs.contains("Tailwind"));
    assert!(docs.contains("style-src 'self' 'unsafe-inline'"));

    let dev_csp = security["devCsp"].as_str().expect("dev CSP");
    for origin in ["http://localhost:1420", "ws://localhost:1420"] {
        assert!(dev_csp.contains(origin));
    }
    assert!(!dev_csp.contains("unsafe-eval"));
}

#[test]
fn product_code_never_uses_skills_list_as_truth() {
    let mut sources = Vec::new();
    rust_sources(&manifest_dir().join("src"), &mut sources);
    for path in sources {
        let source = read(&path);
        let product = source
            .split("#[cfg(test)]")
            .next()
            .expect("source has a prefix");
        for (index, line) in product.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            for forbidden in ["skills list", "list -g --json", "\"list\""] {
                assert!(
                    !code.contains(forbidden),
                    "{}:{} contains forbidden CLI usage {forbidden:?}; `skills list` is diagnostic-only and must not be a data source",
                    path.display(),
                    index + 1
                );
            }
        }
    }
}

#[test]
fn no_shell_is_used_outside_the_debug_fake_cli() {
    let mut sources = Vec::new();
    rust_sources(&manifest_dir().join("src"), &mut sources);
    for path in sources {
        let source = read(&path);
        let product = source
            .split("#[cfg(test)]")
            .next()
            .expect("source has a prefix");
        let is_fake_executor = path.ends_with("process/tokio_executor.rs");
        let shell_mentions = product.matches("/bin/sh").count()
            + product.matches("Command::new(\"sh\")").count();
        if shell_mentions > 0 {
            assert!(
                is_fake_executor,
                "{} spawns a shell; product commands must use argv only",
                path.display()
            );
            assert!(source.contains("SKILLS_BOX_FAKE_CLI"));
            assert!(source.contains("cfg!(debug_assertions)"));
        }
    }
}

#[test]
fn debug_environment_hooks_are_gated_in_debug_builds() {
    let paths = read(manifest_dir().join("src/paths.rs"));
    assert!(paths.contains("cfg!(debug_assertions)"));
    assert!(paths.contains("SKILLS_BOX_ROOT"));

    let resolver = read(manifest_dir().join("src/env/resolver.rs"));
    assert!(resolver.contains("SKILLS_BOX_NODE"));
    assert!(resolver.contains("cfg!(debug_assertions)"));

    let executor = read(manifest_dir().join("src/process/tokio_executor.rs"));
    assert!(executor.contains("SKILLS_BOX_FAKE_CLI"));
    assert!(executor.contains("cfg!(debug_assertions)"));
    assert!(executor.contains("if !debug"));
}

#[test]
fn command_builders_only_use_the_approved_skills_flags() {
    // Strip `#[cfg(test)]` blocks so the forbidden-flag assertions maintained
    // by unit tests do not count as product usage.
    let files = [
        "src/cli.rs",
        "src/tasks/install.rs",
        "src/tasks/update.rs",
        "src/tasks/uninstall.rs",
    ];
    for file in files {
        let source = read(manifest_dir().join(file));
        let product = source
            .split("#[cfg(test)]")
            .next()
            .expect("source has a prefix");
        for forbidden in [
            "\"list\"",
            "\"upgrade\"",
            "\"--copy\"",
            "\"remove\", \"--all\"",
        ] {
            assert!(
                !product.contains(forbidden),
                "{file} product code contains forbidden CLI token {forbidden}"
            );
        }
    }
}
