#!/bin/sh
# 用法：sudo sh install-agent.sh --server wss://<host>/ws/agent --token mn_xxx --repo owner/repo
set -eu
export LC_ALL=C

fail() { echo "moni-agent: $*" >&2; exit 1; }
usage() {
  echo 'Usage: install-agent.sh --server URL --token TOKEN --repo OWNER/REPO [--interval SECONDS] [--version vX.Y.Z]'
  echo '       install-agent.sh --uninstall'
}

REPO=${MONI_REPO:-}
SERVER=
TOKEN=
INTERVAL=5
VERSION=
UNINSTALL=false
arg_count=$#
while [ "$#" -gt 0 ]; do
  case "$1" in
    --server|--token|--repo|--interval|--version)
      [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 requires a value"
      case "$2" in --*) fail "$1 requires a value" ;; esac
      case "$1" in
        --server) SERVER=$2 ;;
        --token) TOKEN=$2 ;;
        --repo) REPO=$2 ;;
        --interval) INTERVAL=$2 ;;
        --version) VERSION=$2 ;;
      esac
      shift 2 ;;
    --uninstall)
      [ "$arg_count" -eq 1 ] || fail '--uninstall must be used alone'
      UNINSTALL=true
      shift ;;
    --help|-h) usage; exit 0 ;;
    *) fail 'unknown argument; use --help' ;;
  esac
done

[ "$(id -u)" -eq 0 ] || fail 'run as root (sudo sh install-agent.sh ...)'
command -v systemctl >/dev/null 2>&1 || fail 'systemd is required'
BIN=/usr/local/bin/moni-agent
ENV_FILE=/etc/moni/agent.env
UNIT=/etc/systemd/system/moni-agent.service

if [ "$UNINSTALL" = true ]; then
  if [ -f "$UNIT" ]; then systemctl disable --now moni-agent.service; fi
  rm -f "$UNIT" "$BIN" "$ENV_FILE"
  rmdir /etc/moni 2>/dev/null || :
  systemctl daemon-reload
  echo 'moni-agent uninstalled (system account moni retained)'
  exit 0
fi

[ -n "$SERVER" ] && [ -n "$TOKEN" ] && [ -n "$REPO" ] || fail 'required: --server, --token, --repo (or MONI_REPO)'
case "$REPO$VERSION$INTERVAL" in
  *[![:graph:]]*) fail '--repo, --version and --interval must not contain whitespace or control characters' ;;
esac
case "$SERVER" in ws://?*|wss://?*) ;; *) fail '--server must be a ws:// or wss:// URL' ;; esac
# EnvironmentFile values must not introduce quotes, escapes or additional lines.
case "$SERVER" in *[![:graph:]]*|*\"*|*\'*|*\\*) fail '--server contains unsupported characters' ;; esac
case "$TOKEN" in mn_?*) ;; *) fail '--token must be a moni token (mn_...)' ;; esac
case "$TOKEN" in *[!a-zA-Z0-9_-]*) fail '--token contains unsupported characters' ;; esac
printf '%s\n' "$REPO" | grep -Eq '^[A-Za-z0-9_-][A-Za-z0-9_.-]*/[A-Za-z0-9_-][A-Za-z0-9_.-]*$' || fail '--repo must be OWNER/REPO'
printf '%s\n' "$INTERVAL" | grep -Eq '^[1-9][0-9]{0,9}$' && [ "$INTERVAL" -le 4294967295 ] || fail '--interval must be 1..4294967295 seconds'
if [ -n "$VERSION" ]; then
  printf '%s\n' "$VERSION" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+([-+][A-Za-z0-9.-]+)?$' || fail '--version must be vX.Y.Z (optional prerelease/build suffix)'
fi
for dependency in curl sha256sum install getent groupadd useradd; do
  command -v "$dependency" >/dev/null 2>&1 || fail "required command not found: $dependency"
done
case "$(uname -m)" in
  x86_64|amd64) ARCH=amd64 ;;
  aarch64|arm64) ARCH=arm64 ;;
  *) fail 'supported architectures: amd64, arm64' ;;
esac

umask 077
tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/moni-install.XXXXXXXX")
binary_tmp=
env_tmp=
cleanup() {
  rm -rf -- "$tmp_dir"
  if [ -n "$binary_tmp" ]; then rm -f -- "$binary_tmp"; fi
  if [ -n "$env_tmp" ]; then rm -f -- "$env_tmp"; fi
}
trap cleanup 0
trap 'exit 130' INT
trap 'exit 143' TERM

ASSET=moni-agent-linux-$ARCH
BASE_URL=https://github.com/$REPO/releases/latest/download
if [ -n "$VERSION" ]; then BASE_URL=https://github.com/$REPO/releases/download/$VERSION; fi
curl -fsSL --retry 3 "$BASE_URL/$ASSET" -o "$tmp_dir/$ASSET"
curl -fsSL --retry 3 "$BASE_URL/SHA256SUMS" -o "$tmp_dir/SHA256SUMS"
# Only verify this asset; other manifest paths must never be read from the filesystem.
awk -v asset="$ASSET" 'NF == 2 && $2 == asset { print; count++ } END { if (count != 1) exit 1 }' \
  "$tmp_dir/SHA256SUMS" > "$tmp_dir/CHECKSUM" || fail 'SHA256SUMS must contain exactly one checksum for this architecture'
(cd "$tmp_dir" && sha256sum -c CHECKSUM) || fail 'SHA256 verification failed; installation unchanged'

getent group moni >/dev/null 2>&1 || groupadd --system moni
id moni >/dev/null 2>&1 || useradd --system --gid moni --no-create-home --shell /usr/sbin/nologin moni
[ "$(id -u moni)" -ne 0 ] || fail 'the moni account must not be root'

install -d -m 755 /usr/local/bin
install -d -m 750 -o root -g moni /etc/moni
# Rename on the same filesystem so a running agent can be upgraded atomically.
binary_tmp=$(mktemp "$BIN.XXXXXXXX")
install -m 755 -o root -g root "$tmp_dir/$ASSET" "$binary_tmp"
mv -f "$binary_tmp" "$BIN"
binary_tmp=
env_tmp=$(mktemp "$ENV_FILE.XXXXXXXX")
printf 'MONI_SERVER="%s"\nMONI_TOKEN="%s"\nMONI_INTERVAL=%s\n' "$SERVER" "$TOKEN" "$INTERVAL" > "$env_tmp"
chown root:moni "$env_tmp"
chmod 640 "$env_tmp"
mv -f "$env_tmp" "$ENV_FILE"
env_tmp=

cat > "$tmp_dir/moni-agent.service" <<'UNIT'
[Unit]
Description=moni-rs agent
After=network-online.target
Wants=network-online.target

[Service]
User=moni
Group=moni
EnvironmentFile=/etc/moni/agent.env
ExecStart=/usr/local/bin/moni-agent
Restart=on-failure
RestartPreventExitStatus=2 3
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
RestrictRealtime=true
LockPersonality=true
MemoryDenyWriteExecute=true
CapabilityBoundingSet=
# sysinfo queries interface addresses through netlink.
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK

[Install]
WantedBy=multi-user.target
UNIT
install -m 644 -o root -g root "$tmp_dir/moni-agent.service" "$UNIT"
systemctl daemon-reload
systemctl enable moni-agent.service
systemctl restart moni-agent.service
systemctl is-active --quiet moni-agent.service
echo 'moni-agent installed and running'
