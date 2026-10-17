#!/bin/sh
# 用法：sudo sh install-agent.sh --server wss://<host>/ws/agent --token mn_xxx --repo owner/repo --update-public-key FILE
set -eu
export LC_ALL=C
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

fail() { echo "moni-agent: $*" >&2; exit 1; }
usage() {
  echo 'Usage: install-agent.sh --server URL --token TOKEN --repo OWNER/REPO [--interval SECONDS] [--version vX.Y.Z]'
  echo '       [--update-public-key FILE] [--auto-update] [--reconfigure]'
  echo '       First installation requires --update-public-key; reinstall reuses the pinned local key.'
  echo '       install-agent.sh --uninstall'
}

REPO=${MONI_REPO:-}
SERVER=
TOKEN=
INTERVAL=2
VERSION=
UPDATE_PUBLIC_KEY=
AUTO_UPDATE=false
RECONFIGURE=false
UNINSTALL=false
arg_count=$#
while [ "$#" -gt 0 ]; do
  case "$1" in
    --server|--token|--repo|--interval|--version|--update-public-key)
      [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 requires a value"
      case "$2" in --*) fail "$1 requires a value" ;; esac
      case "$1" in
        --server) SERVER=$2 ;;
        --token) TOKEN=$2 ;;
        --repo) REPO=$2 ;;
        --interval) INTERVAL=$2 ;;
        --version) VERSION=$2 ;;
        --update-public-key) UPDATE_PUBLIC_KEY=$2 ;;
      esac
      shift 2 ;;
    --auto-update) AUTO_UPDATE=true; shift ;;
    --reconfigure) RECONFIGURE=true; shift ;;
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
UPDATE_HELPER=/usr/local/libexec/moni-agent-update.sh
UPDATE_CONF=/etc/moni/update.conf
PUBLIC_KEY=/etc/moni/release.pub
STATE=/var/lib/moni-update
REQUEST_DIR=/var/lib/moni

if [ "$UNINSTALL" = true ]; then
  for name in moni-agent-update.path moni-agent-update.timer; do
    if [ -f "/etc/systemd/system/$name" ]; then systemctl disable --now "$name"; fi
  done
  for name in moni-agent-update.service moni-agent-update-auto.service; do
    if [ -f "/etc/systemd/system/$name" ]; then systemctl stop "$name"; fi
  done
  if [ -f "$UNIT" ]; then systemctl disable --now moni-agent.service; fi
  rm -f "$UNIT" "$BIN" "$ENV_FILE" "$UPDATE_HELPER" "$UPDATE_CONF" "$PUBLIC_KEY" \
    /etc/systemd/system/moni-agent-update.service /etc/systemd/system/moni-agent-update-auto.service \
    /etc/systemd/system/moni-agent-update.path /etc/systemd/system/moni-agent-update.timer
  rm -f "$REQUEST_DIR/update-request" "$REQUEST_DIR/health"
  rmdir "$REQUEST_DIR" 2>/dev/null || :
  [ ! -L "$STATE" ] || fail 'unsafe updater state directory'
  rm -rf -- "$STATE"
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
  printf '%s\n' "$VERSION" | grep -Eq '^v(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$' || fail '--version must be a stable vX.Y.Z tag'
fi
for dependency in curl openssl sha256sum install getent groupadd useradd stat flock timeout; do
  command -v "$dependency" >/dev/null 2>&1 || fail "required command not found: $dependency"
done
case "$(uname -m)" in
  x86_64|amd64) ARCH=amd64 ;;
  aarch64|arm64) ARCH=arm64 ;;
  *) fail 'supported architectures: amd64, arm64' ;;
esac
[ ! -L /etc/moni ] && [ ! -L "$ENV_FILE" ] && [ ! -L "$REQUEST_DIR" ] || fail 'unsafe configuration or state path'

KEY_SOURCE=$UPDATE_PUBLIC_KEY
if [ -z "$KEY_SOURCE" ]; then
  [ -f "$PUBLIC_KEY" ] && [ -f "$UPDATE_CONF" ] || fail 'first installation requires a trusted --update-public-key'
  for trusted_path in /etc/moni "$PUBLIC_KEY" "$UPDATE_CONF"; do
    [ ! -L "$trusted_path" ] || fail 'unsafe update configuration'
    metadata=$(stat -c '%u:%a' -- "$trusted_path")
    [ "${metadata%%:*}" = 0 ] && [ "$((0${metadata#*:} & 022))" -eq 0 ] || fail 'unsafe update configuration permissions'
  done
  [ "$(stat -c '%a' -- "$UPDATE_CONF")" = 600 ] || fail 'update.conf must have mode 600'
  configured_repo=$(awk '/^REPO=/ { n++; sub(/^REPO=/, ""); print } END { if (n != 1) exit 1 }' "$UPDATE_CONF") || fail 'invalid update configuration'
  [ "$configured_repo" = "$REPO" ] || fail 'changing the pinned repository requires --update-public-key'
  KEY_SOURCE=$PUBLIC_KEY
fi
[ -f "$KEY_SOURCE" ] || fail 'public key file not found'

umask 077
tmp_dir=$(mktemp -d /tmp/moni-install.XXXXXXXX)
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

cp "$KEY_SOURCE" "$tmp_dir/release.pub"
KEY_SOURCE=$tmp_dir/release.pub
key_bits=$(openssl rsa -pubin -in "$KEY_SOURCE" -text -noout 2>/dev/null |
  sed -n 's/^Public-Key: (\([0-9][0-9]*\) bit)$/\1/p')
[ -n "$key_bits" ] && [ "$key_bits" -ge 3072 ] || fail 'an RSA public key of at least 3072 bits is required'

ASSET=moni-agent-linux-$ARCH
download() {
  (ulimit -f "$((($3 + 511) / 512))"
    curl -fsSL --proto '=https' --proto-redir '=https' --connect-timeout 10 \
      --max-time 30 --retry 1 --max-filesize "$3" "$1" -o "$2")
  [ "$(stat -c '%s' -- "$2")" -le "$3" ] || fail 'download exceeded the size limit'
}
if [ -z "$VERSION" ]; then
  latest=$(curl -fsSLI --proto '=https' --proto-redir '=https' --connect-timeout 10 \
    --max-time 30 --retry 1 --max-filesize 65536 -o /dev/null -w '%{url_effective}' \
    "https://github.com/$REPO/releases/latest") || fail 'could not resolve latest release'
  case "$latest" in
    "https://github.com/$REPO/releases/tag/"*) VERSION=${latest#"https://github.com/$REPO/releases/tag/"} ;;
    *) fail 'latest release resolved outside the requested repository' ;;
  esac
  printf '%s\n' "$VERSION" | grep -Eq '^v(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$' || fail 'latest release must have a stable vX.Y.Z tag'
fi
BASE_URL=https://github.com/$REPO/releases/download/$VERSION
download "$BASE_URL/SHA256SUMS" "$tmp_dir/SHA256SUMS" 1048576
download "$BASE_URL/SHA256SUMS.sig" "$tmp_dir/SHA256SUMS.sig" 16384
openssl dgst -sha256 -verify "$KEY_SOURCE" -signature "$tmp_dir/SHA256SUMS.sig" \
  "$tmp_dir/SHA256SUMS" >/dev/null 2>&1 || fail 'release signature verification failed; installation unchanged'
download "$BASE_URL/$ASSET" "$tmp_dir/$ASSET" 33554432
# Only verify this asset; other manifest paths must never be read from the filesystem.
checksum() {
  awk -v asset="$1" 'NF == 2 && $2 == asset {
    count++; if (length($1) == 64 && $1 ~ /^[0-9a-fA-F]+$/) print; else bad = 1
  } END { if (count != 1 || bad) exit 1 }' "$tmp_dir/SHA256SUMS" > "$tmp_dir/CHECKSUM" || fail 'invalid checksum manifest'
  (cd "$tmp_dir" && sha256sum -c CHECKSUM) || fail 'SHA256 verification failed; installation unchanged'
}
checksum "$ASSET"
download "$BASE_URL/update-agent.sh" "$tmp_dir/update-agent.sh" 131072
checksum update-agent.sh

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
if [ -f "$ENV_FILE" ]; then
  awk -v reconfigure="$RECONFIGURE" '
    /^[[:space:]]*MONI_UPDATE_REPO=/ { next }
    reconfigure == "true" && /^[[:space:]]*MONI_(SERVER|TOKEN|INTERVAL)=/ { next }
    { print }
  ' "$ENV_FILE" > "$env_tmp"
else
  : > "$env_tmp"
fi
if [ ! -f "$ENV_FILE" ] || [ "$RECONFIGURE" = true ]; then
  printf 'MONI_SERVER="%s"\nMONI_TOKEN="%s"\nMONI_INTERVAL=%s\n' "$SERVER" "$TOKEN" "$INTERVAL" >> "$env_tmp"
fi
printf '\nMONI_UPDATE_REPO="%s"\n' "$REPO" >> "$env_tmp"
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
StateDirectory=moni
StateDirectoryMode=0700
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
install -d -m 755 -o root -g root /usr/local/libexec
install -d -m 700 -o moni -g moni "$REQUEST_DIR"
[ ! -L "$STATE" ] || fail 'unsafe updater state directory'
install -d -m 700 -o root -g root "$STATE"
install -m 755 -o root -g root "$tmp_dir/update-agent.sh" "$UPDATE_HELPER"
install -m 644 -o root -g root "$tmp_dir/release.pub" "$PUBLIC_KEY"
printf 'REPO=%s\n' "$REPO" > "$tmp_dir/update.conf"
install -m 600 -o root -g root "$tmp_dir/update.conf" "$UPDATE_CONF"
cat > "$tmp_dir/moni-agent-update.service" <<'UPDATE'
[Unit]
Description=Install a signed moni-agent update
After=network-online.target
Wants=network-online.target
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=oneshot
User=root
Group=root
ExecStart=/usr/local/libexec/moni-agent-update.sh --requested
TimeoutStartSec=450
UMask=0077
StateDirectory=moni-update
StateDirectoryMode=0700
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/usr/local/bin /var/lib/moni /var/lib/moni-update
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
UPDATE
sed 's/--requested/--auto/' "$tmp_dir/moni-agent-update.service" > "$tmp_dir/moni-agent-update-auto.service"
cat > "$tmp_dir/moni-agent-update.path" <<'UPDATE'
[Unit]
Description=Watch for moni-agent update requests

[Path]
PathChanged=/var/lib/moni/update-request
PathExists=/var/lib/moni/update-request
Unit=moni-agent-update.service

[Install]
WantedBy=multi-user.target
UPDATE
cat > "$tmp_dir/moni-agent-update.timer" <<'UPDATE'
[Unit]
Description=Check for signed moni-agent updates

[Timer]
OnBootSec=10min
OnUnitActiveSec=6h
RandomizedDelaySec=30min
Persistent=true
Unit=moni-agent-update-auto.service

[Install]
WantedBy=timers.target
UPDATE
for name in moni-agent-update.service moni-agent-update-auto.service moni-agent-update.path moni-agent-update.timer; do
  install -m 644 -o root -g root "$tmp_dir/$name" "/etc/systemd/system/$name"
done
systemctl daemon-reload
systemctl enable --now moni-agent-update.path
if [ "$AUTO_UPDATE" = true ]; then systemctl enable --now moni-agent-update.timer; fi
systemctl enable moni-agent.service
systemctl restart moni-agent.service
systemctl is-active --quiet moni-agent.service
echo 'moni-agent installed and running'
