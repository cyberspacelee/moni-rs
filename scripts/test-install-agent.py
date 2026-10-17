#!/usr/bin/env python3
"""Exercise the installer with local releases and mocked system administration."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


with tempfile.TemporaryDirectory(prefix="moni-installer-test-") as directory:
    root = Path(directory)
    mock_bin = root / "mock-bin"
    mock_bin.mkdir()
    downloads = root / "downloads"
    downloads.mkdir()
    (root / "etc/systemd/system").mkdir(parents=True)
    source = Path(__file__).with_name("install-agent.sh").read_text()
    for path in ("/usr/local/bin", "/etc/moni", "/etc/systemd/system"):
        source = source.replace(path, str(root) + path)
    installer = root / "install.sh"
    installer.write_text(source)
    mock = mock_bin / "mock"
    mock.write_text(r'''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
root = pathlib.Path(os.environ['TEST_ROOT'])
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
with (root / 'calls').open('a') as log:
    log.write(json.dumps([name, *args]) + '\n')
if name == 'id':
    if args == ['-u']: print(0)
    elif not (root / 'user').exists(): sys.exit(1)
    elif args == ['-u', 'moni']: print(123)
elif name == 'getent':
    sys.exit(0 if (root / 'group').exists() else 1)
elif name in ('groupadd', 'useradd'):
    (root / ('group' if name == 'groupadd' else 'user')).touch()
elif name == 'uname':
    print(os.environ.get('TEST_ARCH', 'x86_64'))
elif name == 'curl':
    url = next(arg for arg in args if arg.startswith('https://'))
    shutil.copyfile(root / 'downloads' / url.rsplit('/', 1)[1], args[args.index('-o') + 1])
elif name == 'install':
    mode, directory, paths = 0o755, False, []
    while args:
        arg = args.pop(0)
        if arg == '-m': mode = int(args.pop(0), 8)
        elif arg in ('-o', '-g'): args.pop(0)
        elif arg == '-d': directory = True
        else: paths.append(arg)
    if directory:
        for path in paths:
            pathlib.Path(path).mkdir(parents=True, exist_ok=True)
            os.chmod(path, mode)
    else:
        shutil.copyfile(*paths)
        os.chmod(paths[-1], mode)
''')
    mock.chmod(0o755)
    for name in ("curl", "id", "getent", "groupadd", "useradd", "uname", "install", "chown", "systemctl"):
        (mock_bin / name).symlink_to(mock)
    env = {**os.environ, "PATH": f"{mock_bin}:{os.environ['PATH']}",
           "TEST_ROOT": str(root), "TMPDIR": str(root)}
    env.pop("MONI_REPO", None)
    args = ["--server", "wss://example.com/ws/agent", "--token", "mn_test_secret",
            "--repo", "example/moni-rs"]

    def release(content):
        lines = []
        for arch in ("amd64", "arm64"):
            asset = f"moni-agent-linux-{arch}"
            (downloads / asset).write_bytes(content)
            lines.append(f"{hashlib.sha256(content).hexdigest()}  {asset}\n")
        (downloads / "SHA256SUMS").write_text("".join(lines))

    def run(arguments, success=True):
        result = subprocess.run(["sh", str(installer), *arguments], env=env,
                                text=True, capture_output=True, check=False)
        assert (result.returncode == 0) == success, result.stdout + result.stderr
        assert "mn_test_secret" not in result.stdout + result.stderr
        assert not list(root.glob("moni-install.*")), "download directory leaked"
        assert not list(root.glob("etc/moni/agent.env.*")), "environment temporary file leaked"
        assert not list(root.glob("usr/local/bin/moni-agent.*")), "binary temporary file leaked"
        return result

    def calls():
        return [json.loads(line) for line in (root / "calls").read_text().splitlines()]

    binary = root / "usr/local/bin/moni-agent"
    config = root / "etc/moni/agent.env"
    unit = root / "etc/systemd/system/moni-agent.service"
    for invalid in (["--server"], args + ["--version"], args + ["--interval", "0"],
                    args + ["--interval", "4294967296"], args + ["--repo", "../bad"],
                    args + ["--repo", "example/moni-rs\n../bad"],
                    args + ["--server", 'wss://example/\nMONI_TOKEN=oops'],
                    args + ["--token", 'mn_test_secret\nINJECT=yes'],
                    args + ["--version", "../../bad"], args + ["--version", "v1.2.3\n../bad"],
                    args + ["--repo", "example/moni-rs", "--uninstall"]):
        run(invalid, success=False)
    assert not binary.exists()

    release(b"first release")
    run(args + ["--interval", "10"])
    assert binary.read_bytes() == b"first release"
    assert binary.stat().st_mode & 0o777 == 0o755
    assert config.stat().st_mode & 0o777 == 0o640
    assert 'MONI_TOKEN="mn_test_secret"\n' in config.read_text()
    assert "MONI_INTERVAL=10\n" in config.read_text()
    assert "User=moni\nGroup=moni\n" in unit.read_text()
    assert "NoNewPrivileges=true" in unit.read_text()
    assert any(c[:2] == ["chown", "root:moni"] for c in calls())
    assert sum(c[0] == "useradd" for c in calls()) == 1

    release(b"upgraded release")
    env["TEST_ARCH"] = "aarch64"
    run(args + ["--version", "v1.2.3"])
    assert binary.read_bytes() == b"upgraded release"
    assert sum(c == ["systemctl", "restart", "moni-agent.service"] for c in calls()) == 2
    assert sum(c[0] == "useradd" for c in calls()) == 1
    assert any("https://github.com/example/moni-rs/releases/download/v1.2.3/moni-agent-linux-arm64" in c for c in calls())

    before = (binary.read_bytes(), config.read_bytes(), unit.read_bytes())
    previous_calls = len(calls())
    (downloads / "moni-agent-linux-arm64").write_bytes(b"corrupted download")
    run(args, success=False)
    assert (binary.read_bytes(), config.read_bytes(), unit.read_bytes()) == before
    assert not any(c[0] == "systemctl" for c in calls()[previous_calls:])
    (downloads / "SHA256SUMS").write_text("0" * 64 + "  ../../unexpected\n")
    run(args, success=False)
    release(b"upgraded release")
    manifest = downloads / "SHA256SUMS"
    manifest.write_text(manifest.read_text() * 2)
    run(args, success=False)
    run(["--uninstall"])
    run(["--uninstall"])
    assert not binary.exists() and not config.exists() and not unit.exists()
    assert (root / "user").exists()
    print("Installer passed: validation, amd64/arm64, SHA256 rejection, permissions, upgrade/restart and idempotent uninstall")
