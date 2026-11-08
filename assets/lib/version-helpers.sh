#!/bin/bash

# A version is dotted numbers. Each segment compares as a number and a missing segment is zero.
version_cmp() {
    local one="$1" other="$2" left right
    local -a a b
    IFS=. read -r -a a <<< "$one"
    IFS=. read -r -a b <<< "$other"
    local at
    for ((at = 0; at < ${#a[@]} || at < ${#b[@]}; at++)); do
        left="${a[at]:-0}"
        right="${b[at]:-0}"
        left="${left#"${left%%[!0]*}"}"
        right="${right#"${right%%[!0]*}"}"
        if [ "${#left}" -ne "${#right}" ]; then
            if [ "${#left}" -gt "${#right}" ]; then
                echo 1
            else
                echo -1
            fi
            return
        fi
        if [ "$left" != "$right" ]; then
            if [ "$left" \> "$right" ]; then
                echo 1
            else
                echo -1
            fi
            return
        fi
    done
    echo 0
}

dotted_version() {
    case "$1" in
        "" | .* | *. | *..* | *[!0-9.]*) return 1 ;;
        *) return 0 ;;
    esac
}

# The Debian version policy adds an epoch and a revision to the upstream version.
upstream_version() {
    local version="$1"
    version="${version#*:}"
    version="${version%-*}"
    printf '%s\n' "$version"
}

query_installed() {
    local family="$1" package="$2" version
    case "$family" in
        fedora | rhel) version="$(rpm -q --qf '%{VERSION}' -- "$package" 2> /dev/null)" || return 1 ;;
        debian | ubuntu)
            version="$(dpkg-query -W -f='${Version}' -- "$package" 2> /dev/null)" || return 1
            version="$(upstream_version "$version")"
            ;;
        *) return 1 ;;
    esac
    [ -n "$version" ] || return 1
    printf '%s\n' "$version"
}

# A base claims a capability it installs from its package database. An uninstalled package takes the database's candidate.
query_version() {
    local family="$1" package="$2" version
    if version="$(query_installed "$family" "$package")"; then
        printf '%s\n' "$version"
        return
    fi
    case "$family" in
        fedora | rhel)
            if command -v dnf5 > /dev/null; then
                version="$(dnf5 -q repoquery --latest-limit 1 --qf '%{version}' -- "$package" 2> /dev/null | sed -n '1p')"
            else
                version="$(dnf -q repoquery --latest-limit 1 --qf '%{version}' -- "$package" 2> /dev/null | sed -n '1p')"
            fi
            ;;
        debian | ubuntu)
            version="$(apt-cache policy -- "$package" 2> /dev/null \
                | awk '$1 == "Candidate:" { candidate = $2 } END { print candidate }')"
            case "$version" in
                "" | "(none)") return 1 ;;
            esac
            version="$(upstream_version "$version")"
            ;;
        *) return 1 ;;
    esac
    [ -n "$version" ] || return 1
    printf '%s\n' "$version"
}

refresh_metadata() {
    case "$1" in
        debian | ubuntu) apt-get update -qq ;;
        fedora | rhel) : ;;
        *) return 1 ;;
    esac
}

# A provider of a child capability meets a floor on its parent. The resolver decides the same way for a manifest.
measured_version() {
    local file="$1" name="$2"
    awk -F'|' -v name="$name" '$2 == name || index($2, name "/") == 1 { print $4; exit }' "$file"
}
