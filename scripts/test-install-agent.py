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
        asset = root / 'downloads' / url.rsplit('/', 1)[1]
        if '-o' in args:
            shutil.copyfile(asset, args[args.index('-o') + 1])
        else:
            assert asset.name == 'install-agent.sh'
            sys.stdout.write((root / 'install.sh').read_text())
elif name == 'sudo':
    os.execvp(args[0], args)
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
    for name in ("curl", "sudo", "id", "getent", "groupadd", "useradd", "uname", "stat", "install", "chown", "systemctl"):
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
    embedded_keys = re.findall(r"-----BEGIN PUBLIC KEY-----\n.*?-----END PUBLIC KEY-----\n", source, re.S)
    assert len(embedded_keys) == 1, "installer must embed exactly one release public key"
    committed_key = Path(__file__).with_name("release-public.pem").read_text()
    assert embedded_keys[0] == committed_key, "installer public key differs from the committed release key"
    source = source.replace(embedded_keys[0], public_key.read_text())
    installer.write_text(source)

    settings_source = (Path(__file__).resolve().parents[1] / "web/src/pages/Settings.tsx").read_text()
    command_function = re.search(r"function installCommand\([^\n]+\): string \{\n.*?\n\}", settings_source, re.S)
    assert command_function, "Settings installCommand template was not found"
    command_js = re.sub(r"function installCommand\([^\n]+\): string", "function installCommand(config, token)",
                        command_function[0], count=1)

    def panel_command(token):
        result = subprocess.run(["node", "-e", 'const location = {protocol: "https:", host: "example.com"};\n'
                                 + command_js + '\nprocess.stdout.write(installCommand('
                                 '{agent_release_repo: "example/moni-rs"}, process.env.TEST_INSTALL_TOKEN));'],
                                env={**env, "TEST_INSTALL_TOKEN": token}, text=True, capture_output=True, check=True)
        return result.stdout

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
        command = ["bash", "-o", "pipefail", "-c", arguments] if isinstance(arguments, str) else ["sh", str(installer), *arguments]
        result = subprocess.run(command, env=env,
                                text=True, capture_output=True, check=False)
        assert not re.search(r"mn_[A-Za-z0-9_-]+", result.stdout + result.stderr), "installer logged an agent token"
        assert (result.returncode == 0) == success, result.stdout + result.stderr
        assert not list(root.glob("moni-install.*")), "download directory leaked"
        assert not list(root.glob("etc/moni/agent.env.*")), "environment temporary file leaked"
        assert not list(root.glob("usr/local/bin/moni-agent.*")), "binary temporary file leaked"
        return result

    def calls():
        return [json.loads(path.read_text()) for path in sorted((root / "calls").iterdir())]

    binary = root / "usr/local/bin/moni-agent"
    config = root / "etc/moni/agent.env"
    unit = root / "etc/systemd/system/moni-agent.service"
    pinned_key = root / "etc/moni/release.pub"
    update_conf = root / "etc/moni/update.conf"

    def configuration():
        values = {}
        for line in config.read_text().splitlines():
            if line and not line.startswith('#') and '=' in line:
                name, value = line.split('=', 1)
                assert name not in values, f"duplicate configuration field: {name}"
                values[name] = value
        return values

    def installed():
        return tuple(path.read_bytes() for path in (binary, config, unit, pinned_key, update_conf))

    for invalid in ([], ["--server"], ["--repo", "example/moni-rs"], args + ["--version"], args + ["--interval", "0"],
                    args + ["--interval", "4294967296"], args + ["--repo", "../bad"],
                    args + ["--repo", "example/moni-rs\n../bad"],
                    args + ["--server", 'wss://example/\nMONI_TOKEN=oops'],
                    args + ["--token", 'mn_test_secret\nINJECT=yes'],
                    args + ["--version", "../../bad"], args + ["--version", "v1.2.3\n../bad"],
                    args + ["--version", "v01.2.3"], args + ["--version", "v1.2.3-rc1"],
                    args + ["--update-public-key", "/missing"], args + ["--reconfigure"],
                    args + ["--repo", "example/moni-rs", "--uninstall"]):
        run(invalid, success=False)
    assert not binary.exists()
    assert not any(c[0] == "curl" for c in calls()), "invalid arguments attempted a download"

    release(b"first release")
    run(panel_command("mn_test_secret"))
    assert binary.read_bytes() == b"first release"
    assert binary.stat().st_mode & 0o777 == 0o755
    assert config.stat().st_mode & 0o777 == 0o640
    assert 'MONI_TOKEN="mn_test_secret"\n' in config.read_text()
    assert "MONI_INTERVAL=2\n" in config.read_text()
    assert pinned_key.read_bytes() == public_key.read_bytes()
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
    previous_config = configuration()
    run(panel_command("mn_rotated_secret"))
    assert binary.read_bytes() == b"upgraded release"
    assert backup.read_bytes() == b"saved previous binary"
    assert configuration() == {**previous_config, "MONI_TOKEN": '"mn_rotated_secret"'}, "panel command did not rotate only the token"
    assert "MONI_IFACES=eth0\nMONI_DISKS=/data\n" in config.read_text()
    assert sum(c == ["systemctl", "restart", "moni-agent.service"] for c in calls()) == 2
    assert sum(c[0] == "useradd" for c in calls()) == 1
    assert any("https://github.com/example/moni-rs/releases/download/v1.2.3/moni-agent-linux-arm64" in c for c in calls())

    previous_config = configuration()
    run(["--server", "wss://changed.example/ws/agent"])
    assert configuration() == {**previous_config, "MONI_SERVER": '"wss://changed.example/ws/agent"'}
    previous_config = configuration()
    run(["--interval", "10"])
    assert configuration() == {**previous_config, "MONI_INTERVAL": "10"}

    previous_config = configuration()
    pinned_key.unlink()
    update_conf.unlink()
    run(["--repo", "example/moni-rs"])
    assert configuration() == previous_config, "migration from an existing environment changed its credentials"
    assert pinned_key.read_bytes() == public_key.read_bytes()

    other_private_key = root / "other-signing.pem"
    other_public_key = root / "other-release.pub"
    subprocess.run(["openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:3072",
                    "-out", str(other_private_key)], check=True, capture_output=True)
    subprocess.run(["openssl", "pkey", "-in", str(other_private_key), "-pubout", "-out", str(other_public_key)],
                   check=True, capture_output=True)
    release(b"unattended upgrade")
    previous_config = configuration()
    previous_calls = len(calls())
    # An existing installation must use its pinned key even if the bootstrap key changes.
    installer.write_text(source.replace(public_key.read_text(), other_public_key.read_text()))
    env["MONI_REPO"] = "other/repo"
    run([])
    env.pop("MONI_REPO")
    installer.write_text(source)
    assert binary.read_bytes() == b"unattended upgrade"
    assert configuration() == previous_config, "argument-free upgrade changed existing configuration"
    assert "MONI_IFACES=eth0\nMONI_DISKS=/data\n" in config.read_text()
    assert pinned_key.read_bytes() == public_key.read_bytes()
    assert backup.read_bytes() == b"saved previous binary"
    assert any("https://github.com/example/moni-rs/releases/download/v1.2.3/moni-agent-linux-arm64" in c
               for c in calls()[previous_calls:]), "argument-free upgrade did not reuse the pinned repository"
    assert not any(arg.startswith("https://github.com/other/repo/")
                   for call in calls()[previous_calls:] for arg in call), "inherited repository overrode the local pin"

    before = installed()
    run(["--update-public-key", str(other_public_key)], success=False)
    assert installed() == before, "untrusted replacement key changed the installation"
    for path in (pinned_key, update_conf):
        saved = path.with_suffix('.saved')
        path.rename(saved)
        run(args, success=False)
        assert not path.exists(), "incomplete pin was silently replaced by the bootstrap key"
        saved.rename(path)
        assert installed() == before

    previous_calls = len(calls())
    (downloads / "moni-agent-linux-arm64").write_bytes(b"corrupted download")
    run([], success=False)
    assert installed() == before
    assert not any(c[0] == "systemctl" for c in calls()[previous_calls:])
    (downloads / "SHA256SUMS").write_text("0" * 64 + "  ../../unexpected\n")
    sign()
    run([], success=False)
    release(b"upgraded release")
    manifest = downloads / "SHA256SUMS"
    manifest.write_text(manifest.read_text() * 2)
    sign()
    run([], success=False)
    release(b"upgraded release")
    (downloads / "SHA256SUMS.sig").unlink()
    run([], success=False)
    assert installed() == before
    release(b"signed release")
    run(["--auto-update"])
    assert 'MONI_UPDATE_REPO="example/moni-rs"' in config.read_text()
    assert "MONI_IFACES=eth0\nMONI_DISKS=/data\n" in config.read_text()
    assert update_conf.read_text() == "REPO=example/moni-rs\n"
    assert update_conf.stat().st_mode & 0o777 == 0o600
    assert (root / "usr/local/libexec/moni-agent-update.sh").exists()
    assert (root / "var/lib/moni").stat().st_mode & 0o777 == 0o700
    assert (root / "var/lib/moni-update").stat().st_mode & 0o777 == 0o700
    assert "OnUnitActiveSec=6h" in (root / "etc/systemd/system/moni-agent-update.timer").read_text()
    assert any(c == ["systemctl", "enable", "--now", "moni-agent-update.path"] for c in calls())
    assert any(c == ["systemctl", "enable", "--now", "moni-agent-update.timer"] for c in calls())
    before = installed()
    manifest.write_text(manifest.read_text() + "# tampered after signing\n")
    run([], success=False)
    assert installed() == before
    run(args + ["--repo", "other/repo"], success=False)
    release(b"signed release")
    run(["--interval", "3"])
    assert "MONI_INTERVAL=3\n" in config.read_text()
    assert 'MONI_TOKEN="mn_rotated_secret"\n' in config.read_text()
    assert 'MONI_SERVER="wss://changed.example/ws/agent"\n' in config.read_text()
    assert "MONI_IFACES=eth0\nMONI_DISKS=/data\n" in config.read_text()
    assert config.read_text().count("MONI_UPDATE_REPO=") == 1
    env["TEST_LATEST"] = "https://github.com/other/repo/releases/tag/v1.2.3"
    run([], success=False)
    env.pop("TEST_LATEST")
    assert all('--proto' in c and '--proto-redir' in c and '--max-time' in c and '--max-filesize' in c
               for c in calls() if c[0] == "curl" and not any(arg.endswith('/install-agent.sh') for arg in c))
    health = root / "var/lib/moni/health"
    health.write_text("0.1.0\n12345\n")
    run(["--uninstall"])
    run(["--uninstall"])
    assert not binary.exists() and not config.exists() and not unit.exists()
    assert not health.exists() and not health.parent.exists()
    assert (root / "user").exists()
    assert not (root / "etc/moni/release.pub").exists() and not update_conf.exists()
    print("Installer passed: real panel command, embedded key, token rotation, partial configuration updates, argument-free upgrade, SHA256/RSA rejection, pinned source, root units and uninstall")
