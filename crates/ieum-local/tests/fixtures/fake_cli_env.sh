#!/bin/sh
# 가짜 claude CLI: 받은 CLAUDE_CONFIG_DIR(없으면 "unset")를 답으로 돌려준다.
cat > /dev/null
d="${CLAUDE_CONFIG_DIR-unset}"
echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"$d\"}"
