# 보안 요구사항 (항상 로드)

## 파일·경로
- 사용자가 고른 폴더는 `validate_user_folder`를 통과해야 한다(홈 아래 일반 폴더만 — `~/Library`·숨김 폴더·앱 데이터 폴더 거부)
- 파일 노드는 `AllowedRoots`(`roots_for_workflow`) 밖을 건드리지 않는다. 상대 경로는 `resolve_in_base`·`sanitize_name`로만 해석한다(절대 경로·`..`·심볼릭 링크 거부)
- 저장소 파일 이름이 되는 id는 UUID만 받는다(`store.rs` `check_id`)

## 비밀값
- API 키·세션은 키체인 vault에만 저장한다. `credentials.json`에는 이름표·끝 4자만 둔다
- 키는 `ApiKey`로만 들고 다닌다(`Debug`가 값을 가린다)
- 에러 문구·로그·실행 기록에 토큰·비밀번호·요청 헤더 값·요청 본문·서버가 보낸 메시지를 넣지 않는다. URL은 쿼리·fragment·계정 정보를 가린다(`http.rs`, `web/client.rs`)

## 네트워크
- 이음 BE 클라이언트(`src-tauri/src/web/`)는 읽기 전용이다 — GET 말고는 로그인·갱신·로그아웃·가입만. 리다이렉트는 끄고 응답 크기·시간 상한을 둔다
- HTTP 노드에 내부망 차단이 없는 것은 사용자가 정한 것이다 — 묻지 않고 추가하거나 바꾸지 않는다
- 빌더에서 HTTP 노드를 테스트하려면 사람이 확인해야 한다(`http_confirm`) — 이 확인을 건너뛰는 경로를 만들지 않는다

## 화면
- `tauri.conf.json`의 CSP를 느슨하게 하지 않는다
- HTML을 만드는 formatter는 `ammonia`로 정리한 결과만 낸다
