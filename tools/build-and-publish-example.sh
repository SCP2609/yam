#!/bin/sh
# Example PC helper. It builds and uploads one target without printing secrets.
set -eu

TARGET=${1:?"Usage: $0 <target> [source-dir] [artifact-path]"}
SOURCE=${2:-.}
ARTIFACT=${3:-"$SOURCE/dist/package-$TARGET.yampkg"}

: "${YAM_PUBLISH_TOKEN:?Set the publish token from https://repo.yse.wtf first}"
: "${YAM_AUTHOR_KEY_FILE:?Set path to the downloaded Ed25519 PEM private key}"

mkdir -p "$(dirname "$ARTIFACT")"
./target/release/yam-publish release \
  --source "$SOURCE" \
  --platform "$TARGET" \
  --output "$ARTIFACT" \
  --token "$YAM_PUBLISH_TOKEN" \
  --key "$YAM_AUTHOR_KEY_FILE"
