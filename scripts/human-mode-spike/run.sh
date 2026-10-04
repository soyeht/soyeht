#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
source_dir="$root/scripts/human-mode-spike"
build_dir="$(mktemp -d /tmp/soyeht-human-spike.XXXXXX)"
output_dir="${1:-$build_dir/results}"
mkdir -p "$output_dir"

swiftc "$source_dir/EventPoster.swift" -framework CoreGraphics -o "$build_dir/poster"
swiftc "$source_dir/Runner.swift" -framework AppKit -framework WebKit -o "$build_dir/runner"

port=18765
uv run python -m http.server "$port" --bind 127.0.0.1 --directory "$source_dir" >"$build_dir/http.log" 2>&1 &
server_pid=$!
trap 'kill "$server_pid" 2>/dev/null || true' EXIT

SOYEHT_HUMAN_SPIKE_POSTER="$build_dir/poster" "$build_dir/runner" process "$output_dir/process.json"
SOYEHT_HUMAN_SPIKE_POSTER="$build_dir/poster" "$build_dir/runner" global "$output_dir/global.json"

echo "human-mode-spike: results in $output_dir"
