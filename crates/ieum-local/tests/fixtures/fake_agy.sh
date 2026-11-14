#!/bin/sh
# 가짜 agy: `models`면 옆의 models.txt를, 아니면 인자·환경·stdin을 작업 폴더에 남기고 옆의 stream.jsonl을 낸다.
if [ "$1" = models ]; then cat "$(dirname "$0")/models.txt"; exit $?; fi
printf '%s\n' "$@" > args.txt
printf '%s' "${AGY_CLI_DISABLE_AUTO_UPDATE-unset}" > env.txt
cat > stdin.txt
cat "$(dirname "$0")/stream.jsonl"
