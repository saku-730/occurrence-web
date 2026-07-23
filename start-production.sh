#!/usr/bin/env bash

# Start the already-built production services from any working directory.
# PostgreSQL and Fuseki must already be running and reachable from backend/.env.
set -Eeuo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
backend_binary="$repo_dir/backend/target/release/backend"
frontend_build_id="$repo_dir/frontend/.next/BUILD_ID"
garage_config="${GARAGE_CONFIG_FILE:-$repo_dir/garage/garage.toml}"
service_pids=()

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'Required command is not available: %s\n' "$1" >&2
    exit 1
  fi
}

cleanup() {
  local exit_code=$?

  trap - EXIT INT TERM

  if ((${#service_pids[@]} > 0)); then
    printf '\nStopping production services...\n'
    kill "${service_pids[@]}" 2>/dev/null || true
    wait "${service_pids[@]}" 2>/dev/null || true
  fi

  exit "$exit_code"
}

trap cleanup EXIT INT TERM

require_command garage
require_command npm


if [[ ! -f "$garage_config" ]]; then
  printf 'Garage configuration was not found: %s\n' "$garage_config" >&2
  exit 1
fi

if [[ ! -x "$backend_binary" ]]; then
  printf 'Release backend binary was not found: %s\n' "$backend_binary" >&2
  printf 'Build it separately with: (cd backend && cargo build --release)\n' >&2
  exit 1
fi

if [[ ! -f "$frontend_build_id" ]]; then
  printf 'Next.js production build was not found: %s\n' "$frontend_build_id" >&2
  printf 'Build it separately with: (cd frontend && npm run build)\n' >&2
  exit 1
fi

printf 'Starting Garage with %s...\n' "$garage_config"
(
  cd "$repo_dir"
  exec env GARAGE_CONFIG_FILE="$garage_config" garage server
) &
service_pids+=("$!")

printf 'Starting Rust backend at http://127.0.0.1:3001...\n'
(
  cd "$repo_dir/backend"
  exec "$backend_binary"
) &
service_pids+=("$!")

printf 'Starting Next.js production server at http://127.0.0.1:3002...\n'
(
  cd "$repo_dir/frontend"
  exec env HOSTNAME="127.0.0.1" npm run start
) &
service_pids+=("$!")

# All three services are one foreground process group for this simple
# deployment. If any one exits, stop the others to avoid a partial service.
wait -n "${service_pids[@]}"
