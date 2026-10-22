"""Local HTTP boundary for the recorded reader scenario; never forwards requests."""
import html
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

FIXTURE = Path(__file__).resolve().parents[1] / "tests/reader/nordwand.json"

class ReaderFixture:
    def __init__(self):
        self.data = json.loads(FIXTURE.read_text())
        self.calls = []
        self.gets = []
        self.get_gates = {}
        self.fail_item = None
        self.errors = []
        self.gates = {("fast", "discussion"): threading.Event(),
                      ("thorough", "article"): threading.Event(),
                      ("thorough", "discussion"): threading.Event()}
        self.fail_discussion = True
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                self.respond(*fixture.get(self.path))

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                self.respond(*fixture.post(self.path, body))

            def respond(self, status, value):
                payload = json.dumps(value).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.base = f"http://127.0.0.1:{self.server.server_port}/"

    def get(self, path):
        self.gets.append(path)
        gate = self.get_gates.get(path)
        if gate is not None and not gate.wait(30):
            self.errors.append(f"Unreleased GET gate {path}")
            return 500, {"error": "Fixture gate timed out"}
        if path == "/v0/topstories.json":
            return 200, list(range(90010001, 90010061))
        if path == "/v0/newstories.json":
            return 503, {"error": "Synthetic delayed feed failure"}
        if path.startswith("/v0/") and path.endswith("stories.json"):
            return 200, [self.data["provenance"]["story_id"]]
        if path.startswith("/v0/item/") and path.endswith(".json"):
            ident = int(path.split("/")[-1][:-5])
            if ident == self.fail_item:
                return 503, {"error": "Synthetic child fetch failure"}
            if 90010001 <= ident <= 90010060:
                return 200, {"id": ident, "type": "story", "title": f"Synthetic feed control {ident - 90010000:02}",
                             "by": "replay_control", "time": 1792497600, "score": 123,
                             "url": "https://subdomain.example.com/control", "descendants": 0}
            comments = self.data["input"]["comments"]
            kids = lambda parent: [c["id"] for c in comments if c["parent"] == parent]
            if ident == self.data["provenance"]["story_id"]:
                return 200, {"id": ident, "type": "story", "title": self.data["input"]["title"],
                             "by": "replay", "time": 1792497600, "score": 123, "descendants": len(comments),
                             "text": "Replay article control: a typeface intended for programming.",
                             "kids": kids(None)}
            for comment in comments:
                if comment["id"] == ident:
                    return 200, {"id": ident, "type": "comment", "by": comment["author"],
                                 "time": 1792497600, "text": html.escape(comment["text"]).replace("\n", "<br>"),
                                 "kids": kids(ident)}
        self.errors.append(f"Unexpected GET {path}")
        return 500, {"error": "Unexpected fixture request"}

    def post(self, path, body):
        if path != "/chat/completions":
            self.errors.append(f"Unexpected POST {path}")
            return 500, {"error": "Unexpected fixture request"}
        content = json.loads(body["messages"][1]["content"])
        section = "article" if "article" in content else "discussion"
        profile = next((name for name, model in self.data["provenance"]["models"].items() if model == body["model"]), None)
        if profile is None:
            self.errors.append("Unexpected fixture model")
            return 500, {"error": "Unexpected fixture model"}
        call = {"profile": profile, "section": section, "status": "waiting"}
        self.calls.append(call)
        gate = self.gates.get((profile, section))
        if gate is not None and not gate.wait(30):
            self.errors.append(f"Unreleased {profile} {section} gate")
            call["status"] = 500
            return 500, {"error": "Fixture gate timed out"}
        if profile == "thorough" and section == "discussion" and self.fail_discussion:
            self.fail_discussion = False
            call["status"] = 503
            return 503, {"error": "Controlled replay failure"}
        result = self.data[profile] if section == "discussion" else {"summary": "Replay article control: a typeface intended for programming."}
        call["status"] = 200
        return 200, {"choices": [{"finish_reason": "stop", "message": {"content": json.dumps(result)}}]}

    def close(self):
        for gate in [*self.gates.values(), *self.get_gates.values()]:
            gate.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
