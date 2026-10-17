#!/usr/bin/env python3
"""Exercise the installer with local releases and mocked system administration."""
import hashlib
import json
import os
from pathlib import Path
import re
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
    paths = ("/usr/local/bin", "/usr/local/libexec", "/etc/moni", "/etc/systemd/system",
             "/var/lib/moni-update", "/var/lib/moni")
    source = re.sub("|".join(re.escape(path) for path in paths),
                    lambda match: str(root) + match[0], source)
    source = re.sub(r'^export PATH=.*$', 'export PATH="$TEST_MOCK_BIN:$PATH"', source, flags=re.M)
    source = source.replace('/tmp/moni-install.', str(root / 'moni-install.'))
    installer = root / "install.sh"
    installer.write_text(source)
    mock = mock_bin / "mock"
    mock.write_text(r'''#!/usr/bin/env python3
import json, os, pathlib, shutil, stat, sys, time
root = pathlib.Path(os.environ['TEST_ROOT'])
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
(root / 'calls').mkdir(exist_ok=True)
(root / 'calls' / (str(time.time_ns()) + '-' + str(os.getpid()))).write_text(json.dumps([name, *args]))
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
elif name == 'stat':
    info = pathlib.Path(args[-1]).lstat(); mode = format(stat.S_IMODE(info.st_mode), 'o')
    code = args[args.index('-c') + 1]
    print({'%u:%a': '0:' + mode, '%a': mode, '%s': info.st_size}[code])
elif name == 'curl':
    url = next(arg for arg in args if arg.startswith('https://'))
    if url.endswith('/releases/latest'):
        print(os.environ.get('TEST_LATEST', url.removesuffix('/latest') + '/tag/v1.2.3'), end='')
    else:
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
    for name in ("curl", "id", "getent", "groupadd", "useradd", "uname", "stat", "install", "chown", "systemctl"):
        (mock_bin / name).symlink_to(mock)
    env = {**os.environ, "PATH": f"{mock_bin}:{os.environ['PATH']}",
           "TEST_ROOT": str(root), "TEST_MOCK_BIN": str(mock_bin), "TMPDIR": str(root)}
    env.pop("MONI_REPO", None)
    args = ["--server", "wss://example.com/ws/agent", "--token", "mn_test_secret",
            "--repo", "example/moni-rs"]
    private_key = root / "signing.pem"
    public_key = root / "release.pub"
    subprocess.run(["openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:3072",
                    "-out", str(private_key)], check=True, capture_output=True)
    subprocess.run(["openssl", "pkey", "-in", str(private_key), "-pubout", "-out", str(public_key)],
                   check=True, capture_output=True)

    def sign():
        subprocess.run(["openssl", "dgst", "-sha256", "-sign", str(private_key), "-out",
                        str(downloads / "SHA256SUMS.sig"), str(downloads / "SHA256SUMS")],
                       check=True, capture_output=True)

    def release(content):
        lines = []
        for arch in ("amd64", "arm64"):
            asset = f"moni-agent-linux-{arch}"
            (downloads / asset).write_bytes(content)
            lines.append(f"{hashlib.sha256(content).hexdigest()}  {asset}\n")
        helper = Path(__file__).with_name("update-agent.sh").read_bytes()
        (downloads / "update-agent.sh").write_bytes(helper)
        lines.append(f"{hashlib.sha256(helper).hexdigest()}  update-agent.sh\n")
        (downloads / "SHA256SUMS").write_text("".join(lines))
        sign()

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
        return [json.loads(path.read_text()) for path in sorted((root / "calls").iterdir())]

    binary = root / "usr/local/bin/moni-agent"
    config = root / "etc/moni/agent.env"
    unit = root / "etc/systemd/system/moni-agent.service"
    for invalid in (["--server"], args, args + ["--version"], args + ["--interval", "0"],
                    args + ["--interval", "4294967296"], args + ["--repo", "../bad"],
                    args + ["--repo", "example/moni-rs\n../bad"],
                    args + ["--server", 'wss://example/\nMONI_TOKEN=oops'],
                    args + ["--token", 'mn_test_secret\nINJECT=yes'],
                    args + ["--version", "../../bad"], args + ["--version", "v1.2.3\n../bad"],
                    args + ["--version", "v01.2.3"], args + ["--version", "v1.2.3-rc1"],
                    args + ["--auto-update"], args + ["--update-public-key", "/missing"],
                    args + ["--repo", "example/moni-rs", "--uninstall"]):
        run(invalid, success=False)
    assert not binary.exists()
    assert not any(c[0] == "curl" for c in calls()), "untrusted first installation attempted a download"

    release(b"first release")
    run(args + ["--interval", "10", "--update-public-key", str(public_key)])
    assert binary.read_bytes() == b"first release"
    assert binary.stat().st_mode & 0o777 == 0o755
    assert config.stat().st_mode & 0o777 == 0o640
    assert 'MONI_TOKEN="mn_test_secret"\n' in config.read_text()
    assert "MONI_INTERVAL=10\n" in config.read_text()
    assert "User=moni\nGroup=moni\n" in unit.read_text()
    assert "NoNewPrivileges=true" in unit.read_text()
    assert any(c[:2] == ["chown", "root:moni"] for c in calls())
    assert sum(c[0] == "useradd" for c in calls()) == 1
    assert "StateDirectory=moni\nStateDirectoryMode=0700" in unit.read_text()
    assert not any(c == ["systemctl", "enable", "--now", "moni-agent-update.timer"] for c in calls())
    config.write_text(config.read_text() + "MONI_IFACES=eth0\nMONI_DISKS=/data\n")
    backup = root / "var/lib/moni-update/previous"
    backup.write_bytes(b"saved previous binary")

    release(b"upgraded release")
    env["TEST_ARCH"] = "aarch64"
    run(args + ["--version", "v1.2.3"])
    assert binary.read_bytes() == b"upgraded release"
    assert backup.read_bytes() == b"saved previous binary"
    assert "MONI_INTERVAL=10\n" in config.read_text(), "reinstall overwrote the existing configuration"
    assert "MONI_IFACES=eth0\nMONI_DISKS=/data\n" in config.read_text()
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
    sign()
    run(args, success=False)
    release(b"upgraded release")
    manifest = downloads / "SHA256SUMS"
    manifest.write_text(manifest.read_text() * 2)
    sign()
    run(args, success=False)
    release(b"upgraded release")
    (downloads / "SHA256SUMS.sig").unlink()
    run(args, success=False)
    assert (binary.read_bytes(), config.read_bytes(), unit.read_bytes()) == before
    release(b"signed release")
    run(args + ["--auto-update"])
    assert 'MONI_UPDATE_REPO="example/moni-rs"' in config.read_text()
    assert "MONI_IFACES=eth0\nMONI_DISKS=/data\n" in config.read_text()
    update_conf = root / "etc/moni/update.conf"
    assert update_conf.read_text() == "REPO=example/moni-rs\n"
    assert update_conf.stat().st_mode & 0o777 == 0o600
    assert (root / "usr/local/libexec/moni-agent-update.sh").exists()
    assert (root / "var/lib/moni").stat().st_mode & 0o777 == 0o700
    assert (root / "var/lib/moni-update").stat().st_mode & 0o777 == 0o700
    assert "OnUnitActiveSec=6h" in (root / "etc/systemd/system/moni-agent-update.timer").read_text()
    assert any(c == ["systemctl", "enable", "--now", "moni-agent-update.path"] for c in calls())
    assert any(c == ["systemctl", "enable", "--now", "moni-agent-update.timer"] for c in calls())
    before = (binary.read_bytes(), config.read_bytes(), unit.read_bytes())
    manifest.write_text(manifest.read_text() + "# tampered after signing\n")
    run(args, success=False)
    assert (binary.read_bytes(), config.read_bytes(), unit.read_bytes()) == before
    run(args + ["--repo", "other/repo"], success=False)
    release(b"reconfigured release")
    run(args + ["--reconfigure", "--interval", "3"])
    assert "MONI_INTERVAL=3\n" in config.read_text()
    assert "MONI_IFACES=eth0\nMONI_DISKS=/data\n" in config.read_text()
    assert config.read_text().count("MONI_UPDATE_REPO=") == 1
    env["TEST_LATEST"] = "https://github.com/other/repo/releases/tag/v1.2.3"
    run(args, success=False)
    env.pop("TEST_LATEST")
    assert all('--proto' in c and '--proto-redir' in c and '--max-time' in c and '--max-filesize' in c
               for c in calls() if c[0] == "curl")
    health = root / "var/lib/moni/health"
    health.write_text("0.1.0\n12345\n")
    run(["--uninstall"])
    run(["--uninstall"])
    assert not binary.exists() and not config.exists() and not unit.exists()
    assert not health.exists() and not health.parent.exists()
    assert (root / "user").exists()
    assert not (root / "etc/moni/release.pub").exists() and not update_conf.exists()
    print("Installer passed: validation, amd64/arm64, configuration preservation, SHA256/RSA rejection, pinned source, root units and uninstall")
