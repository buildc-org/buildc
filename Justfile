[private]
default:
    @just --list

lint:
    ./lint.sh

fix:
    ./lint.sh --fix
