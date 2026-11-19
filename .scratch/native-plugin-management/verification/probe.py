"""Exercise native CLIs using static fixtures and an isolated HOME only."""
import json
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import tempfile

root = Path(tempfile.mkdtemp(prefix="skills-box-plugins-"))
home = root / "home"
home.mkdir()
market = root / "market"
(market / ".claude-plugin").mkdir(parents=True)
(market / "demo/.claude-plugin").mkdir(parents=True)
(market / "demo/skills/hello").mkdir(parents=True)
(market / ".claude-plugin/marketplace.json").write_text(json.dumps({"name":"skills-box-test","owner":{"name":"Fixture"},"plugins":[{"name":"demo","source":"./demo","description":"Static fixture"}]}))
(market / "demo/.claude-plugin/plugin.json").write_text(json.dumps({"name":"demo","version":"1.0.0","description":"Static fixture"}))
(market / "demo/skills/hello/SKILL.md").write_text("---\nname: hello\ndescription: Static fixture\n---\nNo executable content.\n")
env = {"PATH": "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin", "LANG": "en_US.UTF-8"}
env.update(HOME=str(home), CODEX_HOME=str(home / ".codex"), CLAUDE_CONFIG_DIR=str(home / ".claude"), XDG_CONFIG_HOME=str(home/".config"), GIT_TERMINAL_PROMPT="0", GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1", DISABLE_AUTOUPDATER="1")
for key in ["CODEX_HOME", "CLAUDE_CONFIG_DIR", "XDG_CONFIG_HOME"]:
    Path(env[key]).mkdir()
report = {"root":str(root), "calls":[]}
out = Path(__file__).with_name("probe-results.json")
def save():
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2))
def run(host, args, cwd=None):
    p = subprocess.run([shutil.which(host), *args], env=env, cwd=cwd or root, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
    record = {"host":host, "args":args, "code":p.returncode, "stdout":p.stdout, "stderr":p.stderr}
    report["calls"].append(record)
    save()
    print(host, " ".join(args), p.returncode, p.stdout[:180].replace("\n", " "), flush=True)
    return p
def rpc(method, params):
    p = subprocess.Popen([shutil.which("codex"), "app-server"], env=env, cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
    def request(i, name, data):
        p.stdin.write(json.dumps({"id":i,"method":name,"params":data})+"\n"); p.stdin.flush()
        selector = selectors.DefaultSelector(); selector.register(p.stdout, selectors.EVENT_READ)
        while selector.select(20):
            line = p.stdout.readline()
            if not line: break
            msg = json.loads(line)
            if msg.get("id") == i: return msg
        raise RuntimeError("RPC response timeout")
    try:
        request(1,"initialize",{"clientInfo":{"name":"skills_box_probe","version":"0.1.0"}})
        p.stdin.write('{"method":"initialized"}\n'); p.stdin.flush()
        msg = request(2,method,params)
        report["calls"].append({"host":"codex-rpc","method":method,"params":params,"response":msg}); save()
        print(method, json.dumps(msg)[:250], flush=True)
        return msg
    finally:
        p.stdin.close(); p.terminate(); p.wait(timeout=10)

for host in ["codex", "claude"]:
    run(host,["--version"])
    run(host,["plugin","marketplace","list","--json"])
    run(host,["plugin","marketplace","add",str(market),"--json"])
    run(host,["plugin","marketplace","list","--json"])
    run(host,["plugin","list","--available","--json"])
    run(host,["plugin","add" if host=="codex" else "install","demo@skills-box-test","--json"])
    run(host,["plugin","list","--available","--json"])
    if host == "codex":
        config = rpc("config/read",{"includeLayers":True})
        report["codex_config"] = config
        save()
        rpc("config/value/write",{"keyPath":"plugins.\"demo@skills-box-test\".enabled","value":False,"mergeStrategy":"replace","filePath":str(home/".codex/config.toml")})
        run(host,["plugin","list","--available","--json"])
        run(host,["plugin","add","demo@skills-box-test","--json"])
        run(host,["plugin","list","--available","--json"])
        run(host,["plugin","marketplace","remove","skills-box-test","--json"])
        run(host,["plugin","list","--json"])
        run(host,["plugin","remove","demo@skills-box-test","--json"])
    else:
        for action in ["disable","update","enable"]:
            run(host,["plugin",action,"demo@skills-box-test","--scope","user","--json"])
            run(host,["plugin","list","--json"])
        run(host,["plugin","marketplace","update","skills-box-test","--json"])
        run(host,["plugin","uninstall","demo@skills-box-test","--scope","user","--json"])
        run(host,["plugin","marketplace","remove","skills-box-test","--scope","user","--json"])
save()
print("Report:", out, flush=True)
