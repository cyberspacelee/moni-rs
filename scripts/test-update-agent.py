#!/usr/bin/env python3
"""Run the root updater in an isolated filesystem with real temporary RSA signatures."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


with tempfile.TemporaryDirectory(prefix="moni-updater-test-") as directory:
    root = Path(directory)
    mock_bin = root / "mock-bin"
    mock_bin.mkdir()
    for path in ("etc/moni", "usr/local/bin", "var/lib/moni", "var/lib/moni-update", "downloads"):
        (root / path).mkdir(parents=True)
        (root / path).chmod(0o755)
    (root / "var/lib").chmod(0o755)
    script = Path(__file__).with_name("update-agent.sh").read_text()
    paths = ("/usr/local/bin", "/etc/moni", "/var/lib/moni-update", "/var/lib/moni")
    script = re.sub("|".join(re.escape(path) for path in paths),
                    lambda match: str(root) + match[0], script)
    script = re.sub(r'^export PATH=.*$', 'export PATH="$TEST_MOCK_BIN:$PATH"', script, flags=re.M)
    updater = root / "update.sh"
    updater.write_text(script)
    private_key = root / "signing.pem"
    public_key = root / "etc/moni/release.pub"
    subprocess.run(["openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:3072",
                    "-out", str(private_key)], check=True, capture_output=True)
    subprocess.run(["openssl", "pkey", "-in", str(private_key), "-pubout", "-out", str(public_key)],
                   check=True, capture_output=True)
    public_key.chmod(0o644)
    config = root / "etc/moni/update.conf"
    config.write_text("REPO=trusted/moni-rs\n")
    config.chmod(0o600)
    binary = root / "usr/local/bin/moni-agent"
    request = root / "var/lib/moni/update-request"
    downloads = root / "downloads"
    mock = mock_bin / "mock"
    mock.write_text(r'''#!/usr/bin/env python3
import json, os, pathlib, re, shutil, stat, sys, time
root = pathlib.Path(os.environ['TEST_ROOT'])
name, args = pathlib.Path(sys.argv[0]).name, sys.argv[1:]
(root / 'calls').mkdir(exist_ok=True)
(root / 'calls' / (str(time.time_ns()) + '-' + str(os.getpid()))).write_text(json.dumps([name, *args]))
if name == 'id': print(0)
elif name == 'uname': print(os.environ.get('TEST_ARCH', 'x86_64'))
elif name == 'stat':
    file = pathlib.Path(args[-1]); info = file.lstat(); mode = format(stat.S_IMODE(info.st_mode), 'o')
    code = args[args.index('-c') + 1]
    print({'%u:%a': '0:' + mode, '%a': mode, '%s': info.st_size, '%d': info.st_dev}[code])
elif name == 'curl':
    url = next(arg for arg in args if arg.startswith('https://'))
    if url.endswith('/releases/latest'):
        print(os.environ.get('TEST_LATEST_URL', url.removesuffix('/latest') + '/tag/' + os.environ['TEST_VERSION']), end='')
    else:
        target = args[args.index('-o') + 1]
        shutil.copyfile(root / 'downloads' / url.rsplit('/', 1)[1], target)
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
            pathlib.Path(path).mkdir(parents=True, exist_ok=True); os.chmod(path, mode)
    else:
        shutil.copyfile(*paths); os.chmod(paths[-1], mode)
elif name == 'systemctl':
    if args[0] == 'restart' and os.environ.get('TEST_RESTART_FAIL') and not (root / 'restart-failed').exists():
        (root / 'restart-failed').touch(); sys.exit(1)
    if args[0] == 'restart':
        pid = root / 'main-pid'
        pid.write_text(str(int(pid.read_text()) + 1 if pid.exists() else 4242))
        if not os.environ.get('TEST_NO_HEALTH'):
            version = re.search(r'moni-agent ([0-9.]+)', (root / 'usr/local/bin/moni-agent').read_text())[1]
            health = root / 'var/lib/moni/health'
            health.unlink(missing_ok=True)
            kind = os.environ.get('TEST_HEALTH_KIND')
            if kind == 'symlink': health.symlink_to(root / 'etc/moni/update.conf')
            elif kind == 'wrong-pid': health.write_text(version + '\n999999\n')
            elif kind == 'oversize': health.write_text('x' * 1024)
            elif kind == 'nul': health.write_bytes((version + '\x00\n' + pid.read_text() + '\n').encode())
            else: health.write_text(version + '\n' + pid.read_text() + '\n')
    if args[0] == 'show':
        counter = root / 'show-checks'; count = int(counter.read_text()) + 1 if counter.exists() else 1
        counter.write_text(str(count))
        pid = (root / 'main-pid').read_text()
        if os.environ.get('TEST_HEALTH_KIND') and count == 2:
            version = re.search(r'moni-agent ([0-9.]+)', (root / 'usr/local/bin/moni-agent').read_text())[1]
            health = root / 'var/lib/moni/health'; health.unlink(missing_ok=True)
            health.write_text(version + '\n' + pid + '\n')
        print(int(pid) + 1 if os.environ.get('TEST_PID_CHANGE') and count == 2 else pid)
    if args[0] == 'is-active':
        counter = root / 'health-checks'; count = int(counter.read_text()) + 1 if counter.exists() else 1
        counter.write_text(str(count))
        if count == int(os.environ.get('TEST_HEALTH_FAIL_AT', '0')): sys.exit(1)
''')
    mock.chmod(0o755)
    for name in ("id", "uname", "stat", "curl", "install", "systemctl", "sleep", "sync"):
        (mock_bin / name).symlink_to(mock)
    env = {**os.environ, "TEST_ROOT": str(root), "TEST_MOCK_BIN": str(mock_bin), "TEST_VERSION": "v0.1.1"}

    def executable(version):
        return f'#!/bin/sh\n[ "$1" = --version ] || exit 1\nprintf "moni-agent {version[1:]}\\n"\n'.encode()

    def installed(version="v0.1.0"):
        binary.write_bytes(executable(version))
        binary.chmod(0o755)
        for path in (root / "health-checks", root / "show-checks", root / "restart-failed"):
            path.unlink(missing_ok=True)

    def sign():
        subprocess.run(["openssl", "dgst", "-sha256", "-sign", str(private_key), "-out",
                        str(downloads / "SHA256SUMS.sig"), str(downloads / "SHA256SUMS")],
                       check=True, capture_output=True)

    def release(version="v0.1.1"):
        env["TEST_VERSION"] = version
        manifest = ""
        for arch in ("amd64", "arm64"):
            name = f"moni-agent-linux-{arch}"
            content = executable(version)
            (downloads / name).write_bytes(content)
            manifest += f"{hashlib.sha256(content).hexdigest()}  {name}\n"
        (downloads / "SHA256SUMS").write_text(manifest)
        sign()

    def ask(version="v0.1.1"):
        request.write_text(version + "\n")

    def run(mode="--requested", success=True):
        result = subprocess.run(["sh", str(updater), mode], env=env, text=True,
                                capture_output=True, timeout=15, check=False)
        assert (result.returncode == 0) == success, result.stdout + result.stderr
        assert not list((root / "var/lib/moni-update").glob("work.*")), "root download state leaked"
        assert not list(binary.parent.glob("moni-agent.*")), "binary temporary file leaked"
        return result

    def calls():
        return [json.loads(path.read_text()) for path in sorted((root / "calls").iterdir())]

    installed()
    release()
    ask()
    run()
    assert binary.read_bytes() == executable("v0.1.1")
    assert not request.exists()
    assert (root / "health-checks").read_text() == "2"
    assert (root / "var/lib/moni-update/previous").read_bytes() == executable("v0.1.0")
    assert binary.stat().st_mode & 0o777 == 0o755
    before_calls = len(calls())
    ask("v0.1.0")
    run()
    ask("v0.1.1")
    run()
    assert not any(call[0] == "curl" for call in calls()[before_calls:]), "same version/downgrade fetched a release"
    for bad in ("v1.2.3; touch /tmp/moni-updater-injection", "../x", "v01.2.3", "v1.2.3-rc1",
                "v1.2.3\nREPO=attacker/repo", "v1.2.3\x00", "v1.2.3\n", "x" * 80):
        ask(bad)
        result = run(success=False)
        assert bad not in result.stdout + result.stderr
        assert not request.exists(), "invalid request blocks future requests"
    request.symlink_to(config)
    run(success=False)
    assert not request.is_symlink() and config.read_text() == "REPO=trusted/moni-rs\n"
    request.mkdir()
    run(success=False)
    assert not request.exists()
    os.mkfifo(request)
    run(success=False)
    assert not request.exists()
    print("PASS request validation, symlink/directory rejection, consumption, stable versions, downgrade guard")

    installed()
    release()
    original = binary.read_bytes()
    manifest = downloads / "SHA256SUMS"
    manifest.write_text(manifest.read_text() + "# changed after signature\n")
    ask()
    run(success=False)
    assert binary.read_bytes() == original and not request.exists()
    release()
    (downloads / "moni-agent-linux-amd64").write_bytes(b"corrupted")
    ask()
    run(success=False)
    assert binary.read_bytes() == original
    for contents in ("0" * 64 + "  ../../etc/passwd\n", "z" * 64 + "  moni-agent-linux-amd64\n"):
        manifest.write_text(contents)
        sign()
        ask()
        run(success=False)
        assert binary.read_bytes() == original
    release()
    manifest.write_text(manifest.read_text() * 2)
    sign()
    ask()
    run(success=False)
    assert binary.read_bytes() == original
    release("v0.1.2")
    ask("v0.1.1")
    run(success=False)
    assert binary.read_bytes() == original, "binary version mismatch was installed"
    print("PASS real RSA signature, altered manifest/binary, malicious paths, duplicate checksum, binary version")

    for kind in ("symlink", "wrong-pid", "oversize", "nul"):
        installed()
        release()
        env["TEST_HEALTH_KIND"] = kind
        ask()
        run()
        assert (root / "health-checks").read_text() == "3", f"invalid {kind} health marker was accepted"
        assert config.read_text() == "REPO=trusted/moni-rs\n"
        env.pop("TEST_HEALTH_KIND")
    print("PASS health marker symlink, wrong PID, oversize and NUL rejection before valid Welcome")

    release()
    env["TEST_HEALTH_FAIL_AT"] = "2"
    installed()
    ask()
    run(success=False)
    assert binary.read_bytes() == executable("v0.1.0"), "unhealthy binary was not rolled back"
    assert sum(call[:2] == ["systemctl", "restart"] for call in calls()) >= 3
    env.pop("TEST_HEALTH_FAIL_AT")
    installed()
    env["TEST_RESTART_FAIL"] = "1"
    ask()
    run(success=False)
    assert binary.read_bytes() == executable("v0.1.0")
    env.pop("TEST_RESTART_FAIL")
    installed()
    env["TEST_PID_CHANGE"] = "1"
    ask()
    run(success=False)
    assert binary.read_bytes() == executable("v0.1.0")
    env.pop("TEST_PID_CHANGE")
    installed()
    env["TEST_NO_HEALTH"] = "1"
    ask()
    run(success=False)
    assert binary.read_bytes() == executable("v0.1.0"), "a running but disconnected agent was accepted"
    env.pop("TEST_NO_HEALTH")
    print("PASS authenticated-connection health, second check and restart failure restore the previous binary")

    installed("v0.9.0")
    release("v0.10.0")
    env["TEST_ARCH"] = "aarch64"
    run("--auto")
    assert binary.read_bytes() == executable("v0.10.0")
    assert any("releases/download/v0.10.0/moni-agent-linux-arm64" in " ".join(call) for call in calls())
    env["TEST_LATEST_URL"] = "https://github.com/attacker/repo/releases/tag/v9.0.0"
    run("--auto", success=False)
    env.pop("TEST_LATEST_URL")
    config.chmod(0o666)
    run("--auto", success=False)
    config.chmod(0o600)
    binary.parent.chmod(0o777)
    run("--auto", success=False)
    binary.parent.chmod(0o755)
    interrupted = root / "var/lib/moni-update/pending"
    (root / "var/lib/moni-update/previous").write_bytes(executable("v0.10.0"))
    (root / "var/lib/moni-update/previous").chmod(0o755)
    interrupted.write_text("v0.11.0\n")
    interrupted.chmod(0o600)
    installed("v0.11.0")
    run()
    assert binary.read_bytes() == executable("v0.10.0") and not interrupted.exists()
    assert all('--proto' in call and '--proto-redir' in call and '--max-time' in call and '--max-filesize' in call
               for call in calls() if call[0] == "curl")
    assert all('github.com/trusted/moni-rs/' in " ".join(call) for call in calls() if call[0] == "curl")
    print("PASS auto latest resolution, pinned repository, arm64, numeric versions, unsafe paths and interrupted recovery")
    print("Signed updater tests passed.")
