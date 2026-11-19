from pathlib import Path
base=Path(__file__).with_name('probe.py').read_text().split('for host in ["codex", "claude"]:')[0]
exec(base.replace('probe-results.json','git-results.json'))
def git(*args):
    p=subprocess.run(['git','-C',str(market),*args],env=env,capture_output=True,text=True); assert p.returncode==0,p.stderr

git('init','-b','main');git('add','.');git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-m','v1')
# Serve a local bare repo over HTTP to exercise actual Git source classification without credentials.
import http.server, threading
bare=root/'market.git';subprocess.run(['git','clone','--bare',str(market),str(bare)],env=env,capture_output=True,check=True)
subprocess.run(['git','-C',str(bare),'update-server-info'],env=env,check=True)
class Quiet(http.server.SimpleHTTPRequestHandler):
    def __init__(self,*a,**k):super().__init__(*a,directory=str(root),**k)
    def log_message(self,*a):pass
    def git_http(self):
        from urllib.parse import urlsplit
        url=urlsplit(self.path)
        e={**env,'GIT_PROJECT_ROOT':str(root),'GIT_HTTP_EXPORT_ALL':'1','PATH_INFO':url.path,'QUERY_STRING':url.query,'REQUEST_METHOD':self.command,'CONTENT_TYPE':self.headers.get('Content-Type',''),'CONTENT_LENGTH':self.headers.get('Content-Length','0'),'REMOTE_ADDR':'127.0.0.1'}
        body=self.rfile.read(int(e['CONTENT_LENGTH']))
        p=subprocess.run(['git','http-backend'],env=e,input=body,capture_output=True)
        headers,data=p.stdout.split(b'\r\n\r\n',1)
        self.send_response(200)
        for line in headers.split(b'\r\n'):
            key,value=line.decode().split(':',1)
            if key!='Status':self.send_header(key,value.strip())
        self.end_headers();self.wfile.write(data)
    def do_GET(self):
        if '/market.git/' in self.path:self.git_http()
        else:super().do_GET()
    def do_POST(self):self.git_http()
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Quiet);threading.Thread(target=server.serve_forever,daemon=True).start()
url=f'http://127.0.0.1:{server.server_port}/market.git'
for host in ['codex','claude']:
    args=['plugin','marketplace','add',url,'--json']
    args += ['--ref','main','--sparse','.claude-plugin','--sparse','demo'] if host=='codex' else ['--sparse','.claude-plugin','demo']
    run(host,args)
    run(host,['plugin','add' if host=='codex' else 'install','demo@skills-box-test','--json'])
    if host=='codex':rpc('config/value/write',{'keyPath':'plugins."demo@skills-box-test".enabled','value':False,'mergeStrategy':'replace','filePath':str(home/'.codex/config.toml')})
    else:run(host,['plugin','disable','demo@skills-box-test','--scope','user','--json'])
(market/'demo/.claude-plugin/plugin.json').write_text(json.dumps({'name':'demo','version':'2.0.0'}))
(market/'demo/skills/hello/SKILL.md').write_text('---\nname: hello\ndescription: Changed fixture\n---\nVersion 2.\n')
git('add','.');git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-m','v2');git('push',str(bare),'main')
subprocess.run(['git','-C',str(bare),'update-server-info'],env=env,check=True)
for host in ['codex','claude']:
    run(host,['plugin','marketplace','upgrade' if host=='codex' else 'update','skills-box-test','--json'])
    run(host,['plugin','list','--json'])
    if host=='claude':run(host,['plugin','update','demo@skills-box-test','--scope','user','--json'])
    run(host,['plugin','list','--json'])
    report[host+'_files']={str(p.relative_to(home)):p.read_text() for p in home.rglob('SKILL.md') if f'.{host}' in str(p) and '.system' not in p.parts};save()
    run(host,['plugin','marketplace','remove','skills-box-test','--json'])
# JSON URL source uses absolute plugin path for local static fixture.
run('claude',['plugin','marketplace','add',f'http://127.0.0.1:{server.server_port}/market/.claude-plugin/marketplace.json','--scope','user','--json'])
run('claude',['plugin','marketplace','list','--json'])
server.shutdown()
