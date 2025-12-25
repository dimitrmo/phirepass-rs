#!/usr/bin/env sh
set -eu

: "${SSH_USER:=sshuser}"
: "${SSH_PASSWORD:=changeme}"
: "${LANG:=en_US.UTF-8}"
: "${LC_ALL:=$LANG}"

ssh-keygen -A >/dev/null 2>&1 || true

if ! id -u "$SSH_USER" >/dev/null 2>&1; then
  adduser -D -s /bin/bash "$SSH_USER"
fi

echo "${SSH_USER}:${SSH_PASSWORD}" | chpasswd

cat >/etc/profile.d/locale.sh <<EOF
export LANG="$LANG"
export LC_ALL="$LC_ALL"
EOF

# Start sshd as a child so we can handle Ctrl+C (SIGINT)
/usr/sbin/sshd -D -e &
pid="$!"

# On Ctrl+C or docker stop, terminate sshd and exit
trap 'kill -TERM "$pid" 2>/dev/null; wait "$pid"' INT TERM

wait "$pid"
