# Live HN/provider smoke test; makes paid model requests. Run with uv run --with pyte.
import argparse
import os, time, json, tempfile, re
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
EVIDENCE=Path(tempfile.mkdtemp(prefix="hn-live-evidence-"))
from terminal_capture import TUI


import urllib.request
from concurrent.futures import ThreadPoolExecutor
parser=argparse.ArgumentParser()
parser.add_argument('--provider',choices=['openrouter','anthropic'],default='openrouter')
provider=parser.parse_args().provider
key_name='ANTHROPIC_API_KEY' if provider=='anthropic' else 'OPENROUTER_API_KEY'
key=os.environ.get(key_name, "")
keyfile=ROOT/".env.local"
if not key and keyfile.exists():
    for line in keyfile.read_text().splitlines():
        if line.startswith(key_name+"="):
            key=line.split("=",1)[1].strip().strip("\"'")
assert key, f"Set {key_name} or add it to .env.local"
os.environ[key_name]=key
os.environ.pop('OPENROUTER_API_KEY' if provider=='anthropic' else 'ANTHROPIC_API_KEY',None)

def fetch(url):
    with urllib.request.urlopen(url,timeout=25) as response:return json.load(response)
item=fetch('https://hacker-news.firebaseio.com/v0/item/50150809.json')
assert item and item.get('url')
print('Live story:',item['id'],item['title'],item['url'],flush=True)
story=dict(id=item['id'],title=item['title'],url=item['url'],author=item.get('by',''),time='',time_ago='today',score=item['score'],text='',links=[],comments=item['descendants'])
work=Path(tempfile.mkdtemp(prefix=f'hn-{provider}-live-'))
(work/'data/hn').mkdir(parents=True)
(work/'data/hn/saved.json').write_text(json.dumps([story]))
app=TUI(work/'data',160,50,output=EVIDENCE)
try:
    app.wait(lambda t:'points' in t,timeout=30);app.key('4');app.wait(lambda t:item['title'] in t)
    started=time.monotonic();app.key('c')
    app.wait(lambda t:'Summary ready' in t or 'Summary incomplete' in t or 'key rejected' in t,timeout=120)
    app.capture('fast-ready');assert 'Summary ready' in app.text(),app.text()
    assert 'ARTICLE · Fast' in app.text() and 'DISCUSSION · Fast' in app.text()
    print(f'PASS live Fast article + discussion in {time.monotonic()-started:.1f}s',flush=True)
    app.key('jjj')
    before=re.split(r'^╰[^\n]*\n',app.text(),maxsplit=1,flags=re.M)[1]
    app.key('S');assert 'S View' in app.text();app.key('S')
    after=re.split(r'^╰[^\n]*\n',app.text(),maxsplit=1,flags=re.M)[1]
    assert after==before, 'Summary toggle moved the visible comments'
    print('PASS summary hide/show preserves visible comments',flush=True)
    app.key('\t');app.key('\r');assert 'CITED COMMENT' in app.text();app.capture('captured-source');app.key('\x1b')
    assert 'Summary ready' in app.text();app.capture('fast-source-back')
    app.key('M');assert key not in app.text();app.key('\t');assert 'Haiku 5.5' in app.text();app.capture('settings-fast');app.key(' ');assert 'Sonnet 5.5' in app.text();app.capture('settings-thorough');started=time.monotonic();app.key('\r')
    assert 'ARTICLE · Fast' in app.text(), 'Upgrade removed current article'
    app.capture('thorough-upgrading')
    app.wait(lambda t:('Summary ready' in t and 'ARTICLE · Thorough' in t and 'DISCUSSION · Thorough' in t) or 'Summary incomplete' in t,timeout=120)
    app.capture('thorough-ready');assert 'Summary ready' in app.text(),app.text()
    print(f'PASS live Thorough upgrade in {time.monotonic()-started:.1f}s; old result remained readable',flush=True)
    # A revisit must reuse the completed snapshot, with no loading state.
    app.key('\x1b');app.key('c');assert 'Summary ready' in app.text();assert 'Updating' not in app.text()
    app.key('M');app.key(' ');app.key('\r');assert 'Auto off' in app.text()
    config=json.loads((work/'data/config/hn/summary.json').read_text());assert not config['automatic'];assert config['profile']=='thorough';assert not config['api_key'];assert not config['anthropic_api_key'];assert config['provider']=={'openrouter':'open_router','anthropic':'anthropic'}[provider]
    print('PASS source/back, retained result on revisit, Auto off, environment key not persisted',flush=True)
    app.key('M');app.key('\t\t');started=time.monotonic();app.key('\r')
    app.wait(lambda t:'Summary ready' in t or 'Summary incomplete' in t,timeout=120)
    app.capture('refreshed');assert 'Summary ready' in app.text(),app.text()
    print(f'PASS explicit refresh with Auto off in {time.monotonic()-started:.1f}s',flush=True)
    # Exercise the live comment tree with the summary hidden.
    app.key('S');app.key('g')
    def count():
        m=re.search(r'DISCUSSION · (\d+) visible',app.text())
        return int(m.group(1)) if m else 0
    initial=count()
    for _ in range(12):
        app.key('\r');app.pump(1);expanded=count()
        if expanded>initial: break
        app.key('j')
    else: raise AssertionError('No loaded thread with replies found')
    app.key('j');app.key(']');app.key('u');app.key('c');app.key('g')
    assert count()==initial
    print(f'PASS reply navigation {initial} -> {expanded} -> {initial}',flush=True)
    app.key('\x1b');app.quit();print('PASS clean terminal exit',flush=True)
except BaseException:
    app.capture('failure');app.abort();raise
finally:
    os.environ.pop(key_name,None)
records=[json.loads(p.read_text()) for p in (work/'data/state/hn/openrouter').glob('*.json')]
assert len(records)==6, f"Expected exactly six requests, got {len(records)}"
assert all(r['provider'].lower()==provider and r['http_status']==200 for r in records)
assert all(key not in json.dumps(r) for r in records)
print(f'PASS six successful {provider} requests, no extra call on revisit, no key in logs',flush=True)
print('Evidence:',EVIDENCE,flush=True)
print('Isolated settings and diagnostic logs:',work,flush=True)
