#!/usr/bin/env bash
set -euo pipefail

# The same pinned tools ship in CI packages and official releases.
MAC_FFMPEG_BASE=https://ffmpeg.martin-riedl.de/download/macos
MAC_FFMPEG_ARM64=arm64/1789931890_9.0.2
MAC_FFMPEG_ARM64_SHA256=c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924
MAC_FFPROBE_ARM64_SHA256=fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6
MAC_FFMPEG_X64=amd64/1789931006_9.0.2
MAC_FFMPEG_X64_SHA256=7c6b4125b191cbf773832dc51f424cf2b6bb7da43007d1e066f95909e47cacd4
MAC_FFPROBE_X64_SHA256=2322438ed2f6319a691291b247d09c69dcaa3a982460d1f269a7e1af335cfdfd

dl=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/mgr-ffmpeg.XXXXXX")
trap 'rm -rf "$dl"' EXIT
mkdir -p "$dl/arm64" "$dl/x64" src-tauri/binaries
fetch() { # architecture build tool checksum
    curl -fsSL --retry 3 -o "$dl/$1/$3.zip" "$MAC_FFMPEG_BASE/$2/$3.zip"
    echo "$4  $dl/$1/$3.zip" | shasum -a 256 --check
    unzip -o -q "$dl/$1/$3.zip" -d "$dl/$1" "$3"
}
fetch arm64 "$MAC_FFMPEG_ARM64" ffmpeg "$MAC_FFMPEG_ARM64_SHA256"
fetch arm64 "$MAC_FFMPEG_ARM64" ffprobe "$MAC_FFPROBE_ARM64_SHA256"
fetch x64 "$MAC_FFMPEG_X64" ffmpeg "$MAC_FFMPEG_X64_SHA256"
fetch x64 "$MAC_FFMPEG_X64" ffprobe "$MAC_FFPROBE_X64_SHA256"
for tool in ffmpeg ffprobe; do
    output="src-tauri/binaries/$tool"
    lipo -create "$dl/arm64/$tool" "$dl/x64/$tool" -output "$output"
    bash .github/scripts/verify-universal-macos.sh "$output"
    chmod +x "$output"
    # lipo drops the input signatures. Release CI replaces this local signature with Developer ID.
    codesign --force --sign - "$output"
    "$output" -version
done
encoders=$(src-tauri/binaries/ffmpeg -hide_banner -encoders)
for encoder in h264_videotoolbox libopus libx264; do
    grep -E "[[:space:]]${encoder}[[:space:]]" <<< "$encoders"
done
