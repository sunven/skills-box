"""Loopback smart Git/JSON server for opt-in native adapter tests."""
import http.server, os, subprocess, sys
from urllib.parse import urlsplit
root=sys.argv[1]
class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self,*args,**kwargs):super().__init__(*args,directory=root,**kwargs)
    def log_message(self,*args):pass
    def git_http(self):
        url=urlsplit(self.path)
        env={**os.environ,'GIT_PROJECT_ROOT':root,'GIT_HTTP_EXPORT_ALL':'1','PATH_INFO':url.path,'QUERY_STRING':url.query,'REQUEST_METHOD':self.command,'CONTENT_TYPE':self.headers.get('Content-Type',''),'CONTENT_LENGTH':self.headers.get('Content-Length','0'),'REMOTE_ADDR':'127.0.0.1'}
        p=subprocess.run(['git','http-backend'],env=env,input=self.rfile.read(int(env['CONTENT_LENGTH'])),capture_output=True)
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
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
print(server.server_port,flush=True)
server.serve_forever()
