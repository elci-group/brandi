#!/usr/bin/env bash
set -euo pipefail

output_dir="${1:-supply-chain}"
mkdir -p "$output_dir"

command -v cargo-cyclonedx >/dev/null 2>&1 || {
  echo "cargo-cyclonedx is required; install the version pinned by the supply-chain workflow" >&2
  exit 2
}
command -v cyclonedx-gomod >/dev/null 2>&1 || {
  echo "cyclonedx-gomod is required; install the version pinned by the supply-chain workflow" >&2
  exit 2
}
command -v cargo-deny >/dev/null 2>&1 || {
  echo "cargo-deny is required" >&2
  exit 2
}

cargo cyclonedx --format json --all
mv brandi.cdx.json "$output_dir/brandi-rust.cdx.json"
cargo deny list --format json > "$output_dir/brandi-rust-licenses.json"
(
  cd tui
  cyclonedx-gomod mod -json -output "../$output_dir/brandi-go.cdx.json"
  go list -m -json all > "../$output_dir/brandi-go-modules.json"
)
sha256sum "$output_dir"/* > "$output_dir/SHA256SUMS"
