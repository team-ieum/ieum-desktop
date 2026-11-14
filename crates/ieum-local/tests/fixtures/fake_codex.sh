#!/bin/sh
# 가짜 codex: `debug models --bundled`면 옆의 bundled.json을, 아니면 인자·stdin·CODEX_HOME·넘겨받은 카탈로그를 작업 폴더에 남기고 옆의 stream.jsonl을 낸다.
d=$(dirname "$0")
if [ "$1" = debug ]; then cat "$d/bundled.json"; exit $?; fi
printf '%s\n' "$@" > args.txt
printf '%s' "${CODEX_HOME-unset}" > home.txt
for a in "$@"; do case "$a" in model_catalog_json=*) p=${a#model_catalog_json=\"}; cp "${p%\"}" catalog.json;; esac; done
cat > stdin.txt
cat "$d/stream.jsonl"
