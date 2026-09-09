#!/usr/bin/env bash
# Install the checked-in Theosis timer as a user service. Theosis performs the
# version comparison, delegates builds to Baby, and verifies the installed
# Brandi binary before reporting success.
set -euo pipefail

if ! command -v theosis >/dev/null 2>&1; then
  echo "theosis is required at ~/.local/bin/theosis; install it before enabling updates" >&2
  exit 1
fi
if ! command -v baby >/dev/null 2>&1; then
  echo "baby is required at ~/.local/bin/baby; install it before enabling updates" >&2
  exit 1
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)
systemd_user_dir=${XDG_CONFIG_HOME:-"$HOME/.config"}/systemd/user

install -Dm644 "$project_dir/packaging/systemd/brandi-theosis.service" \
  "$systemd_user_dir/brandi-theosis.service"
install -Dm644 "$project_dir/packaging/systemd/brandi-theosis.timer" \
  "$systemd_user_dir/brandi-theosis.timer"
systemctl --user daemon-reload
systemctl --user enable --now brandi-theosis.timer
systemctl --user start brandi-theosis.service
