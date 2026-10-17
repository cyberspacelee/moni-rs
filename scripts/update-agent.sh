#!/bin/sh
# Root-only updater. The unprivileged request contains a version, never a URL or command.
set -eu
export LC_ALL=C
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
umask 077

fail() { echo "moni-agent updater: $*" >&2; exit 1; }
valid_version() {
  case "$1" in *[!v0-9.]*) return 1 ;; esac
  printf '%s\n' "$1" | grep -Eq '^v(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$'
}
root_dir() {
  [ -d "$1" ] && [ ! -L "$1" ] || return 1
  metadata=$(stat -c '%u:%a' -- "$1")
  [ "${metadata%%:*}" = 0 ] && [ "$((0${metadata#*:} & 022))" -eq 0 ]
}
root_file() {
  [ -f "$1" ] && [ ! -L "$1" ] || return 1
  metadata=$(stat -c '%u:%a' -- "$1")
  [ "${metadata%%:*}" = 0 ] && [ "$((0${metadata#*:} & 022))" -eq 0 ]
}
version_of() {
  result=$(timeout 5 "$1" --version) || return 1
  case "$result" in 'moni-agent '*) result=v${result#moni-agent } ;; *) return 1 ;; esac
  valid_version "$result" || return 1
  printf '%s\n' "$result"
}
newer() {
  awk -v old="$1" -v new="$2" 'BEGIN {
    split(substr(old, 2), a, "."); split(substr(new, 2), b, ".");
    for (i = 1; i <= 3; i++) { if (b[i] > a[i]) exit 0; if (b[i] < a[i]) exit 1 }
    exit 1
  }'
}
download() {
  # Older curl versions only enforce --max-filesize when Content-Length is known.
  (ulimit -f "$((($3 + 511) / 512))"
    curl -fsSL --proto '=https' --proto-redir '=https' --connect-timeout 10 \
      --max-time 30 --retry 1 --max-filesize "$3" "$1" -o "$2")
  [ "$(stat -c '%s' -- "$2")" -le "$3" ] || fail 'download exceeded the size limit'
}

[ "$#" -eq 1 ] || fail 'expected --requested or --auto'
case "$1" in --requested|--auto) MODE=$1 ;; *) fail 'expected --requested or --auto' ;; esac
[ "$(id -u)" -eq 0 ] || fail 'root is required'
CONFIG=/etc/moni/update.conf
PUBLIC_KEY=/etc/moni/release.pub
REQUEST=/var/lib/moni/update-request
HEALTH=/var/lib/moni/health
STATE=/var/lib/moni-update
BIN=/usr/local/bin/moni-agent
for dependency in curl openssl sha256sum flock timeout stat install cmp sync; do
  command -v "$dependency" >/dev/null 2>&1 || fail 'required command is unavailable'
done
[ ! -L "$STATE" ] || fail 'unsafe updater state directory'
root_dir "${STATE%/*}" && root_dir "${BIN%/*}" && root_dir "${CONFIG%/*}" || fail 'unsafe updater parent directory'
if [ -e "$STATE" ]; then root_dir "$STATE" || fail 'unsafe updater state directory'; fi
install -d -m 700 -o root -g root "$STATE"
[ -d "${REQUEST%/*}" ] && [ ! -L "${REQUEST%/*}" ] || fail 'unsafe agent state directory'
agent_state_mode=$(stat -c '%a' -- "${REQUEST%/*}")
[ "$((0$agent_state_mode & 022))" -eq 0 ] || fail 'unsafe agent state permissions'
[ "$(stat -c '%d' -- "${REQUEST%/*}")" = "$(stat -c '%d' -- "$STATE")" ] || fail 'agent and updater state must share a filesystem'
root_file "$CONFIG" && root_file "$PUBLIC_KEY" && root_file "$BIN" || fail 'trusted configuration or binary has unsafe ownership or permissions'
[ "$(stat -c '%a' -- "$CONFIG")" = 600 ] || fail 'update.conf must have mode 600'
REPO=$(awk '/^REPO=/ { n++; sub(/^REPO=/, ""); print } END { if (n != 1) exit 1 }' "$CONFIG") || fail 'invalid update.conf'
printf '%s\n' "$REPO" | grep -Eq '^[A-Za-z0-9_-][A-Za-z0-9_.-]*/[A-Za-z0-9_-][A-Za-z0-9_.-]*$' || fail 'invalid pinned release repository'
key_bits=$(openssl rsa -pubin -in "$PUBLIC_KEY" -text -noout 2>/dev/null |
  sed -n 's/^Public-Key: (\([0-9][0-9]*\) bit)$/\1/p')
[ -n "$key_bits" ] && [ "$key_bits" -ge 3072 ] || fail 'a pinned RSA public key of at least 3072 bits is required'

exec 9> "$STATE/lock"
flock -w 120 9 || fail 'another update is still running'
work=$(mktemp -d "$STATE/work.XXXXXXXX")
binary_tmp=
rollback=false
restore_previous() {
  root_file "$STATE/previous" || return 1
  restore=$(mktemp "$BIN.XXXXXXXX")
  if install -m 755 -o root -g root "$STATE/previous" "$restore" && mv -f "$restore" "$BIN"; then
    sync "$BIN" "${BIN%/*}"
    systemctl restart moni-agent.service || return 1
    rm -f -- "$STATE/pending"
    sync "$STATE"
    return 0
  fi
  rm -f -- "$restore"
  return 1
}
main_pid() {
  pid=$(systemctl show --property MainPID --value moni-agent.service) || return 1
  case "$pid" in ''|0*|*[!0-9]*) return 1 ;; esac
  [ "${#pid}" -le 10 ] || return 1
  printf '%s\n' "$pid"
}
connected_health() {
  [ -e "$HEALTH" ] || [ -L "$HEALTH" ] || return 1
  rm -rf -- "$work/health"
  mv -fT -- "$HEALTH" "$work/health" || return 1
  [ -f "$work/health" ] && [ ! -L "$work/health" ] || return 1
  [ "$(stat -c '%s' -- "$work/health")" -le 128 ] || return 1
  dd if="$work/health" of="$work/health.content" bs=129 count=1 2>/dev/null || return 1
  printf '%s\n%s\n' "${VERSION#v}" "$1" > "$work/health.expected"
  cmp -s "$work/health.content" "$work/health.expected"
}
cleanup() {
  if [ "$rollback" = true ]; then
    if restore_previous; then
      echo 'moni-agent updater: restored the previous binary' >&2
    else
      echo 'moni-agent updater: rollback failed; restore the root-owned previous binary manually' >&2
    fi
  fi
  if [ -n "$binary_tmp" ]; then rm -f -- "$binary_tmp"; fi
  rm -rf -- "$work"
}
trap cleanup 0
trap 'exit 130' INT
trap 'exit 143' TERM

# An interrupted replacement is recovered before the next requested or automatic update.
if [ -e "$STATE/pending" ] || [ -L "$STATE/pending" ]; then
  root_file "$STATE/pending" && restore_previous || fail 'could not recover an interrupted update'
  echo 'moni-agent updater: recovered an interrupted update' >&2
fi

if [ "$MODE" = --requested ]; then
  # Rename into root-only storage before inspecting it, closing the symlink/type-check race.
  [ -e "$REQUEST" ] || [ -L "$REQUEST" ] || exit 0
  if ! mv -fT -- "$REQUEST" "$work/request"; then fail 'could not claim update request'; fi
  [ -f "$work/request" ] && [ ! -L "$work/request" ] || fail 'invalid update request'
  [ "$(stat -c '%s' -- "$work/request")" -le 64 ] || fail 'invalid update request'
  VERSION=$(dd if="$work/request" bs=65 count=1 2>/dev/null)
  valid_version "$VERSION" || fail 'invalid update request'
  printf '%s\n' "$VERSION" > "$work/request.canonical"
  cmp -s "$work/request" "$work/request.canonical" || fail 'invalid update request'
else
  latest=$(curl -fsSLI --proto '=https' --proto-redir '=https' --connect-timeout 10 \
    --max-time 30 --retry 1 --max-filesize 65536 -o /dev/null -w '%{url_effective}' \
    "https://github.com/$REPO/releases/latest") || fail 'could not resolve latest release'
  case "$latest" in
    "https://github.com/$REPO/releases/tag/"*) VERSION=${latest#"https://github.com/$REPO/releases/tag/"} ;;
    *) fail 'latest release resolved outside the pinned repository' ;;
  esac
  valid_version "$VERSION" || fail 'latest release must have a stable vX.Y.Z tag'
fi
CURRENT=$(version_of "$BIN") || fail 'installed binary has an unsupported version'
newer "$CURRENT" "$VERSION" || exit 0
case "$(uname -m)" in
  x86_64|amd64) ARCH=amd64 ;;
  aarch64|arm64) ARCH=arm64 ;;
  *) fail 'unsupported architecture' ;;
esac
ASSET=moni-agent-linux-$ARCH
BASE=https://github.com/$REPO/releases/download/$VERSION
download "$BASE/SHA256SUMS" "$work/SHA256SUMS" 1048576
download "$BASE/SHA256SUMS.sig" "$work/SHA256SUMS.sig" 16384
openssl dgst -sha256 -verify "$PUBLIC_KEY" -signature "$work/SHA256SUMS.sig" \
  "$work/SHA256SUMS" >/dev/null 2>&1 || fail 'release signature verification failed'
awk -v asset="$ASSET" 'NF == 2 && $2 == asset {
  count++; if (length($1) == 64 && $1 ~ /^[0-9a-fA-F]+$/) print; else bad = 1
} END { if (count != 1 || bad) exit 1 }' "$work/SHA256SUMS" > "$work/CHECKSUM" || fail 'invalid checksum manifest'
download "$BASE/$ASSET" "$work/$ASSET" 33554432
(cd "$work" && sha256sum -c CHECKSUM >/dev/null 2>&1) || fail 'binary checksum verification failed'
binary_tmp=$(mktemp "$BIN.XXXXXXXX")
install -m 755 -o root -g root "$work/$ASSET" "$binary_tmp"
[ "$(version_of "$binary_tmp")" = "$VERSION" ] || fail 'binary version does not match the release tag'
rm -f -- "$HEALTH"
install -m 755 -o root -g root "$BIN" "$STATE/previous"
sync "$STATE/previous"
printf '%s\n' "$VERSION" > "$STATE/pending"
sync "$STATE/pending" "$STATE"
rollback=true
mv -f "$binary_tmp" "$BIN"
binary_tmp=
sync "$BIN" "${BIN%/*}"
systemctl restart moni-agent.service || fail 'service restart failed'
connected=false
attempt=0
while [ "$attempt" -lt 45 ]; do
  if systemctl is-active --quiet moni-agent.service; then
    if active_pid=$(main_pid) && connected_health "$active_pid"; then connected=true; break; fi
  fi
  sleep 1
  attempt=$((attempt + 1))
done
[ "$connected" = true ] || fail 'new agent did not complete an authenticated connection'
sleep 2
systemctl is-active --quiet moni-agent.service && [ "$(main_pid)" = "$active_pid" ] || fail 'new agent did not remain healthy'
[ "$(version_of "$BIN")" = "$VERSION" ] || fail 'installed version changed during the update'
rm -f -- "$STATE/pending"
sync "$STATE"
rollback=false
echo 'moni-agent updater: signed update installed and service is active'
