#!/bin/sh
set -e
cd "$(dirname "$0")"
exec ./build --features server -- --serve-port 8080 --drafts --no-icons
