#!/bin/sh
# 가짜 CLI: 자식 하나를 띄우고 오래 잔다. 자기·자식 pid를 cwd에 남긴다.
echo $$ > cli.pid
sleep 60 &
echo $! > child.pid
wait
