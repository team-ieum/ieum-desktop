#!/bin/sh
# 가짜 CLI: stderr로 3MB를 쏟은 뒤 정상 답을 낸다.
head -c 3000000 /dev/zero | tr '\0' x >&2
echo '{"type":"result","subtype":"success","is_error":false,"result":"ok"}'
