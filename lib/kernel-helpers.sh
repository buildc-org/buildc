#!/bin/bash

kver() {
    local pkg
    pkg="$(cat /usr/lib/buildc/kernel-package 2>/dev/null || echo 'kernel-core')"
    rpm -q --qf '%{VERSION}-%{RELEASE}.%{ARCH}' "$pkg"
}
