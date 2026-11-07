#!/bin/sh
# Builds the app with cargo and copies the binary to where meson expects it.
# usage: cargo-build.sh <source dir> <target dir> <debug|release> <output> <app id> <locale dir>
set -e
source_dir=$1 target_dir=$2 profile=$3 output=$4
export CARGO_TARGET_DIR=$target_dir STICKY_APP_ID=$5 STICKY_LOCALEDIR=$6

if [ "$profile" = release ]; then
  cargo build --release --manifest-path "$source_dir/Cargo.toml" -p sticky-notes
else
  cargo build --manifest-path "$source_dir/Cargo.toml" -p sticky-notes
fi

cp "$target_dir/$profile/sticky-notes" "$output"
