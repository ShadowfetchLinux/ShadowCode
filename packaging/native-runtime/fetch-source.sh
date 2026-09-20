#!/usr/bin/env bash
# Sourced by build.sh. A mirror may improve availability, never change identity.
fetch_source() (
    set -euo pipefail
    local destination="$1" algorithm="$2" digest="$3" retained="$4"
    shift 4
    case "$algorithm" in
        sha256) [[ "$digest" =~ ^[a-f0-9]{64}$ ]] ;;
        sha512) [[ "$digest" =~ ^[a-f0-9]{128}$ ]] ;;
        *) echo "Unsupported source digest: $algorithm" >&2; exit 1 ;;
    esac
    verify() { printf '%s  %s\n' "$digest" "$1" | "${algorithm}sum" -c -; }
    # Retained source artifacts are authoritative build inputs. A damaged one
    # must fail rather than silently being replaced using the network.
    if [[ -e "$destination" ]]; then
        verify "$destination"
        exit
    fi
    mkdir -p "$(dirname "$destination")"
    local pending
    pending="$(mktemp "$destination.pending.XXXXXX")"
    trap 'rm -f -- "$pending"' EXIT
    if [[ -e "$retained" ]]; then
        cp -- "$retained" "$pending"
        verify "$pending"
        mv -- "$pending" "$destination"
        exit
    fi
    local url
    for url in "$@"; do
        if wget -q -T 30 -t 2 -O "$pending" "$url" && verify "$pending"; then
            mv -- "$pending" "$destination"
            exit
        fi
        echo "Pinned source unavailable or checksum mismatch: $url" >&2
    done
    echo "No verified source available for $destination" >&2
    exit 1
)
