#!/usr/bin/env bash
# Run seamint with the project .env from any cwd.
set -euo pipefail
cd "$(dirname "$0")"
exec ./target/release/seamint.exe "$@"
