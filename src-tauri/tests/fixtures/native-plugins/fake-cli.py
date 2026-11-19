#!/usr/bin/env python3
"""Deterministic native transport fixture, never reads outside CLAUDE_CONFIG_DIR."""
import json, os, sys, time
from pathlib import Path
root=Path(os.environ['CLAUDE_CONFIG_DIR'])
a=sys.argv[1:]
def read(name):return json.loads((root/name).read_text())
if a==['--version']:
    print('2.1.323 (Claude Code)')
elif a==['plugin','marketplace','list','--json']:
    known=read('plugins/known_marketplaces.json')
    print(json.dumps([{'name':name,**row['source'],'installLocation':row['installLocation']} for name,row in known.items()]))
elif a==['plugin','list','--json']:
    if (root/'fail-read').exists():sys.exit(1)
    settings=read('settings.json')
    print(json.dumps([{'id':id,**row,'enabled':settings['enabledPlugins'].get(id)} for id,rows in read('plugins/installed_plugins.json')['plugins'].items() for row in rows]))
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
