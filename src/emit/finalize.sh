# ---- /opt relocation ----
mkdir -p /usr/lib/opt
tmpfiles=/usr/lib/tmpfiles.d/zz-opt-symlinks.conf
printf 'd /var/opt 0755 root root -\n' > "$tmpfiles"
for d in /opt/*/; do
    [ -d "$d" ] || continue
    name="$(basename "$d")"
    cp -a "$d" "/usr/lib/opt/${name}"
    esc="${name// /\\x20}"
    printf 'L+ /var/opt/%s - - - - /usr/lib/opt/%s\n' "$esc" "$esc" >> "$tmpfiles"
done
rm -rf /opt
mv /opt.bak /opt

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

var_escape() {
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

declare -A var_declared=()
touch "$var_conf"
while read -r verb path _; do
    case "$verb" in '' | \#*) continue ;; esac
    [ -n "$path" ] && var_declared[$path]=1
done < <(cat /usr/lib/tmpfiles.d/*.conf /etc/tmpfiles.d/*.conf 2> /dev/null)

var_prune=()
for keep in "${var_keep[@]}"; do
    var_prune+=(-path "$keep" -o)
done
unset 'var_prune[-1]'

var_remove=()
while IFS=$'\t' read -r -d '' type mode owner group path; do
    esc="$(var_escape "$path")"
    [ -n "${var_declared[$esc]:-}" ] && continue
    var_declared[$esc]=1
    var_remove+=("$path")
    case "$type" in
        d) printf 'd %s %s %s %s -\n' "$esc" "$mode" "$owner" "$group" >> "$var_conf" ;;
        l) printf 'L+ %s - - - - %s\n' "$esc" "$(var_escape "$(readlink "$path")")" >> "$var_conf" ;;
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
