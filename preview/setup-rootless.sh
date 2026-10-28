#!/bin/sh
# Dedicated Debian preview runtime. Does not alter the existing Docker daemon.
set -eu
[ "$(id -u)" = 0 ] || { echo 'Run as root via the documented deploy account.' >&2; exit 1; }
if getent passwd hnpreview >/dev/null; then
  echo 'hnpreview already exists; inspect it before repeating provisioning.' >&2
  exit 1
fi
apt-get install -y --no-install-recommends --no-upgrade uidmap slirp4netns
useradd --create-home --user-group --shell /usr/sbin/nologin hnpreview
chmod 700 /home/hnpreview
preview_uid=$(id -u hnpreview)
# Bound the entire experiment, including daemon and every visitor session.
mkdir -p "/etc/systemd/system/user-${preview_uid}.slice.d" "/etc/systemd/system/user@${preview_uid}.service.d"
cat > "/etc/systemd/system/user-${preview_uid}.slice.d/hn-preview.conf" <<'EOF'
[Slice]
MemoryMax=768M
CPUQuota=100%
TasksMax=256
EOF
cat > "/etc/systemd/system/user@${preview_uid}.service.d/hn-preview.conf" <<'EOF'
[Service]
Delegate=cpu cpuset io memory pids
EOF
systemctl daemon-reload
loginctl enable-linger hnpreview
systemctl start "user@${preview_uid}.service"
sudo -H -u hnpreview env XDG_RUNTIME_DIR="/run/user/${preview_uid}" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/${preview_uid}/bus" dockerd-rootless-setuptool.sh install --force
sudo -H -u hnpreview env XDG_RUNTIME_DIR="/run/user/${preview_uid}" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/${preview_uid}/bus" systemctl --user start dbus
id hnpreview
sudo -H -u hnpreview env DOCKER_HOST="unix:///run/user/${preview_uid}/docker.sock" docker info --format '{{.ServerVersion}} {{.CgroupVersion}} {{.CgroupDriver}}'
systemctl show "user-${preview_uid}.slice" -p MemoryMax -p CPUQuotaPerSecUSec -p TasksMax
