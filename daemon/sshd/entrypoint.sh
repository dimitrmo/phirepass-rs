#!/usr/bin/env sh
set -eu

: "${SSH_USER:=sshuser}"
: "${SSH_PASSWORD:=changeme}"

# Ensure host keys exist (in case you mount /etc/ssh as a volume)
ssh-keygen -A >/dev/null 2>&1 || true

# Create user if missing
if ! id -u "$SSH_USER" >/dev/null 2>&1; then
  adduser -D -s /bin/bash "$SSH_USER"
fi

# Set/Reset password
echo "${SSH_USER}:${SSH_PASSWORD}" | chpasswd

exec "$@"
