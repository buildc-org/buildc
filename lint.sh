#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

# components/ is a fetched collection cache. It is linted where it is authored.
mapfile -t scripts < <(find . \( -path ./target -o -path ./fuzz/target -o -path ./components \) -prune -o \
    -name '*.sh' -type f -print)
shfmt=(shfmt -i 4 -ci -bn -sr)

if [ "${1:-}" = "--fix" ]; then
    "${shfmt[@]}" -w "${scripts[@]}"
    cargo fmt
    echo "lint: formatted ${#scripts[@]} scripts and the source"
    exit
fi

for tool in shellcheck shfmt; do
    command -v "$tool" > /dev/null 2>&1 || {
        echo "lint: $tool not found, install it first" >&2
        exit 1
    }
done

unformatted() {
    echo "lint: unformatted, run ./lint.sh --fix" >&2
    exit 1
}

if grep -rnE 'kdl::|Kdl[A-Z]' src --exclude-dir=parse --exclude-dir=provenance; then
    echo "lint: a KDL type outside the two places that read a document, src/parse/ and the \
readers in src/provenance/" >&2
    exit 1
fi

if grep -nE 'kdl::|Kdl[A-Z]' src/provenance/mod.rs; then
    echo "lint: a KDL type in src/provenance/mod.rs, which holds the model half" >&2
    exit 1
fi

mapfile -t member_src < <(find . \( -path ./target -o -path ./fuzz/target -o -path ./components \) -prune \
    -o -path '*/src/*' -name '*.rs' -type f -print)

if grep -n 'ratatui' "${member_src[@]}"; then
    echo "lint: ratatui outside the common crate, which is the only place it may appear" >&2
    exit 1
fi

# The alternatives catch a path or a renaming import, a braced import beside another item, and a call site whose import sits in another file.
if grep -rnE 'common::ui|common::\{[^}]*\bui\b|\bui::' src/resolve src/emit; then
    echo "lint: the resolver or an emitter reading the ui module, and that dependency runs one way" >&2
    exit 1
fi

# A re-export under another name walks around the rule above. Re-exporting one item must still pass.
if grep -rnE '^[[:space:]]*pub(\(crate\))? use common::ui( as [A-Za-z_][A-Za-z0-9_]*)?[[:space:]]*;' src; then
    echo "lint: the ui module re-exported under another name, which walks around the rule above" >&2
    exit 1
fi

forbidden() {
    local file=$1
    local pattern=$2
    local message=$3
    if [ ! -f "$file" ]; then
        echo "lint: $file is gone or is now a directory, so the command-edge check cannot run" >&2
        exit 1
    fi
    if grep -nE "$pattern" "$file" | grep -vE '^[0-9]+:[[:space:]]*(//|/\*|\*)'; then
        echo "lint: $message" >&2
        exit 1
    fi
}

forbidden src/provider.rs \
    'crate::add_component::|use crate::add_component( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "provider reads add_component and the provider index sits below the add component flow"

forbidden src/add_component.rs \
    'crate::(set|create)::|use crate::(set|create)( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "add_component reads a setup command and the shared setup lives in setup"

forbidden src/create.rs \
    'crate::set::|use crate::set( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "create reads set, and the shared setup lives in setup"

forbidden src/set.rs \
    'crate::create::|use crate::create( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "set reads create, and the shared setup lives in setup"

forbidden src/copy.rs \
    'crate::(create|add_component|set|dispatch|setup|conformance|report)::|use crate::(create|add_component|set|dispatch|setup|conformance|report)( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "copy holds prompt text and reads the host from forge, so every flow sits above it"

forbidden src/add_component.rs \
    'crate::dispatch::|use crate::dispatch( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "a command reads dispatch, and the command error is error.rs"

forbidden src/set.rs \
    'crate::dispatch::|use crate::dispatch( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "a command reads dispatch, and the command error is error.rs"

forbidden src/parse/declaration.rs \
    'crate::(create|add_component|set|dispatch|setup|conformance|report|forge|provider)::|use crate::(create|add_component|set|dispatch|setup|conformance|report|forge|provider)( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "declaration edits the KDL files and sits below every command and flow"

forbidden src/setup.rs \
    'crate::(create|add_component|set|dispatch|provider)::|use crate::(create|add_component|set|dispatch|provider)( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "setup holds the shared questions and sits below every command"

forbidden src/conformance.rs \
    'crate::(create|add_component|set|dispatch|provider)::|use crate::(create|add_component|set|dispatch|provider)( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "conformance holds the profile declaration and sits below every command"

forbidden src/report.rs \
    'crate::(create|add_component|set|dispatch|provider|setup|conformance)::|use crate::(create|add_component|set|dispatch|provider|setup|conformance)( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "report renders the written tree and sits below every flow"

forbidden src/forge.rs \
    'crate::(copy|create|add_component|set|dispatch|setup|conformance|report)::|use crate::(copy|create|add_component|set|dispatch|setup|conformance|report)( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "forge is the host leaf, and every reader sits above it"

forbidden src/bases.rs \
    'crate::(create|add_component|set|dispatch|provider)::|use crate::(create|add_component|set|dispatch|provider)( as [A-Za-z_][A-Za-z0-9_]*)?;' \
    "bases holds the base-component query and sits below every command"

mapfile -t parse_files < <(find src/parse -name '*.rs' -type f)
for file in "${parse_files[@]}"; do
    forbidden "$file" \
        'crate::bases::|crate::\{[^}]*\bbases\b|\bbases::|use crate::bases( as [A-Za-z_][A-Za-z0-9_]*)?;' \
        "parse reads the base catalog, and the catalog sits above the parser"
done

mapfile -t posix < <(grep -lx '#!/bin/sh' "${scripts[@]}")
mapfile -t in_bash < <(grep -Lx '#!/bin/sh' "${scripts[@]}" | grep -v '/repo\.sh$')
mapfile -t repos < <(printf '%s\n' "${scripts[@]}" | grep '/repo\.sh$')
shellcheck -s bash "${in_bash[@]}"
# A fixture's repo.sh sets REPO_ID for the generated script. The linter cannot see that assignment.
[ "${#repos[@]}" -eq 0 ] || shellcheck -s bash -e SC2034 "${repos[@]}"
[ "${#posix[@]}" -eq 0 ] || shellcheck -s sh "${posix[@]}"
"${shfmt[@]}" -d "${scripts[@]}" || unformatted
cargo fmt --check || unformatted
echo "lint: ${#scripts[@]} scripts and the source are clean"

cargo deny --locked check bans licenses sources
cargo deny --manifest-path fuzz/Cargo.toml --locked check bans licenses sources
echo "lint: the dependency policy accepts the locked graph"

cargo check --locked --features fuzzing
echo "lint: the fuzz entry points compile"

mapfile -t fuzz_targets < <(grep -A1 '^\[\[bin\]\]' fuzz/Cargo.toml | sed -n 's/^name = "\(.*\)"$/\1/p')
for target in "${fuzz_targets[@]}"; do
    if [ ! -d "fuzz/corpus/$target" ]; then
        echo "lint: the fuzz target $target has no fuzz/corpus/$target" >&2
        exit 1
    fi
done
echo "lint: every fuzz target has a corpus"

cargo clippy --locked --all-targets -- -D warnings
echo "lint: clippy accepts every target"

cargo nextest run --locked
echo "lint: the goldens match"
