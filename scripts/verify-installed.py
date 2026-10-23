"""Check installed hn settings through the real UI, outside the checkout, without live requests."""
import argparse
import json
from pathlib import Path
import shutil
import tempfile

from terminal_capture import TUI


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=shutil.which("hn"), help="installed hn (default: PATH)")
    args = parser.parse_args()
    if not args.binary:
        parser.error("hn is not on PATH; install it or pass --binary /path/to/hn")
    binary = str(Path(args.binary).resolve(strict=True))
    output = Path(tempfile.mkdtemp(prefix="hn-installed-captures-"))
    with tempfile.TemporaryDirectory(prefix="hn-installed-check-") as temporary:
        home = Path(temporary)
        data = home / "data"
        saved = data / "hn/saved.json"
        saved.parent.mkdir(parents=True)
        saved.write_text(json.dumps([dict(id=1, title="Installed reader check", url="",
                                         author="fixture", time="", time_ago="today", score=1,
                                         text="Local setup check.", links=[], comments=0)]))
        config = data / "config/hn/summary.json"
        config.parent.mkdir(parents=True)
        key = "local-fixture-not-a-real-key"
        config.write_text(json.dumps(dict(provider="anthropic", anthropic_api_key=key,
                                         automatic=False, profile="thorough")))
        config.chmod(0o600)
        env = {"HOME": str(home), "NO_PROXY": "", "no_proxy": ""}
        env.update({name: "http://127.0.0.1:9" for name in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY")})
        for run, profile, automatic in [(1, "Thorough", "Off"), (2, "Fast", "On")]:
            app = TUI(data, command=[binary], env=env, cwd=home, output=output)
            try:
                app.wait(lambda t: "HN:" in t)
                app.key("4")
                app.wait(lambda t: "Installed reader check" in t)
                app.key("cM")
                screen = app.wait(lambda t: "Summary settings" in t)
                assert "Provider: Anthropic" in screen
                assert f"Model preference: {profile}" in screen
                assert f"Automatic summaries: {automatic}" in screen
                assert "Anthropic key: •" in screen and key not in screen
                app.capture(f"settings-{run}")
                if run == 1:
                    # Start on Automatic, enable it, choose Fast, then apply from Automatic.
                    app.key(" \t \x1b[Z\r")
                    persisted = json.loads(config.read_text())
                    assert persisted["provider"] == "anthropic"
                    assert persisted["profile"] == "fast" and persisted["automatic"]
                    assert persisted["anthropic_api_key"] == key
                    assert config.stat().st_mode & 0o777 == 0o600
                else:
                    app.key("\x1b")
                app.quit()
            except BaseException:
                app.capture("failure")
                raise
            finally:
                app.abort()
    print(f"PASS {binary}: settings saved and restored across two launches outside the checkout")
    print(f"Captures: {output}")


if __name__ == "__main__":
    main()
