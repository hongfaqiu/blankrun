#!/bin/sh
# 仅用于在 Linux 上 cargo check macOS 目标：不真正编译 C/ObjC，只产生空目标文件。不要用于构建。
out=""
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; -o*) out="${1#-o}";; -Fo*) out="${1#-Fo}";; esac; shift; done
[ -n "$out" ] && : > "$out"
exit 0
