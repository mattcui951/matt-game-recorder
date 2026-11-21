#!/usr/bin/env bash
set -euo pipefail

archs=$(lipo -archs "$1")
for arch in arm64 x86_64; do
    if [[ " $archs " != *" $arch "* ]]; then
        echo "$1 is missing $arch (found: $archs)" >&2
        exit 1
    fi
done
