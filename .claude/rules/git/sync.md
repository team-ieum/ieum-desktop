# 브랜치·pull·push
- 브랜치: `main`(배포) ← `dev`(통합) ← 작업 브랜치 `feat/<이슈번호>-<짧은-설명>`·`fix/<이슈번호>-<짧은-설명>` (예: `feat/7-schedule-triggers`). 작업 브랜치는 `dev`에서 딴다
- pull은 항상 rebase: `git pull --rebase`. 작업 브랜치를 최신 `dev`에 맞출 때도 merge 대신 `git rebase origin/dev`
- `main`·`dev`에는 직접 push하지 않는다 — PR로만 들어간다
- `main`·`dev`는 force push 금지. 작업 브랜치를 rebase한 뒤에는 `git push --force-with-lease`만 쓴다(`--force` 금지)
