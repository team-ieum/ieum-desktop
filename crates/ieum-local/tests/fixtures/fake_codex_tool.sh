#!/bin/sh
# 가짜 codex: 도구가 되살아난 것처럼 command_execution 이벤트를 내고 오래 잔다. 자기 pid를 cwd에 남긴다.
if [ "$1" = debug ]; then echo '{"models":[{"slug":"m"}]}'; exit 0; fi
echo $$ > cli.pid
cat > /dev/null
echo '{"type":"item.started","item":{"id":"i0","type":"command_execution","command":"ls"}}'
sleep 30
