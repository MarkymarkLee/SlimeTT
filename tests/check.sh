#!/bin/sh
# Every check run before a change is called done: formatting, lints (warnings are errors), unit tests.
# Layout changes are also checked by eye with the popup render described in the README (STT_SHOT).
set -e
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
