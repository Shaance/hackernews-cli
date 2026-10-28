"""Run as hnpreview inside the dedicated VM; inspect real host cgroups, not gVisor's virtual /sys."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import uuid

docker = ["docker", "-H", os.environ.get("DOCKER_HOST", f"unix:///run/user/{os.getuid()}/docker.sock")]

def command(*args, check=True, timeout=30):
    return subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          check=check, timeout=timeout)

def fields(path):
    return dict(line.split() for line in path.read_text().splitlines())

name = "hn-gvisor-probe-" + uuid.uuid4().hex[:10]
cid = None
case = sys.argv[1]
assert case in ("pids", "memory")
try:
    cid = command(*docker, "run", "-d", "--name", name, "--runtime=" + os.environ.get("DOCKER_RUNTIME", "runsc-rootless"),
                  "--network=none", "--read-only", "--user=65534:65534", "--cap-drop=ALL",
                  "--security-opt=no-new-privileges", "--memory=256m", "--memory-swap=256m",
                  "--cpus=0.5", "--pids-limit=64", "--tmpfs=/tmp:rw,noexec,nosuid,size=16m",
                  "--entrypoint=/bin/sleep", os.environ.get("PREVIEW_IMAGE", "hn-preview:local"), "600").stdout.strip()
    info = json.loads(command(*docker, "inspect", cid).stdout)[0]
    pid = info["State"]["Pid"]
    cg = Path("/sys/fs/cgroup" + Path(f"/proc/{pid}/cgroup").read_text().strip().split("::", 1)[1])
    assert f"/user.slice/user-{os.getuid()}.slice/user@{os.getuid()}.service/" in str(cg), cg
    assert cg.joinpath("memory.max").read_text().strip() == "268435456"
    assert cg.joinpath("cpu.max").read_text().strip() == "50000 100000"
    assert cg.joinpath("pids.max").read_text().strip() == "64"
    unit = cg.name
    user_unit = command("systemctl", "--user", "show", unit, "-p", "LoadState").stdout
    system_unit = command("systemctl", "show", unit, "-p", "LoadState").stdout
    assert "LoadState=loaded" in user_unit and "LoadState=not-found" in system_unit
    print("PASS user-owned systemd scope and real host limits:", cg, flush=True)
    isolation = command(*docker, "exec", cid, "sh", "-c",
                        "id; uname -r; cat /proc/net/dev; "
                        "cat /proc/self/status | grep -E '^(CapEff|NoNewPrivs):'; "
                        "test ! -S /var/run/docker.sock; "
                        "if touch /etc/hn-probe 2>/dev/null; then exit 90; fi; touch /tmp/hn-probe").stdout
    assert "uid=65534" in isolation and "gvisor" in isolation and "0000000000000000" in isolation
    assert "NoNewPrivs:\t1" in isolation
    net = command(*docker, "exec", cid, "cat", "/proc/net/dev").stdout
    assert [line.split(":")[0].strip() for line in net.splitlines()[2:] if line.strip()] == ["lo"]
    command(*docker, "exec", cid, "sh", "-c",
            'for host in 1.1.1.1 169.254.169.254 172.17.0.1; do '
            'if timeout 2 bash -c "echo > /dev/tcp/$host/80" 2>/dev/null; then exit 91; fi; done; '
            'if timeout 2 getent hosts example.com >/dev/null; then exit 92; fi')
    print("PASS gVisor identity, non-root, readonly root, no socket, blocked network\n" + isolation, flush=True)
    before = fields(cg / "cpu.stat")
    command(*docker, "exec", cid, "timeout", "3", "sh", "-c", "while :; do :; done", check=False)
    after = fields(cg / "cpu.stat")
    assert int(after["nr_throttled"]) > int(before["nr_throttled"])
    print("PASS CPU throttling", before["nr_throttled"], after["nr_throttled"], flush=True)
    event_file, event_key = ("pids.events", "max") if case == "pids" else ("memory.events", "oom_kill")
    pressure_command = ('i=0; while [ "$i" -lt 80 ]; do sleep 2 & i=$((i+1)); done; wait'
                        if case == "pids" else 'head -c 536870912 /dev/zero | sort -S 512M >/dev/null')
    before_event = int(fields(cg / event_file)[event_key])
    observed = before_event
    with tempfile.TemporaryFile(mode="w+") as output:
        pressure = subprocess.Popen(docker + ["exec", cid, "timeout", "15", "bash", "-c", pressure_command],
                                    stdout=output, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 25
        while pressure.poll() is None and time.monotonic() < deadline:
            try:
                observed = max(observed, int(fields(cg / event_file)[event_key]))
            except FileNotFoundError:
                pass  # Runtime cleanup may remove the scope immediately after a limit ends it.
            time.sleep(.001)
        pressure.wait(timeout=2)
        output.seek(0)
        detail = output.read()[-600:]
    state = json.loads(command(*docker, "inspect", cid).stdout)[0]["State"]
    assert observed > before_event or (case == "memory" and state["OOMKilled"]), (observed, state, detail)
    print("PASS", case, "limit enforced", {"events": observed - before_event, "exit": pressure.returncode,
          "container_running": state["Running"], "oom_killed": state["OOMKilled"], "output": detail}, flush=True)

finally:
    if cid:
        command(*docker, "rm", "-f", cid)
        if "cg" in locals():
            assert not cg.exists(), f"Cgroup leaked: {cg}"
        print("PASS probe container and cgroup removed", flush=True)
