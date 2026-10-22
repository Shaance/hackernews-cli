"""PTY interaction and styled captures shared by recorded and live reader checks."""
import codecs
import fcntl
import html
import os
import pty
import select
import signal
import struct
import tempfile
import termios
import time
from pathlib import Path
import pyte

ROOT = Path(__file__).resolve().parents[1]
COLORS = dict(black="#252932", red="#ee6b73", green="#9bc779", brown="#e6bd78",
              blue="#70a7e8", magenta="#c792ea", cyan="#7fcbd1", white="#dddfe4",
              brightblack="#7d8490", brightwhite="#ffffff")

class TUI:
    def __init__(self, data, width=240, height=80, command=None, env=None, output=None):
        self.output = output or Path(tempfile.mkdtemp(prefix="hn-terminal-evidence-"))
        self.output.mkdir(parents=True, exist_ok=True)
        self.screen = pyte.Screen(width, height)
        self.stream = pyte.Stream(self.screen)
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.raw = b""
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
            os.environ.update({"XDG_CONFIG_HOME": str(data / "config"), "XDG_DATA_HOME": str(data),
                               "XDG_STATE_HOME": str(data / "state"), "TERM": "xterm-256color",
                               "TERM_PROGRAM": "hn-pty-test", "COLORTERM": "truecolor"})
            os.environ.pop("NO_COLOR", None)
            if env is not None:
                for name in ("ANTHROPIC_API_KEY", "OPENROUTER_API_KEY", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY",
                             "http_proxy", "https_proxy", "all_proxy"):
                    os.environ.pop(name, None)
                os.environ.update(env)
            os.chdir(ROOT)
            argv = command or [str(ROOT / "target/debug/hn")]
            os.execv(argv[0], argv)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))

    def text(self):
        return "\n".join(self.screen.display)

    def pump(self, duration=0.1):
        end = time.monotonic() + duration
        while time.monotonic() < end:
            if select.select([self.fd], [], [], max(0, end - time.monotonic()))[0]:
                try:
                    data = os.read(self.fd, 65536)
                except OSError:
                    break
                if not data:
                    break
                self.raw += data
                self.stream.feed(self.decoder.decode(data))
        return self.text()

    def wait(self, predicate, timeout=20):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            text = self.pump()
            if predicate(text) and self.pump() == text:
                return text
        raise AssertionError("Timed out:\n" + self.text())

    def key(self, key):
        os.write(self.fd, key.encode())
        return self.pump(0.3)

    def capture(self, name):
        if self.fd is not None:
            self.wait(lambda _: True)
        (self.output / (name + ".txt")).write_text(self.text() + "\n")
        rows = []
        for y in range(self.screen.lines):
            runs = []
            last = None
            for x in range(self.screen.columns):
                cell = self.screen.buffer[y][x]
                style = (cell.fg, cell.bg, cell.bold, cell.reverse, cell.italics, cell.underscore)
                if style != last:
                    runs.append([style, ""])
                    last = style
                runs[-1][1] += cell.data
            row = ""
            for (fg, bg, bold, reverse, italic, underline), text in runs:
                def color(value, default):
                    return default if value == "default" else COLORS.get(value, "#" + value)
                fg, bg = color(fg, "#dddfe4"), color(bg, "#252932")
                if reverse:
                    fg, bg = bg, fg
                css = f"color:{fg};background:{bg};font-weight:{700 if bold else 400};font-style:{'italic' if italic else 'normal'};text-decoration:{'underline' if underline else 'none'}"
                row += f'<span style="{css}">{html.escape(text)}</span>'
            rows.append(row)
        document = '<!doctype html><meta charset="utf-8"><title>' + html.escape(name) + '</title><style>body{background:#252932;color:#dddfe4;margin:20px}pre{font:14px/1.3 ui-monospace,monospace;white-space:pre}</style><pre>' + "\n".join(rows) + '</pre>'
        (self.output / (name + ".html")).write_text(document)

    def quit(self):
        self.key("qq")
        end = time.monotonic() + 5
        while time.monotonic() < end:
            self.pump()
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.pid = None
                os.close(self.fd)
                self.fd = None
                assert os.waitstatus_to_exitcode(status) == 0
                assert b"\x1b[?1049l" in self.raw, "alternate screen was not restored"
                return
        raise AssertionError("Terminal did not quit")

    def abort(self):
        if self.pid is not None:
            try:
                os.kill(self.pid, signal.SIGKILL)
                os.waitpid(self.pid, 0)
            except ProcessLookupError:
                pass
            self.pid = None
            os.close(self.fd)
            self.fd = None
