//! Native processes are owned by the worker until reaped. Writes have no UI timeout or cancellation.
use super::model::Host;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Runtime {
    pub host: Host,
    pub home: PathBuf,
    pub root: PathBuf,
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub isolated: bool,
}
pub struct Output {
    pub success: bool,
    pub value: Option<Value>,
    pub diagnostic: String,
}
impl Runtime {
    pub fn discover(host: Host, paths: &crate::paths::AppPaths) -> Result<Self, String> {
        let home = paths.home().to_path_buf();
        let key = if host == Host::Codex {
            "CODEX_HOME"
        } else {
            "CLAUDE_CONFIG_DIR"
        };
        let root = std::env::var_os(key)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                home.join(if host == Host::Codex {
                    ".codex"
                } else {
                    ".claude"
                })
            });
        if !root.is_absolute() {
            return Err(format!("{key} 必须是绝对路径。"));
        }
        let root = match fs::canonicalize(&root) {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => root,
            Err(e) => return Err(format!("无法访问原生配置目录：{e}")),
        };
        let mut dirs: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .filter(|p| p.is_absolute())
                .collect();
        dirs.extend([
            home.join(".local/bin"),
            home.join(".npm-global/bin"),
            home.join(".volta/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ]);
        let mut executable = dirs
            .iter()
            .map(|p| p.join(host.command()))
            .find(|p| executable_file(p));
        if executable.is_none() {
            let shell = std::env::var_os("SHELL")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/bin/zsh"));
            let output = Command::new(shell)
                .args(["-ilc", "printf '\\n__SKILLS_BOX_PATH__%s\\n' \"$PATH\""])
                .current_dir("/")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            // Login shell is a read-only probe; it may be stopped on timeout.
            let mut child = output;
            let start = Instant::now();
            while child.try_wait().map_err(|e| e.to_string())?.is_none() {
                if start.elapsed() > Duration::from_secs(5) {
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Some(path) = line.strip_prefix("__SKILLS_BOX_PATH__") {
                    dirs.extend(std::env::split_paths(path).filter(|p| p.is_absolute()));
                }
            }
            executable = dirs
                .iter()
                .map(|p| p.join(host.command()))
                .find(|p| executable_file(p));
        }
        let executable = executable
            .ok_or_else(|| format!("未找到 {} CLI，请安装后重新检测。", host.command()))?;
        let executable =
            fs::canonicalize(executable).map_err(|e| format!("无法解析原生 CLI 路径：{e}"))?;
        let mut env = BTreeMap::new();
        env.insert(
            "PATH".into(),
            std::env::join_paths(dirs)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into(),
        );
        Ok(Self {
            host,
            home,
            root,
            executable,
            cwd: PathBuf::from("/"),
            env,
            isolated: false,
        })
    }
    fn command(&self, args: &[String]) -> Command {
        let mut c = Command::new(&self.executable);
        if self.isolated {
            c.env_clear();
        }
        c.args(args)
            .current_dir(&self.cwd)
            .envs(&self.env)
            .env("HOME", &self.home)
            .env(
                if self.host == Host::Codex {
                    "CODEX_HOME"
                } else {
                    "CLAUDE_CONFIG_DIR"
                },
                &self.root,
            )
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("SSH_ASKPASS_REQUIRE", "never")
            .env("DISABLE_AUTOUPDATER", "1")
            .stdin(Stdio::null());
        c
    }
    pub fn run(&self, args: &[String], write: bool) -> Result<Output, String> {
        let mut p = Process::start(self.command(args), false)?;
        let start = Instant::now();
        let success = loop {
            if let Some(s) = p.child.try_wait().map_err(|e| e.to_string())? {
                break s.success();
            }
            if !write && start.elapsed() > Duration::from_secs(30) {
                let _ = p.child.kill();
                let _ = p.child.wait();
                return Err("读取原生状态超时，请重新读取。".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let stdout = read(&mut p.stdout)?;
        let stderr = read(&mut p.stderr)?;
        let value = serde_json::from_str(&stdout).ok().or_else(|| {
            stdout
                .lines()
                .rev()
                .find_map(|s| serde_json::from_str(s).ok())
        });
        Ok(Output {
            success,
            value,
            diagnostic: diagnostic(&format!("{stdout}\n{stderr}")),
        })
    }
    pub fn json(&self, args: &[&str]) -> Result<Value, String> {
        let out = self.run(
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            false,
        )?;
        if !out.success {
            return Err(out.diagnostic);
        }
        out.value
            .ok_or("原生 CLI 返回了未知格式，相关操作暂不可用。".into())
    }
    pub fn version(&self) -> Result<String, String> {
        let out = self.run(&["--version".into()], false)?;
        if !out.success {
            return Err(out.diagnostic);
        }
        out.diagnostic
            .split_whitespace()
            .find(|s| semver::Version::parse(s).is_ok())
            .map(str::to_string)
            .ok_or("无法识别 CLI 版本。".into())
    }
    pub fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut p = Process::start(self.command(&["app-server".into()]), true)?;
        let mut writing = false;
        let result = (|| {
            p.request(
                1,
                "initialize",
                json!({"clientInfo":{"name":"skills_box","version":env!("CARGO_PKG_VERSION")}}),
                false,
            )?;
            p.send(json!({"method":"initialized"}))?;
            writing = method == "config/value/write";
            p.request(2, method, params, method == "config/value/write")
        })();
        // No thread is started. Once the response arrives the config write has ended.
        // Close the protocol, then reap the idle server, never a pending writer.
        p.child.stdin.take();
        if !writing {
            let _ = p.child.kill();
        }
        let _ = p.child.wait();
        result
    }
}
fn executable_file(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}
struct Process {
    child: Child,
    stdout: fs::File,
    stderr: fs::File,
}
impl Process {
    fn start(mut command: Command, input: bool) -> Result<Self, String> {
        let stdout = tempfile::tempfile().map_err(|e| e.to_string())?;
        let stderr = tempfile::tempfile().map_err(|e| e.to_string())?;
        command
            .stdout(stdout.try_clone().map_err(|e| e.to_string())?)
            .stderr(stderr.try_clone().map_err(|e| e.to_string())?);
        if input {
            command.stdin(Stdio::piped());
        }
        Ok(Self {
            child: command
                .spawn()
                .map_err(|e| format!("无法运行原生 CLI：{e}"))?,
            stdout,
            stderr,
        })
    }
    fn send(&mut self, v: Value) -> Result<(), String> {
        let input = self.child.stdin.as_mut().ok_or("原生协议已断开")?;
        writeln!(input, "{v}")
            .and_then(|_| input.flush())
            .map_err(|e| e.to_string())
    }
    fn request(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
        write: bool,
    ) -> Result<Value, String> {
        self.send(json!({"id":id,"method":method,"params":params}))?;
        let start = Instant::now();
        loop {
            for line in read(&mut self.stdout)?.lines() {
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    if v["id"] == id {
                        if let Some(result) = v.get("result") {
                            return Ok(result.clone());
                        }
                        if let Some(error) = v.get("error") {
                            return Err(diagnostic(&error.to_string()));
                        }
                    }
                }
            }
            if self.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Err("原生配置协议异常结束，结果需要重新核对。".into());
            }
            if !write && start.elapsed() > Duration::from_secs(30) {
                return Err("原生配置读取超时。".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        // Retain ownership even on unexpected I/O errors.
        self.child.stdin.take();
        let _ = self.child.wait();
    }
}
fn read(f: &mut fs::File) -> Result<String, String> {
    f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    f.take(4 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into())
}
pub fn redact_urls(s: &str) -> String {
    // URLs also occur inside JSON and quoted native diagnostics.
    let mut rest = s;
    let mut result = String::new();
    while let Some(start) = ["https://", "http://", "ssh://"]
        .iter()
        .filter_map(|scheme| rest.find(scheme))
        .min()
    {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\'))
            .unwrap_or(rest.len());
        let candidate = &rest[..end];
        if let Ok(mut url) = reqwest::Url::parse(candidate) {
            if url.scheme() != "ssh" {
                let _ = url.set_username("");
            }
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            result.push_str(url.as_str());
        } else {
            result.push_str("[来源 URL 已隐藏]");
        }
        rest = &rest[end..];
    }
    result.push_str(rest);
    result
}
pub fn diagnostic(s: &str) -> String {
    redact_urls(s)
        .split_whitespace()
        .map(|word| {
            if word.to_lowercase().contains("token=") || word.to_lowercase().contains("api_key=") {
                return "[已隐藏]".into();
            }
            word.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(1800)
        .collect()
}
pub fn same_path(a: &Path, b: &Path) -> bool {
    fs::canonicalize(a).unwrap_or_else(|_| a.into())
        == fs::canonicalize(b).unwrap_or_else(|_| b.into())
}
