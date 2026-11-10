#!/bin/sh
# 가짜 CLI: 받은 인자(한 줄에 하나)와 stdin을 작업 폴더에 남기고 정상 답을 낸다.
printf '%s\n' "$@" > args.txt
cat > stdin.txt
echo '{"type":"result","subtype":"success","is_error":false,"result":"ok"}'
