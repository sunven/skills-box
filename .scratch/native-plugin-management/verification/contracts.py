from pathlib import Path
base = Path(__file__).with_name('probe.py').read_text().split('for host in ["codex", "claude"]:')[0]
exec(base.replace('probe-results.json','contracts-results.json'))
pid='demo@skills-box-test'
run('codex',['plugin','marketplace','add',str(market),'--json'])
run('codex',['plugin','add',pid,'--json'])
c=rpc('config/read',{'includeLayers':True})['result']
layer=next(l for l in c['layers'] if l['name']['type']=='user')
params={'keyPath':'plugins."'+pid+'".enabled','value':False,'mergeStrategy':'replace','filePath':str(home/'.codex/config.toml'),'expectedVersion':layer['version']}
rpc('config/value/write',params)
params['value']=True
rpc('config/value/write',params)
rpc('config/value/write',{'keyPath':'plugins."dotted.name@skills-box-test".enabled','value':False,'mergeStrategy':'replace','filePath':str(home/'.codex/config.toml')})
run('codex',['plugin','marketplace','upgrade','skills-box-test','--json'])
run('codex',['plugin','list','--json'])
run('codex',['plugin','marketplace','remove','skills-box-test','--json'])
report['orphan_config']=rpc('config/read',{'includeLayers':True})
report['orphan_files']={str(p.relative_to(home)):p.read_text()[:12000] for p in (home/'.codex/plugins').rglob('*.json') if p.stat().st_size<100000}
save()
run('claude',['plugin','marketplace','add',str(market),'--scope','user','--json'])
run('claude',['plugin','install',pid,'--scope','user','--json'])
project=root/'project'; project.mkdir(); subprocess.run(['git','init',str(project)],env=env,capture_output=True)
run('claude',['plugin','install',pid,'--scope','project','--json'],project)
report['claude_before_remove']={str(p.relative_to(home)):p.read_text() for p in (home/'.claude').rglob('*.json') if 'cache' not in str(p) and p.stat().st_size<100000};save()
run('claude',['plugin','marketplace','remove','skills-box-test','--scope','user','--json'])
run('claude',['plugin','list','--json'])
report['claude_after_remove']={str(p.relative_to(home)):p.read_text() for p in (home/'.claude').rglob('*.json') if 'cache' not in str(p) and p.stat().st_size<100000};save()
# Local file marketplace registration.
run('claude',['plugin','marketplace','add',str(market/'.claude-plugin/marketplace.json'),'--scope','user','--json'])
run('claude',['plugin','marketplace','list','--json'])
print('Root',root,flush=True)
