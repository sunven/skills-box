#!/usr/bin/env python3
"""Deterministic native transport fixture, reads only its isolated config root."""
import json, os, sys, time
from pathlib import Path
codex='CODEX_HOME' in os.environ
root=Path(os.environ['CODEX_HOME' if codex else 'CLAUDE_CONFIG_DIR'])
a=sys.argv[1:]
def read(name):return json.loads((root/name).read_text())
if codex:
    if a==['--version']:
        print('codex-cli 0.170.0')
    elif a==['plugin','marketplace','list','--json']:
        print(json.dumps(read('marketplaces.json')))
    elif a==['plugin','list','--json']:
        print(json.dumps(read('installed.json')))
    elif a==['app-server']:
        for line in sys.stdin:
            request=json.loads(line)
            if request['method']=='initialize':result={}
            elif request['method']=='initialized':continue
            elif request['method']=='config/read':result={'config':{},'layers':[],'origins':{}}
            else:sys.exit(1)
            print(json.dumps({'id':request['id'],'result':result}),flush=True)
    else:
        sys.exit(1)
    sys.exit(0)
if a==['--version']:
    print('2.1.323 (Claude Code)')
elif a==['plugin','marketplace','list','--json']:
    known=read('plugins/known_marketplaces.json')
    print(json.dumps([{'name':name,**row['source'],'installLocation':row['installLocation']} for name,row in known.items()]))
elif a==['plugin','list','--json']:
    if (root/'fail-read').exists():sys.exit(1)
    settings=read('settings.json')
    rows=[{'id':id,**row,'enabled':settings['enabledPlugins'].get(id)} for id,entries in read('plugins/installed_plugins.json')['plugins'].items() for row in entries]
    if (root/'cli-only-plugins.json').exists():rows=read('cli-only-plugins.json')+rows
    print(json.dumps(rows))
elif a[:2]==['plugin','enable']:
    (root/'started').write_text('started')
    while (root/'hold').exists():time.sleep(.025)
    settings=read('settings.json');settings['enabledPlugins'][a[2]]=True
    (root/'settings.json').write_text(json.dumps(settings))
    if (root/'fail-after-write').exists():
        print(json.dumps({'command':'enable','outcome':'failed','pluginId':a[2],'scope':'user'}));sys.exit(1)
    if (root/'fail-verification').exists():(root/'fail-read').touch()
    if (root/'unknown-response').exists():
        print(json.dumps({'unknown':True}));sys.exit(0)
    print(json.dumps({'command':'enable','outcome':'ok','pluginId':a[2],'scope':'user'}))
else:
    print('unsupported fixture command',a,file=sys.stderr);sys.exit(1)
