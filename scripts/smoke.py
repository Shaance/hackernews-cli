# Live HN/OpenRouter smoke test; makes paid model requests. Run with uv run --with pyte.
import os, pty, fcntl, termios, struct, select, time, json, tempfile, codecs, re, signal
from pathlib import Path
import pyte
ROOT=Path(__file__).resolve().parents[1]
EVIDENCE=Path(tempfile.mkdtemp(prefix="hn-live-evidence-"))
class TUI:
    def __init__(self, data, width=80, height=24):
        self.screen=pyte.Screen(width,height);self.stream=pyte.Stream(self.screen)
        self.decoder=codecs.getincrementaldecoder("utf-8")("replace");self.raw=b""
        self.pid,self.fd=pty.fork()
        if self.pid==0:
            os.environ.update({"XDG_CONFIG_HOME":str(data/"config"),"XDG_DATA_HOME":str(data),"TERM":"xterm-256color","TERM_PROGRAM":"hn-pty-test"})
            os.chdir(ROOT);os.execv(str(ROOT/'target/debug/hn'),['hn'])
        fcntl.ioctl(self.fd,termios.TIOCSWINSZ,struct.pack("HHHH",height,width,0,0))
    def text(self): return "\n".join(self.screen.display)
    def pump(self,duration=0.3):
        end=time.monotonic()+duration
        while time.monotonic()<end:
            if select.select([self.fd],[],[],min(0.1,max(0,end-time.monotonic())))[0]:
                try: data=os.read(self.fd,65536)
                except OSError: break
                self.raw+=data;self.stream.feed(self.decoder.decode(data))
        return self.text()
    def wait(self,predicate,timeout=20):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            text=self.pump()
            if predicate(text): return text
        raise AssertionError("Timed out:\n"+self.text())
    def key(self,key): os.write(self.fd,key.encode());return self.pump()
    def capture(self,name): (EVIDENCE/(name+".txt")).write_text(self.text()+"\n")
    def quit(self):
        self.key("qq");self.pump()
        pid,status=os.waitpid(self.pid,0)
        assert os.waitstatus_to_exitcode(status)==0
        assert b"\x1b[?1049l" in self.raw,"alternate screen was not restored"
        os.close(self.fd)
    def abort(self):
        try: os.kill(self.pid,15);os.waitpid(self.pid,0)
        except ProcessLookupError: pass
        os.close(self.fd)


import urllib.request
from concurrent.futures import ThreadPoolExecutor
key=os.environ.get("OPENROUTER_API_KEY", "")
keyfile=ROOT/".env.local"
if not key and keyfile.exists():
    for line in keyfile.read_text().splitlines():
        if line.startswith("OPENROUTER_API_KEY="):
            key=line.split("=",1)[1].strip().strip("\"'")
assert key, "Set OPENROUTER_API_KEY or add it to .env.local"
os.environ['OPENROUTER_API_KEY']=key

def fetch(url):
    with urllib.request.urlopen(url,timeout=25) as response:return json.load(response)
item=fetch('https://hacker-news.firebaseio.com/v0/item/50150809.json')
assert item and item.get('url')
print('Live story:',item['id'],item['title'],item['url'],flush=True)
story=dict(id=item['id'],title=item['title'],url=item['url'],author=item.get('by',''),time='',time_ago='today',score=item['score'],text='',links=[],comments=item['descendants'])
work=Path(tempfile.mkdtemp(prefix='hn-openrouter-live-'))
(work/'data/hn').mkdir(parents=True)
(work/'data/hn/saved.json').write_text(json.dumps([story]))
app=TUI(work/'data',160,50)
try:
    app.wait(lambda t:'points' in t,timeout=30);app.key('4');app.wait(lambda t:item['title'] in t)
    started=time.monotonic();app.key('c')
    app.wait(lambda t:'Summary ready' in t or 'Summary incomplete' in t or 'key rejected' in t,timeout=120)
    app.capture('fast-ready');assert 'Summary ready' in app.text(),app.text()
    assert 'ARTICLE · Fast' in app.text() and 'DISCUSSION · Fast' in app.text()
    print(f'PASS live Fast article + discussion in {time.monotonic()-started:.1f}s',flush=True)
    app.key('jjj');before=app.text();app.key('S');assert 'S View' in app.text();app.key('S')
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
    config=json.loads((work/'data/config/hn/summary.json').read_text());assert not config['automatic'];assert config['profile']=='thorough';assert not config['api_key']
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
    os.environ.pop('OPENROUTER_API_KEY',None)
print('Evidence:',EVIDENCE,flush=True)
print('Isolated settings:',work,flush=True)
