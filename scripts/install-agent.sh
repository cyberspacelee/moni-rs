#!/bin/sh
# 安装 moni-agent 为 systemd 服务。
# 用法：curl -fsSL https://github.com/<owner>/<repo>/releases/latest/download/install-agent.sh \
#        | sudo sh -s -- --server wss://<host>/ws/agent --token <server_id>:<secret> [--repo owner/repo]
set -eu

REPO="${MONI_REPO:-}"
SERVER=""
TOKEN=""
INTERVAL=5
while [ $# -gt 0 ]; do
  case "$1" in
    --server) SERVER="$2"; shift 2 ;;
    --token) TOKEN="$2"; shift 2 ;;
    --interval) INTERVAL="$2"; shift 2 ;;
    --repo) REPO="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done
[ -n "$SERVER" ] && [ -n "$TOKEN" ] && [ -n "$REPO" ] || {
  echo "required: --server, --token, --repo (or MONI_REPO)" >&2; exit 1; }

case "$(uname -m)" in
  x86_64|amd64) ARCH=amd64 ;;
  aarch64|arm64) ARCH=arm64 ;;
  *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac

BIN=/usr/local/bin/moni-agent
URL="https://github.com/$REPO/releases/latest/download/moni-agent-linux-$ARCH"
echo "downloading $URL"
curl -fsSL "$URL" -o "$BIN.tmp"
chmod 755 "$BIN.tmp"
mv "$BIN.tmp" "$BIN"

id moni >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin moni

install -d -m 750 -o root -g moni /etc/moni
umask 027
cat > /etc/moni/agent.env <<ENV
MONI_SERVER=$SERVER
MONI_TOKEN=$TOKEN
MONI_INTERVAL=$INTERVAL
ENV
chgrp moni /etc/moni/agent.env

cat > /etc/systemd/system/moni-agent.service <<'UNIT'
[Unit]
Description=moni-rs agent
After=network-online.target
Wants=network-online.target

[Service]
User=moni
EnvironmentFile=/etc/moni/agent.env
ExecStart=/usr/local/bin/moni-agent
Restart=always
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable --now moni-agent
systemctl --no-pager status moni-agent | head -5
