use super::*;
use serde_json::json;
use transport::{Output, Runtime};
fn observation(host: Host) -> Observation {
    let mut o = Observation {
        snapshot: Snapshot {
            host,
            config_root: "/fixture".into(),
            executable: "/cli".into(),
            version: "test".into(),
            capabilities: contracts::actions(host)
                .into_iter()
                .map(|action| Capability {
                    action,
                    blocked_reason: None,
                })
                .collect(),
            markets: vec![Market {
                name: "team".into(),
                source_kind: "git".into(),
                source: "https://example.com/team.git".into(),
                root: None,
                reference: None,
                sparse_paths: vec![],
                read_only_reason: None,
                remove_blocked_reason: None,
                sync_blocked_reason: None,
                catalog_error: None,
                affected_plugins: vec!["demo@team".into()],
            }],
            plugins: vec![],
            warnings: vec![],
            fingerprint: "before".into(),
            source_kinds: vec!["local".into(), "git".into(), "url".into()],
        },
        user_version: Some("sha256:before".into()),
    };
    o.snapshot.plugins.push(Plugin {
        id: "demo@team".into(),
        name: "demo".into(),
        marketplace: "team".into(),
        description: None,
        version: Some("1.0.0".into()),
        installation_state: "installed".into(),
        installations: vec![Installation {
            scope: "user".into(),
            version: Some("1.0.0".into()),
            path: None,
            project_path: None,
        }],
        user_enabled: Some(false),
        effective_enabled: Some(false),
        source: json!("./demo"),
        local_root: None,
        read_only_reason: None,
        install_blocked_reason: None,
    });
    o
}
fn request(host: Host, action: Action, target: &str) -> Request {
    Request {
        host,
        action,
        target: target.into(),
        source: String::new(),
        source_kind: "git".into(),
        reference: String::new(),
        sparse_paths: vec![],
    }
}
fn preview(r: Request) -> Preview {
    Preview {
        token: "fixture".into(),
        request: r,
        config_root: "/fixture".into(),
        version: "test".into(),
        command: "fixture".into(),
        consequences: vec![],
        affected_plugins: vec![],
        fingerprint: "before".into(),
    }
}
#[test]
fn blocks_cross_scope_removal_and_shared_plugin_changes() {
    let mut o = observation(Host::Claude);
    o.snapshot.markets[0].remove_blocked_reason = Some("project impact".into());
    assert!(
        operations::arguments(&request(Host::Claude, Action::RemoveMarket, "team"), &o).is_err()
    );
    o.snapshot.plugins[0].installations.push(Installation {
        scope: "project".into(),
        version: None,
        path: None,
        project_path: Some("/project".into()),
    });
    assert!(
        operations::arguments(&request(Host::Claude, Action::Uninstall, "demo@team"), &o).is_err()
    );
    assert!(operations::arguments(&request(Host::Claude, Action::Enable, "demo@team"), &o).is_ok());
}
#[test]
fn unknown_response_cannot_be_success_and_partial_writes_are_preserved() {
    let before = observation(Host::Claude);
    let mut after = observation(Host::Claude);
    after.snapshot.fingerprint = "after".into();
    after.snapshot.plugins[0].user_enabled = Some(true);
    let out = Output {
        success: true,
        value: Some(json!({"unexpected":true})),
        diagnostic: "unknown response".into(),
    };
    let result = operations::outcome(
        preview(request(Host::Claude, Action::Enable, "demo@team")),
        &before,
        Ok(after),
        Ok(out),
    );
    assert_eq!(result.status, "partial");
    assert_eq!(result.command_succeeded, Some(true));
    assert_eq!(result.snapshot.unwrap().plugins[0].user_enabled, Some(true));
}
#[test]
fn post_read_failure_is_unknown_and_approval_failure_needs_host() {
    let before = observation(Host::Claude);
    let result = operations::outcome(
        preview(request(Host::Claude, Action::Install, "demo@team")),
        &before,
        Err("read failed".into()),
        Ok(Output {
            success: true,
            value: Some(json!({"outcome":"ok"})),
            diagnostic: String::new(),
        }),
    );
    assert_eq!(result.status, "unconfirmed");
    assert!(result.executed);
    let result = operations::outcome(
        preview(request(Host::Claude, Action::Install, "demo@team")),
        &before,
        Ok(observation(Host::Claude)),
        Ok(Output {
            success: false,
            value: Some(json!({"outcome":"failed"})),
            diagnostic: "installation requires approval".into(),
        }),
    );
    assert_eq!(result.status, "needs_host");
}
#[test]
fn input_guards_and_native_argument_shapes() {
    let o = observation(Host::Claude);
    let mut r = request(Host::Claude, Action::AddMarket, "");
    for source in [
        "--help",
        "https://user:password@example.com/repo",
        "https://example.com/repo?token=secret",
    ] {
        r.source = source.into();
        assert!(operations::arguments(&r, &o).is_err());
    }
    r.source = "owner/repo".into();
    r.reference = "main".into();
    r.sparse_paths = vec![".claude-plugin".into(), "plugins/demo".into()];
    let args = operations::arguments(&r, &o).unwrap();
    assert!(args.contains(&"owner/repo#main".into()));
    assert_eq!(args.iter().filter(|s| *s == "--sparse").count(), 1);
    r.sparse_paths = vec!["../outside".into()];
    assert!(operations::arguments(&r, &o).is_err());
}
#[test]
fn local_detail_rejects_symlink_escape_and_limits_content() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("plugin");
    fs::create_dir_all(&root).unwrap();
    fs::write(t.path().join("secret"), "credential").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(t.path().join("secret"), root.join("README.md")).unwrap();
    let mut o = observation(Host::Codex);
    o.snapshot.plugins[0].local_root = Some(root.to_string_lossy().into());
    let details = inventory::details(&o.snapshot.plugins[0]);
    assert!(details.readme.is_none());
    assert!(inventory::local_path(&root, "../secret").is_err());
}
#[test]
fn unknown_versions_are_per_action_and_cannot_inherit_write_semantics() {
    let c = contracts::shipped(Host::Claude, "99.1.0");
    assert!(c.capabilities.iter().all(|c| c.blocked_reason.is_some()));
    assert!(c.sources.is_empty());
    let c = contracts::shipped(Host::Codex, "0.170.0");
    assert!(c.capabilities.iter().all(|c| c.blocked_reason.is_none()));
    assert!(!c.capabilities.iter().any(|c| c.action == Action::Update));
}

/// Explicit opt-in: real CLIs, isolated HOME and static fixture only.
#[test]
#[ignore = "requires installed Codex and Claude CLIs; all writes stay in temporary homes"]
fn native_adapters_isolated_lifecycle() {
    for (host, executable) in [
        (Host::Codex, "/opt/homebrew/bin/codex"),
        (Host::Claude, "/Users/sunven/.local/bin/claude"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let base = Runtime {
            host,
            home: temp.path().into(),
            root: temp.path().into(),
            executable: executable.into(),
            cwd: temp.path().into(),
            env: std::collections::BTreeMap::from([(
                "PATH".into(),
                "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".into(),
            )]),
            isolated: true,
        };
        let rt = contracts::isolated_runtime(&base, temp.path()).unwrap();
        let market = contracts::fixture(temp.path()).unwrap();
        let contract = contracts::shipped(host, &rt.version().unwrap());
        let load = || {
            let mut o = inventory::load(&rt, contract.capabilities.clone(), &[]).unwrap();
            o.snapshot.source_kinds = contract.sources.clone();
            o
        };
        let perform = |action, target: &str| {
            let mut r = request(host, action, target);
            r.source = market.to_string_lossy().into();
            r.source_kind = "local".into();
            let before = load();
            let out = operations::execute(&rt, &r, &before).unwrap();
            assert!(
                operations::response_matches(&r, &out),
                "{host:?} {action:?}: {}",
                out.diagnostic
            );
            let after = load();
            assert!(
                operations::postcondition(&r, &before, &after, &out),
                "{host:?} {action:?}: {}\n{}",
                out.diagnostic,
                serde_json::to_string_pretty(&after.snapshot).unwrap()
            );
            after
        };
        perform(Action::AddMarket, "");
        perform(Action::Install, "demo@skills-box-verification");
        perform(Action::Disable, "demo@skills-box-verification");
        let disabled = load();
        if host == Host::Codex {
            // Official config compare-and-swap rejects stale versions, including dotted IDs.
            perform(Action::Enable, "demo@skills-box-verification");
            assert!(rt.rpc("config/value/write",json!({"keyPath":"plugins.\"demo@skills-box-verification\".enabled","value":false,"mergeStrategy":"replace","filePath":rt.root.join("config.toml"),"expectedVersion":disabled.user_version})).is_err());
            let after = perform(Action::Reinstall, "demo@skills-box-verification");
            assert_eq!(after.snapshot.plugins[0].user_enabled, Some(true));
            let details = inventory::details(&after.snapshot.plugins[0]);
            assert_eq!(details.components["Skills"], vec!["hello"]);
            let key = "plugins.\"dotted.name@skills-box-verification\".enabled";
            rt.rpc("config/value/write",json!({"keyPath":key,"value":false,"mergeStrategy":"replace","filePath":rt.root.join("config.toml"),"expectedVersion":after.user_version})).unwrap();
            assert_eq!(
                rt.rpc("config/read", json!({"includeLayers":true}))
                    .unwrap()["config"]["plugins"]["dotted.name@skills-box-verification"]
                    ["enabled"],
                false
            );
        } else {
            perform(Action::Update, "demo@skills-box-verification");
            perform(Action::Enable, "demo@skills-box-verification");
        }
        perform(Action::Uninstall, "demo@skills-box-verification");
        perform(Action::RemoveMarket, "skills-box-verification");
        assert!(load().snapshot.markets.is_empty());
    }
}

#[test]
#[ignore = "real isolated scope and orphan contract verification"]
fn native_adapters_scope_and_orphan() {
    for (host, exe) in [
        (Host::Codex, "/opt/homebrew/bin/codex"),
        (Host::Claude, "/Users/sunven/.local/bin/claude"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let base = Runtime {
            host,
            home: temp.path().into(),
            root: temp.path().into(),
            executable: exe.into(),
            cwd: temp.path().into(),
            env: std::collections::BTreeMap::from([(
                "PATH".into(),
                "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".into(),
            )]),
            isolated: true,
        };
        let rt = contracts::isolated_runtime(&base, temp.path()).unwrap();
        let market = contracts::fixture(temp.path()).unwrap();
        let contract = contracts::shipped(host, &rt.version().unwrap());
        assert!(
            rt.run(
                &[
                    "plugin".into(),
                    "marketplace".into(),
                    "add".into(),
                    market.to_string_lossy().into(),
                    "--json".into()
                ],
                true
            )
            .unwrap()
            .success
        );
        let install = if host == Host::Codex {
            "add"
        } else {
            "install"
        };
        assert!(
            rt.run(
                &[
                    "plugin".into(),
                    install.into(),
                    "demo@skills-box-verification".into(),
                    "--json".into()
                ],
                true
            )
            .unwrap()
            .success
        );
        if host == Host::Claude {
            let project = temp.path().join("project");
            fs::create_dir(&project).unwrap();
            let mut project_rt = rt.clone();
            project_rt.cwd = project;
            assert!(
                project_rt
                    .run(
                        &[
                            "plugin".into(),
                            "install".into(),
                            "demo@skills-box-verification".into(),
                            "--scope".into(),
                            "project".into(),
                            "--json".into()
                        ],
                        true
                    )
                    .unwrap()
                    .success
            );
            let obs = inventory::load(&rt, contract.capabilities, &[]).unwrap();
            assert!(obs.snapshot.markets[0].remove_blocked_reason.is_some());
            assert!(operations::arguments(
                &request(host, Action::RemoveMarket, "skills-box-verification"),
                &obs
            )
            .is_err());
            assert_eq!(obs.snapshot.plugins[0].installations.len(), 2);
            assert!(
                project_rt
                    .run(
                        &[
                            "plugin".into(),
                            "install".into(),
                            "demo@skills-box-verification".into(),
                            "--scope".into(),
                            "local".into(),
                            "--json".into()
                        ],
                        true
                    )
                    .unwrap()
                    .success
            );
            let before = inventory::load(
                &rt,
                contracts::shipped(host, &rt.version().unwrap()).capabilities,
                &[],
            )
            .unwrap();
            assert_eq!(before.snapshot.plugins[0].installations.len(), 3);
            assert!(operations::arguments(
                &request(host, Action::RemoveMarket, "skills-box-verification"),
                &before
            )
            .is_err());
            // Verify the native user-scope flag really has broader effects;
            // production preflight above must prevent this command.
            let removed = rt
                .run(
                    &[
                        "plugin".into(),
                        "marketplace".into(),
                        "remove".into(),
                        "skills-box-verification".into(),
                        "--scope".into(),
                        "user".into(),
                        "--json".into(),
                    ],
                    true,
                )
                .unwrap();
            assert!(removed.success, "{}", removed.diagnostic);
            let remaining =
                inventory::optional_json(&rt.root.join("plugins/installed_plugins.json")).unwrap();
            assert!(remaining["plugins"]["demo@skills-box-verification"].is_null());
        } else {
            let project = temp.path().join("project");
            fs::create_dir_all(project.join(".codex")).unwrap();
            fs::write(
                project.join(".codex/config.toml"),
                "[plugins.\"demo@skills-box-verification\"]\nenabled=true\n",
            )
            .unwrap();
            let obs = inventory::load(&rt, contract.capabilities.clone(), &[project]).unwrap();
            assert!(obs.snapshot.markets[0].remove_blocked_reason.is_some());
            assert!(
                rt.run(
                    &[
                        "plugin".into(),
                        "marketplace".into(),
                        "remove".into(),
                        "skills-box-verification".into(),
                        "--json".into()
                    ],
                    true
                )
                .unwrap()
                .success
            );
            let obs = inventory::load(&rt, contract.capabilities, &[]).unwrap();
            assert_eq!(obs.snapshot.plugins[0].installation_state, "unconfirmed");
            assert_eq!(obs.snapshot.plugins[0].user_enabled, Some(true));
        }
    }
}

fn fake_service() -> (tempfile::TempDir, Service, Runtime) {
    use crate::{
        env::{EnvResolver, EnvironmentService},
        process::tokio_executor::TokioExecutor,
        tasks::{InventoryConflictProvider, TaskManagerConfig},
    };
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path().join("home"), temp.path().join("app-data"));
    let environment = Arc::new(EnvironmentService::new(EnvResolver::from_process(
        paths.clone(),
    )));
    let projects = Arc::new(ProjectRegistry::new(paths.clone()));
    let tasks = TaskManager::with_projects(
        environment,
        Arc::new(TokioExecutor),
        Arc::new(InventoryConflictProvider::new(paths.clone())),
        paths.clone(),
        projects.clone(),
        TaskManagerConfig::default(),
    );
    let service = Service::new(paths.clone(), tasks, projects);
    let executable = temp.path().join("fake-cli");
    fs::write(
        &executable,
        include_str!("../../tests/fixtures/native-plugins/fake-cli.py"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let rt = Runtime {
        host: Host::Claude,
        home: paths.home().into(),
        root: paths.home().join(".claude"),
        executable,
        cwd: temp.path().into(),
        env: std::collections::BTreeMap::from([(
            "PATH".into(),
            "/usr/bin:/bin:/opt/homebrew/bin".into(),
        )]),
        isolated: true,
    };
    fs::create_dir_all(rt.root.join("plugins")).unwrap();
    let market = contracts::fixture(temp.path()).unwrap();
    fs::write(rt.root.join("settings.json"),json!({"extraKnownMarketplaces":{"skills-box-verification":{"source":{"source":"directory","path":market}}},"enabledPlugins":{"demo@skills-box-verification":false}}).to_string()).unwrap();
    fs::write(rt.root.join("plugins/known_marketplaces.json"),json!({"skills-box-verification":{"source":{"source":"directory","path":market},"installLocation":market}}).to_string()).unwrap();
    fs::write(rt.root.join("plugins/installed_plugins.json"),json!({"version":2,"plugins":{"demo@skills-box-verification":[{"scope":"user","version":"1.0.0","installPath":market.join("demo")}]}}).to_string()).unwrap();
    *service.0.runtime.lock().unwrap() = Some(rt.clone());
    (temp, service, rt)
}
#[tokio::test]
async fn stale_confirmation_is_consumed_without_writing() {
    let (_temp, service, rt) = fake_service();
    let p = service
        .prepare(request(
            Host::Claude,
            Action::Enable,
            "demo@skills-box-verification",
        ))
        .await
        .unwrap();
    let mut config = inventory::optional_json(&rt.root.join("settings.json")).unwrap();
    config["otherExternalChange"] = json!(true);
    fs::write(rt.root.join("settings.json"), config.to_string()).unwrap();
    let result = service.execute(p.token.clone()).await.unwrap();
    assert_eq!(result.status, "conflict");
    assert!(!result.executed);
    assert!(!rt.root.join("started").exists());
    assert!(service.execute(p.token).await.is_err());
}
#[tokio::test]
async fn detached_worker_retains_permit_and_exit_waits_through_verification() {
    use std::time::Duration;
    let (_temp, service, rt) = fake_service();
    fs::write(rt.root.join("hold"), "").unwrap();
    let p = service
        .prepare(request(
            Host::Claude,
            Action::Enable,
            "demo@skills-box-verification",
        ))
        .await
        .unwrap();
    let s = service.clone();
    let ipc = tokio::spawn(async move { s.execute(p.token).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !rt.root.join("started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    ipc.abort(); // A dropped IPC caller cannot cancel or release the worker.
    assert!(service.0.tasks.try_acquire_mutation().is_err());
    let tasks = service.0.tasks.clone();
    let shutdown = tokio::spawn(async move { tasks.shutdown_mutations().await });
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(!shutdown.is_finished());
    assert_eq!(
        service.0.tasks.active_mutation().unwrap().kind,
        crate::interrupted::MutationKind::NativePlugin
    );
    fs::remove_file(rt.root.join("hold")).unwrap();
    let report = tokio::time::timeout(Duration::from_secs(5), shutdown)
        .await
        .unwrap()
        .unwrap();
    assert!(!report.timed_out);
    assert!(service.0.tasks.active_mutation().is_none());
    let record = service.record(Host::Claude).unwrap().unwrap();
    assert_eq!(record.phase, "finished");
    assert_eq!(record.outcome.unwrap().status, "succeeded");
    assert!(!service.0.paths.interrupted_marker_path().exists());
}
#[tokio::test]
async fn service_preserves_partial_and_unknown_results_without_retry() {
    for (trigger, status) in [
        ("fail-after-write", "partial"),
        ("fail-verification", "unconfirmed"),
    ] {
        let (_temp, service, rt) = fake_service();
        fs::write(rt.root.join(trigger), "").unwrap();
        let p = service
            .prepare(request(
                Host::Claude,
                Action::Enable,
                "demo@skills-box-verification",
            ))
            .await
            .unwrap();
        let outcome = service.execute(p.token).await.unwrap();
        assert_eq!(outcome.status, status);
        assert_eq!(
            service
                .record(Host::Claude)
                .unwrap()
                .unwrap()
                .outcome
                .unwrap()
                .status,
            status
        );
        assert_eq!(
            inventory::optional_json(&rt.root.join("settings.json")).unwrap()["enabledPlugins"]
                ["demo@skills-box-verification"],
            true
        );
    }
}
#[tokio::test]
async fn interrupted_record_is_reconciled_as_unknown_not_success() {
    let (_temp, service, _rt) = fake_service();
    let p = service
        .prepare(request(
            Host::Claude,
            Action::Enable,
            "demo@skills-box-verification",
        ))
        .await
        .unwrap();
    service
        .write_record(&OperationRecord {
            phase: "writing".into(),
            preview: p,
            outcome: None,
            message: None,
        })
        .unwrap();
    let page = service.read(Host::Claude).await.unwrap();
    assert!(page.snapshot.is_some());
    let record = page.operation.unwrap();
    assert_eq!(record.phase, "interrupted");
    assert!(record.outcome.is_none());
}

#[test]
#[ignore = "real CLI Git sync/update, ref/sparse and JSON sources via loopback only"]
fn native_adapters_git_and_source_contracts() {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    struct Server(std::process::Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for (host, exe) in [
        (Host::Codex, "/opt/homebrew/bin/codex"),
        (Host::Claude, "/Users/sunven/.local/bin/claude"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let base = Runtime {
            host,
            home: temp.path().into(),
            root: temp.path().into(),
            executable: exe.into(),
            cwd: temp.path().into(),
            env: std::collections::BTreeMap::from([(
                "PATH".into(),
                "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".into(),
            )]),
            isolated: true,
        };
        let rt = contracts::isolated_runtime(&base, temp.path()).unwrap();
        let market = contracts::fixture(temp.path()).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("/usr/bin/git")
                .arg("-C")
                .arg(&market)
                .args(args)
                .env_clear()
                .envs(&rt.env)
                .env("HOME", &rt.home)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "v1",
        ]);
        let bare = temp.path().join("market.git");
        git(&[
            "clone",
            "--bare",
            market.to_str().unwrap(),
            bare.to_str().unwrap(),
        ]);
        let mut server = Server(
            Command::new("/usr/bin/python3")
                .arg("-u")
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/native-plugins/serve-git.py"
                ))
                .arg(temp.path())
                .env_clear()
                .envs(&rt.env)
                .env("HOME", &rt.home)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let mut port = String::new();
        std::io::BufReader::new(server.0.stdout.take().unwrap())
            .read_line(&mut port)
            .unwrap();
        let address = format!("http://127.0.0.1:{}/market.git", port.trim());
        let contract = contracts::shipped(host, &rt.version().unwrap());
        let load = || {
            let mut o = inventory::load(&rt, contract.capabilities.clone(), &[]).unwrap();
            o.snapshot.source_kinds = contract.sources.clone();
            o
        };
        let perform = |r: Request| {
            let before = load();
            let output = operations::execute(&rt, &r, &before).unwrap();
            let after = load();
            assert!(
                operations::response_matches(&r, &output),
                "{host:?} {:?}: {}",
                r.action,
                output.diagnostic
            );
            assert!(
                operations::postcondition(&r, &before, &after, &output),
                "{host:?} {:?}: {}",
                r.action,
                format!(
                    "{}\n{}",
                    output.diagnostic,
                    serde_json::to_string_pretty(&after.snapshot).unwrap()
                )
            );
            after
        };
        let mut add = request(host, Action::AddMarket, "");
        add.source = address;
        add.source_kind = "git".into();
        add.reference = "main".into();
        add.sparse_paths = vec![".claude-plugin".into(), "demo".into()];
        perform(add);
        perform(request(
            host,
            Action::Install,
            "demo@skills-box-verification",
        ));
        perform(request(
            host,
            Action::Disable,
            "demo@skills-box-verification",
        ));
        fs::write(
            market.join("demo/.claude-plugin/plugin.json"),
            json!({"name":"demo","version":"2.0.0"}).to_string(),
        )
        .unwrap();
        fs::write(
            market.join("demo/skills/hello/SKILL.md"),
            "---\nname: hello\ndescription: Updated fixture\n---\nVersion 2\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "v2",
        ]);
        git(&["push", bare.to_str().unwrap(), "main"]);
        let after_sync = perform(request(host, Action::SyncMarket, "skills-box-verification"));
        assert_eq!(after_sync.snapshot.plugins[0].user_enabled, Some(false));
        let updated = if host == Host::Codex {
            after_sync
        } else {
            perform(request(
                host,
                Action::Update,
                "demo@skills-box-verification",
            ))
        };
        assert_eq!(
            updated.snapshot.plugins[0].version.as_deref(),
            Some("2.0.0")
        );
        assert_eq!(updated.snapshot.plugins[0].user_enabled, Some(false));
        let plugin_root = updated.snapshot.plugins[0].local_root.as_ref().unwrap();
        assert!(fs::read_to_string(
            std::path::Path::new(plugin_root).join("skills/hello/SKILL.md")
        )
        .unwrap()
        .contains("Version 2"));
        perform(request(
            host,
            Action::Uninstall,
            "demo@skills-box-verification",
        ));
        perform(request(
            host,
            Action::RemoveMarket,
            "skills-box-verification",
        ));
        if host == Host::Claude {
            let mut file = request(host, Action::AddMarket, "");
            file.source_kind = "local".into();
            file.source = market
                .join(".claude-plugin/marketplace.json")
                .to_string_lossy()
                .into();
            assert!(perform(file).snapshot.markets[0].catalog_error.is_none());
            perform(request(host, Action::SyncMarket, "skills-box-verification"));
            perform(request(
                host,
                Action::Install,
                "demo@skills-box-verification",
            ));
            perform(request(
                host,
                Action::Uninstall,
                "demo@skills-box-verification",
            ));
            perform(request(
                host,
                Action::RemoveMarket,
                "skills-box-verification",
            ));
            let mut url = request(host, Action::AddMarket, "");
            url.source_kind = "url".into();
            url.source = format!(
                "http://127.0.0.1:{}/market/.claude-plugin/marketplace.json",
                port.trim()
            );
            assert!(perform(url).snapshot.markets[0].catalog_error.is_none());
            perform(request(host, Action::SyncMarket, "skills-box-verification"));
            perform(request(
                host,
                Action::RemoveMarket,
                "skills-box-verification",
            ));
        }
    }
}

#[test]
fn nested_origins_require_every_user_owned_leaf() {
    let path = std::path::Path::new("/fixture/config.toml");
    let user = json!({"marketplaces":{"team":{"source":"https://example.invalid/market.git","sparse_paths":["catalog","plugins"]}}});
    let origin = json!({"name":{"type":"user","profile":null,"file":path}});
    let mut config = json!({"config":user,"origins":{"marketplaces.team.source":origin,"marketplaces.team.sparse_paths.0":origin,"marketplaces.team.sparse_paths.1":origin}});
    assert!(inventory::owns(
        &config,
        &user,
        "marketplaces",
        "team",
        path
    ));
    config["origins"]["marketplaces.team.sparse_paths.1"]["name"]["type"] = json!("system");
    assert!(!inventory::owns(
        &config,
        &user,
        "marketplaces",
        "team",
        path
    ));
    config["origins"]
        .as_object_mut()
        .unwrap()
        .remove("marketplaces.team.sparse_paths.1");
    assert!(!inventory::owns(
        &config,
        &user,
        "marketplaces",
        "team",
        path
    ));
}

#[test]
fn native_install_policies_and_unverified_sources_are_blocked() {
    for source in [
        json!({"source":"command","command":"fixture"}),
        json!({"source":"npm","package":"fixture"}),
        json!({"source":"archive","url":"https://example.invalid/plugin.zip"}),
    ] {
        assert!(inventory::install_restriction(Host::Claude, &json!({"source":source})).is_some());
    }
    assert!(inventory::install_restriction(Host::Codex,&json!({"source":{"source":"local","path":"./demo"},"policy":{"installation":"NOT_AVAILABLE"}})).is_some());
    assert!(inventory::install_restriction(
        Host::Codex,
        &json!({"source":{"source":"local","path":"./demo"},"policy":{"installation":"AVAILABLE"}})
    )
    .is_none());
    let text = transport::diagnostic(
        r#"{"url":"https://person:credential@example.invalid/path?token=secret#secret","error":"denied"}"#,
    );
    assert!(!text.contains("credential"));
    assert!(!text.contains("secret"));
    assert!(!text.contains("person"));
    assert!(text.contains("example.invalid/path"));
}

#[tokio::test]
async fn local_source_changes_invalidate_confirmation() {
    let (temp, service, rt) = fake_service();
    let source = contracts::fixture(&temp.path().join("new-source")).unwrap();
    let mut request = request(Host::Claude, Action::AddMarket, "");
    request.source_kind = "local".into();
    request.source = source.to_string_lossy().into();
    let preview = service.prepare(request).await.unwrap();
    fs::write(
        source.join(".claude-plugin/marketplace.json"),
        json!({"name":"changed-market","plugins":[]}).to_string(),
    )
    .unwrap();
    let outcome = service.execute(preview.token).await.unwrap();
    assert_eq!(outcome.status, "conflict");
    assert!(!outcome.executed);
    assert!(!rt.root.join("started").exists());
}

#[tokio::test]
async fn unreadable_catalog_and_native_list_keep_other_markets_visible() {
    let (temp, service, rt) = fake_service();
    let mut settings = inventory::optional_json(&rt.root.join("settings.json")).unwrap();
    let mut known =
        inventory::optional_json(&rt.root.join("plugins/known_marketplaces.json")).unwrap();
    let missing = temp.path().join("missing-market");
    settings["extraKnownMarketplaces"]["broken"] =
        json!({"source":{"source":"directory","path":missing}});
    known["broken"] =
        json!({"source":{"source":"directory","path":missing},"installLocation":missing});
    fs::write(rt.root.join("settings.json"), settings.to_string()).unwrap();
    fs::write(
        rt.root.join("plugins/known_marketplaces.json"),
        known.to_string(),
    )
    .unwrap();
    let state = service.read(Host::Claude).await.unwrap();
    let snapshot = state.snapshot.unwrap();
    assert_eq!(snapshot.markets.len(), 2);
    assert!(snapshot
        .markets
        .iter()
        .find(|m| m.name == "broken")
        .unwrap()
        .catalog_error
        .is_some());
    assert_eq!(snapshot.plugins[0].installation_state, "installed");
    fs::write(rt.root.join("fail-read"), "").unwrap();
    let snapshot = service.read(Host::Claude).await.unwrap().snapshot.unwrap();
    assert_eq!(snapshot.markets.len(), 2);
    assert_eq!(snapshot.plugins[0].installation_state, "installed");
    assert!(snapshot
        .capabilities
        .iter()
        .filter(|c| c.action != Action::AddMarket)
        .all(|c| c.blocked_reason.is_some()));
    assert!(snapshot.warnings.iter().any(|w| w.contains("读取失败")));
}

#[test]
#[ignore = "real isolated Codex native catalog and Claude approval boundary"]
fn native_adapters_native_catalog_and_approval() {
    for (host, exe) in [
        (Host::Codex, "/opt/homebrew/bin/codex"),
        (Host::Claude, "/Users/sunven/.local/bin/claude"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let base = Runtime {
            host,
            home: temp.path().into(),
            root: temp.path().into(),
            executable: exe.into(),
            cwd: temp.path().into(),
            env: std::collections::BTreeMap::new(),
            isolated: true,
        };
        let rt = contracts::isolated_runtime(&base, temp.path()).unwrap();
        let market = contracts::fixture(temp.path()).unwrap();
        if host == Host::Codex {
            fs::create_dir_all(market.join(".agents/plugins")).unwrap();
            fs::remove_file(market.join(".claude-plugin/marketplace.json")).unwrap();
            fs::write(market.join(".agents/plugins/marketplace.json"),json!({"name":"skills-box-verification","plugins":[{"name":"demo","source":{"source":"local","path":"./demo"},"policy":{"installation":"AVAILABLE","authentication":"ON_INSTALL"}}]}).to_string()).unwrap();
        } else {
            fs::write(market.join(".claude-plugin/marketplace.json"),json!({"name":"skills-box-verification","owner":{"name":"Fixture"},"plugins":[{"name":"demo","source":{"source":"command","command":"/usr/bin/false"}}]}).to_string()).unwrap();
        }
        let out = rt
            .run(
                &[
                    "plugin".into(),
                    "marketplace".into(),
                    "add".into(),
                    market.to_string_lossy().into(),
                    "--json".into(),
                ],
                true,
            )
            .unwrap();
        assert!(out.success, "{}", out.diagnostic);
        let contract = contracts::shipped(host, &rt.version().unwrap());
        let obs = inventory::load(&rt, contract.capabilities.clone(), &[]).unwrap();
        assert_eq!(obs.snapshot.plugins.len(), 1);
        let install = request(host, Action::Install, "demo@skills-box-verification");
        if host == Host::Codex {
            assert!(obs.snapshot.plugins[0].install_blocked_reason.is_none());
            assert!(obs.snapshot.plugins[0].local_root.is_some());
            let out = operations::execute(&rt, &install, &obs).unwrap();
            let after = inventory::load(&rt, contract.capabilities, &[]).unwrap();
            assert!(operations::response_matches(&install, &out));
            assert!(operations::postcondition(&install, &obs, &after, &out));
        } else {
            assert!(operations::arguments(&install, &obs).is_err());
            // Invoke without any approval flag to prove the CLI refuses. The
            // fixture command is /usr/bin/false and cannot alter user data.
            let out = rt
                .run(
                    &[
                        "plugin".into(),
                        "install".into(),
                        install.target.clone(),
                        "--scope".into(),
                        "user".into(),
                        "--json".into(),
                    ],
                    true,
                )
                .unwrap();
            assert!(!out.success, "{}", out.diagnostic);
            let after = inventory::load(&rt, contract.capabilities, &[]).unwrap();
            let outcome = operations::outcome(preview(install), &obs, Ok(after), Ok(out));
            assert!(
                matches!(outcome.status.as_str(), "needs_host" | "partial"),
                "{}: {}",
                outcome.status,
                outcome.message
            );
            assert!(outcome.message.contains("批准"), "{}", outcome.message);
        }
    }
}

#[test]
#[ignore = "real isolated compatibility probe; no user configuration"]
fn native_adapters_compatibility_probe() {
    for (host, exe) in [
        (Host::Codex, "/opt/homebrew/bin/codex"),
        (Host::Claude, "/Users/sunven/.local/bin/claude"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let rt = Runtime {
            host,
            home: temp.path().into(),
            root: temp.path().into(),
            executable: exe.into(),
            cwd: temp.path().into(),
            env: std::collections::BTreeMap::new(),
            isolated: true,
        };
        let result = contracts::verify(&rt).unwrap();
        for capability in result.capabilities {
            assert_eq!(
                capability.blocked_reason.is_some(),
                matches!(capability.action, Action::SyncMarket | Action::RemoveMarket),
                "{host:?} {:?}: {:?}",
                capability.action,
                capability.blocked_reason
            );
        }
        assert_eq!(result.sources, vec!["local"]);
    }
}

#[test]
#[ignore = "real official config override response using a temporary CLI wrapper"]
fn native_adapters_codex_override() {
    let temp = tempfile::tempdir().unwrap();
    let wrapper = temp.path().join("codex-with-override");
    fs::write(&wrapper,"#!/bin/sh\nexec /opt/homebrew/bin/codex -c 'plugins={\"demo@skills-box-verification\"={enabled=false}}' \"$@\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let base = Runtime {
        host: Host::Codex,
        home: temp.path().into(),
        root: temp.path().into(),
        executable: wrapper,
        cwd: temp.path().into(),
        env: std::collections::BTreeMap::new(),
        isolated: true,
    };
    let rt = contracts::isolated_runtime(&base, temp.path()).unwrap();
    fs::write(
        rt.root.join("config.toml"),
        "[plugins.\"demo@skills-box-verification\"]\nenabled=true\n",
    )
    .unwrap();
    let contract = contracts::shipped(Host::Codex, &rt.version().unwrap());
    let obs = inventory::load(&rt, contract.capabilities, &[]).unwrap();
    assert!(obs.snapshot.plugins[0].read_only_reason.is_some());
    assert!(operations::arguments(
        &request(Host::Codex, Action::Enable, "demo@skills-box-verification"),
        &obs
    )
    .is_err());
    let response=rt.rpc("config/value/write",json!({"keyPath":"plugins.\"demo@skills-box-verification\".enabled","value":true,"mergeStrategy":"replace","filePath":rt.root.join("config.toml"),"expectedVersion":obs.user_version})).unwrap();
    assert_eq!(response["status"], "okOverridden");
    assert_eq!(
        rt.rpc("config/read", json!({"includeLayers":true}))
            .unwrap()["config"]["plugins"]["demo@skills-box-verification"]["enabled"],
        false
    );
}

#[tokio::test]
async fn missing_cli_is_an_error_instead_of_empty_inventory() {
    let (_temp, service, mut rt) = fake_service();
    rt.executable = rt.home.join("missing-cli");
    *service.0.runtime.lock().unwrap() = Some(rt.clone());
    let page = service.read(Host::Claude).await.unwrap();
    assert!(page.snapshot.is_none());
    assert!(page.error.is_some());
    assert!(service
        .prepare(request(
            Host::Claude,
            Action::Enable,
            "demo@skills-box-verification"
        ))
        .await
        .is_err());
    assert!(!rt.root.join("started").exists());
}

#[test]
#[ignore = "real native network failure against a closed loopback port"]
fn native_adapters_network_failure() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    for (host, exe) in [
        (Host::Codex, "/opt/homebrew/bin/codex"),
        (Host::Claude, "/Users/sunven/.local/bin/claude"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let base = Runtime {
            host,
            home: temp.path().into(),
            root: temp.path().into(),
            executable: exe.into(),
            cwd: temp.path().into(),
            env: std::collections::BTreeMap::new(),
            isolated: true,
        };
        let rt = contracts::isolated_runtime(&base, temp.path()).unwrap();
        let contract = contracts::shipped(host, &rt.version().unwrap());
        let mut before = inventory::load(&rt, contract.capabilities.clone(), &[]).unwrap();
        before.snapshot.source_kinds = contract.sources;
        let mut r = request(host, Action::AddMarket, "");
        r.source_kind = "git".into();
        r.source = format!("http://127.0.0.1:{port}/unreachable.git");
        let out = operations::execute(&rt, &r, &before).unwrap();
        assert!(!out.success);
        let after = inventory::load(&rt, contract.capabilities, &[]).unwrap();
        let outcome = operations::outcome(preview(r), &before, Ok(after), Ok(out));
        assert_ne!(outcome.status, "succeeded");
        assert_eq!(outcome.command_succeeded, Some(false));
        assert!(outcome.snapshot.unwrap().markets.is_empty());
    }
}

#[tokio::test]
async fn unknown_mutation_contract_disables_only_affected_action() {
    let (_temp, service, rt) = fake_service();
    fs::write(rt.root.join("unknown-response"), "").unwrap();
    let p = service
        .prepare(request(
            Host::Claude,
            Action::Enable,
            "demo@skills-box-verification",
        ))
        .await
        .unwrap();
    let outcome = service.execute(p.token).await.unwrap();
    assert_eq!(outcome.status, "unconfirmed");
    let page = service.read(Host::Claude).await.unwrap();
    let caps = page.snapshot.unwrap().capabilities;
    assert!(caps
        .iter()
        .find(|c| c.action == Action::Enable)
        .unwrap()
        .blocked_reason
        .is_some());
    assert!(caps
        .iter()
        .find(|c| c.action == Action::Disable)
        .unwrap()
        .blocked_reason
        .is_none());
    assert!(service
        .prepare(request(
            Host::Claude,
            Action::Enable,
            "demo@skills-box-verification"
        ))
        .await
        .is_err());
}
