#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
case "${1:-gui}" in
    gui)
        if [ -n "${WALLWEAVE_HOME:-}" ]; then
            log_dir="$WALLWEAVE_HOME/state"
        else
            log_dir="${XDG_STATE_HOME:-$HOME/.local/state}/wallweave"
        fi
        # Desktop launchers usually discard stderr, including startup failures.
        # Keep this launch and the previous one, with private permissions.
        umask 077
        if mkdir -p -- "$log_dir"; then
            if [ -f "$log_dir/gui.log" ]; then
                mv -f -- "$log_dir/gui.log" "$log_dir/gui-previous.log" || :
            fi
            exec >> "$log_dir/gui.log" 2>&1
            printf '\nWallweave launcher: %s\n' "$(date -Iseconds)"
        fi
        RUST_BACKTRACE=${RUST_BACKTRACE:-1}
        export RUST_BACKTRACE
        ;;
esac
if [ -x "$project_dir/bin/wallweave" ]; then
    if [ ! -f "$project_dir/bin/libsteam_api.so" ]; then
        printf '%s\n' 'Steamworks runtime missing. Rebuild with: python3 scripts/build-app.py' >&2
        exit 1
    fi
    exec "$project_dir/bin/wallweave" "$@"
fi
exec cargo run --release --locked --manifest-path "$project_dir/Cargo.toml" -- "$@"
