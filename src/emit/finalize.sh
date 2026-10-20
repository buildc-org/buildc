# ---- tmpfiles helpers ----
# systemd-tmpfiles decodes C escapes and `%` specifiers in a path, so a literal path must spell those bytes as escapes.
tmpfiles_escape() {
    local s=$1
    s=${s//\\/\\x5c}
    s=${s//%/%%}
    s=${s//$'\t'/\\x09}
    s=${s//$'\n'/\\x0a}
    s=${s//$'\r'/\\x0d}
    s=${s//$'\v'/\\x0b}
    s=${s//$'\f'/\\x0c}
    s=${s//\'/\\x27}
    s=${s//\"/\\x22}
    s=${s// /\\x20}
    printf '%s' "$s"
}

declare -A tmpfiles_declared=()
read_declared() {
    local verb path
    tmpfiles_declared=()
    while read -r verb path _; do
        case "$verb" in '' | \#*) continue ;; esac
        [ -n "$path" ] && tmpfiles_declared[$path]=1
    done < <(cat /usr/lib/tmpfiles.d/*.conf /etc/tmpfiles.d/*.conf 2> /dev/null)
}

claim() {
    [ -z "${tmpfiles_declared[$1]:-}" ] || return 1
    tmpfiles_declared[$1]=1
}

relocate() {
    local src=$1 store=$2 link=$3 verb=$4 conf=$5 esc
    rm -rf "$store"
    mkdir -p "$(dirname "$store")"
    cp -a "$src" "$store"
    esc="$(tmpfiles_escape "$link")"
    claim "$esc" || return 0
    printf '%s %s - - - - %s\n' "$verb" "$esc" "$(tmpfiles_escape "$store")" >> "$conf"
}

declare_dir() {
    local esc
    esc="$(tmpfiles_escape "$1")"
    claim "$esc" || return 0
    printf 'd %s 0755 root root -\n' "$esc" >> "$2"
}

shopt -s dotglob

# ---- /opt relocation ----
tmpfiles=/usr/lib/tmpfiles.d/zz-opt-symlinks.conf
read_declared
declare_dir /var/opt "$tmpfiles"
for d in /opt/*/; do
    [ -d "$d" ] || continue
    name="$(basename "$d")"
    relocate "$d" "/usr/lib/opt/${name}" "/var/opt/${name}" L+ "$tmpfiles"
done
rm -rf /opt
mv /opt.bak /opt

# ---- /usr/local relocation ----
if [ -L /usr/local.bak ]; then
    tmpfiles=/usr/lib/tmpfiles.d/zz-usrlocal-symlinks.conf
    read_declared
    declare_dir /var/usrlocal "$tmpfiles"
    for entry in /usr/local/*; do
        [ -e "$entry" ] || [ -L "$entry" ] || continue
        name="$(basename "$entry")"
        if [ -d "$entry" ] && [ ! -L "$entry" ]; then
            for child in "$entry"/*; do
                [ -e "$child" ] || [ -L "$child" ] || continue
                declare_dir "/var/usrlocal/${name}" "$tmpfiles"
                rel="${name}/$(basename "$child")"
                relocate "$child" "/usr/lib/usrlocal/${rel}" "/var/usrlocal/${rel}" L "$tmpfiles"
            done
            continue
        fi
        relocate "$entry" "/usr/lib/usrlocal/${name}" "/var/usrlocal/${name}" L "$tmpfiles"
    done
    # shellcheck disable=SC2114 # relocate already copied every entry of this /usr/local
    rm -rf /usr/local
    mv /usr/local.bak /usr/local
fi
shopt -u dotglob

# ---- module presets ----
apply_module_presets() {
    local scope="$1" dir="$2" flag=() f verb unit
    [ "$scope" = user ] && flag=(--global)
    for f in "$dir"/45-module-*.preset; do
        [ -f "$f" ] || continue
        while read -r verb unit; do
            case "$verb" in
                enable) systemctl "${flag[@]}" enable "$unit" ;;
                disable) systemctl "${flag[@]}" disable "$unit" ;;
                *) ;;
            esac
        done < "$f"
    done
}
apply_module_presets system /usr/lib/systemd/system-preset
apply_module_presets user /usr/lib/systemd/user-preset

# ---- /var to tmpfiles ----
var_conf=/usr/lib/tmpfiles.d/zz-var-autogen.conf

# `/var/cache`, `/var/log` and `/var/run` hold runtime state, so the tmpfiles conversion leaves them and the package database at `/var/lib/dpkg` alone.
var_keep=(/var/cache /var/log /var/run /var/lib/dpkg)

touch "$var_conf"
read_declared

var_prune=()
for keep in "${var_keep[@]}"; do
    var_prune+=(-path "$keep" -o)
done
unset 'var_prune[-1]'

var_remove=()
while IFS=$'\t' read -r -d '' type mode owner group path; do
    esc="$(tmpfiles_escape "$path")"
    claim "$esc" || continue
    var_remove+=("$path")
    case "$type" in
        d) printf 'd %s %s %s %s -\n' "$esc" "$mode" "$owner" "$group" >> "$var_conf" ;;
        l) printf 'L+ %s - - - - %s\n' "$esc" "$(tmpfiles_escape "$(readlink "$path")")" >> "$var_conf" ;;
        *) ;;
    esac
done < <(find /var -mindepth 1 -xdev \( "${var_prune[@]}" \) -prune -o -printf '%y\t%m\t%u\t%g\t%p\0')

for ((i = ${#var_remove[@]} - 1; i >= 0; i--)); do
    path="${var_remove[i]}"
    if [ -d "$path" ] && [ ! -L "$path" ]; then
        rmdir --ignore-fail-on-non-empty "$path"
    else
        rm -f "$path"
    fi
done
