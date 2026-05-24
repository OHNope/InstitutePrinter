#!/bin/sh
set -eu

target="${1:-}"
profile="${2:-release}"

case "$profile" in
    debug)
        profile_flag=""
        ;;
    release)
        profile_flag="--release"
        ;;
    *)
        echo "unknown profile: $profile" >&2
        exit 2
        ;;
esac

if [ -n "$target" ]; then
    cargo build $profile_flag --bin titech-lpd --target "$target"
    cp "target/$target/$profile/titech-lpd" assets/titech-lpd
    cargo build $profile_flag --bin installer --target "$target"
    echo "Built target/$target/$profile/installer"
else
    cargo build $profile_flag --bin titech-lpd
    cp "target/$profile/titech-lpd" assets/titech-lpd
    cargo build $profile_flag --bin installer
    echo "Built target/$profile/installer"
fi
