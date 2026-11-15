#!/bin/sh
# 가짜 claude CLI: stream-json 마지막 result 이벤트에 usage를 싣는다.
cat > /dev/null
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"ok"}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{"input_tokens":12,"output_tokens":3,"cache_creation_input_tokens":5,"cache_read_input_tokens":100}}'
