#!/bin/sh
# 가짜 agy: 인자·환경·stdin을 작업 폴더에 남기고 옆의 stream.jsonl을 낸다.
printf '%s\n' "$@" > args.txt
printf '%s' "${AGY_CLI_DISABLE_AUTO_UPDATE-unset}" > env.txt
cat > stdin.txt
cat "$(dirname "$0")/stream.jsonl"
