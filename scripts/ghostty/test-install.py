"""Exercise the installer with a real v0.13.4 distribution, without opening Ghostty."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

INSTALLER = Path(__file__).with_name("install.sh").resolve()
PATCH = INSTALLER.with_name("terminal-browser-v0.13.4.patch")
ORIGINAL = "71cb9486199e4af98c3192ce8a44496d63832006e314bbd2bee70f4542505ac8"
PATCHED = "de03483f54653266fcd6553c03d16e71f982209c7266e1a631d91229fc8d8393"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args, env, cwd, success=True):
    result = subprocess.run(args, env=env, cwd=cwd, text=True, capture_output=True)
    assert (result.returncode == 0) == success, result.stdout + result.stderr
    return result.stdout + result.stderr


def main(distribution):
    with tempfile.TemporaryDirectory(prefix="hn-install-test-") as temporary:
        root = Path(temporary)
        source = root / "download"
        subprocess.run(["ditto", str(distribution), str(source)], check=True)
        bundle = source / "cli/dist/main.js"
        if digest(bundle) == PATCHED:
            with PATCH.open() as patch:
                subprocess.run(["patch", "-R", "-d", str(source), "-p1"], stdin=patch, check=True)
        assert digest(bundle) == ORIGINAL
        # Exercise shell quoting, not only the common no-spaces home path.
        home = root / "reader's $home"
        launcher = home / ".local/bin/terminal-browser"
        launcher.parent.mkdir(parents=True)
        launcher.symlink_to(source / "bin/terminal-browser")
        config = home / ".config/hn/summary.json"
        config.parent.mkdir(parents=True)
        settings = dict(provider="anthropic", anthropic_api_key="local-fixture", automatic=False, profile="thorough")
        config.write_text(json.dumps(settings))
        config.chmod(0o600)
        saved = config.read_bytes()
        env = {key: value for key, value in os.environ.items() if key not in (
            "XDG_DATA_HOME", "XDG_CONFIG_HOME", "TERMINAL_BROWSER_DIST_ROOT", "NATIVE_SCROLL_HELPER",
            "ANTHROPIC_API_KEY", "OPENROUTER_API_KEY",
        )}
        env.update(HOME=str(home), PATH=str(launcher.parent) + ":/usr/bin:/bin:/usr/sbin:/sbin", DO_NOT_TRACK="1")
        command = ["sh", str(INSTALLER), str(source)]
        print("Installing original distribution into an isolated home…", flush=True)
        run(command, env, root)
        installed = home / ".local/share/hn/terminal-browser-v0.13.4"
        assert digest(installed / "cli/dist/main.js") == PATCHED
        assert digest(bundle) == ORIGINAL, "Installer modified its source"
        assert config.read_bytes() == saved
        assert config.stat().st_mode & 0o777 == 0o600
        marker = installed / "retained-on-rerun"
        marker.touch()
        run(command, env, root)
        assert marker.exists(), "Rerun replaced existing installation"
        source.rename(root / "download-moved")
        assert "v0.13.4" in run(["terminal-browser", "--version"], env, root)
        assert "Usage:" in run(["terminal-browser", "open", "--help"], env, root)
        assert not launcher.is_symlink()
        assert str(source) not in launcher.read_text()
        # A future setup can use the installed, already-patched distribution itself.
        run(["sh", str(INSTALLER), str(installed)], env, root)
        assert config.read_bytes() == saved
        print("PASS independent install, original patch, rerun, source removal, quoting and settings preservation", flush=True)

        # Existing unrelated commands must not be silently replaced.
        launcher.write_text("#!/bin/sh\necho unrelated\n")
        unrelated = launcher.read_bytes()
        assert "already exists" in run(["sh", str(INSTALLER), str(installed)], env, root, success=False)
        assert launcher.read_bytes() == unrelated
        launcher.unlink()
        # Exercise fresh installation from an already patched distribution and XDG data path.
        other_data = root / "separate data"
        env["XDG_DATA_HOME"] = str(other_data)
        run(["sh", str(INSTALLER), str(installed)], env, root)
        assert digest(other_data / "hn/terminal-browser-v0.13.4/cli/dist/main.js") == PATCHED
        assert "v0.13.4" in run(["terminal-browser", "--version"], env, root)
        installed_launcher = launcher.read_bytes()
        damaged = root / "unsupported"
        (damaged / "cli/dist").mkdir(parents=True)
        (damaged / "VERSION").write_text("v0.13.4\n")
        (damaged / "cli/dist/main.js").write_text("unsupported bundle")
        assert "Unrecognized" in run(["sh", str(INSTALLER), str(damaged)], env, root, success=False)
        assert launcher.read_bytes() == installed_launcher
        assert config.read_bytes() == saved
        print("PASS existing command preservation, patched source, XDG data path and unsupported bundle rejection", flush=True)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("Usage: python3 scripts/ghostty/test-install.py /path/to/terminal-browser-v0.13.4")
    main(Path(sys.argv[1]).resolve())
