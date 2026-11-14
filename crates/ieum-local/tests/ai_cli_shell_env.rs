//! 로그인 셸 환경과 `CLAUDE_CONFIG_DIR`. 프로세스 환경(SHELL·ZDOTDIR)을 바꾸므로 따로 된 테스트 바이너리.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use ieum_local::ai::{AiProvider, AiRequest, CliKind, CliProvider};

fn ask(provider: &CliProvider, config_dir: Option<PathBuf>) -> String {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(provider.ask(
        AiRequest {
            prompt: "p".into(),
            model: None,
            effort: None,
            schema: None,
            key_name: None,
            config_dir,
            system: None,
        },
        &|_: &str| {},
    ))
    .unwrap()
    .text
}

#[test]
fn config_dir_survives_login_shell_profile() {
    let t = tempfile::tempdir().unwrap();
    let script = t.path().join("fake-cli");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_cli_env.sh"),
        &script,
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let cwd = t.path().join("cwd");
    fs::create_dir(&cwd).unwrap();
    let provider = CliProvider::new(CliKind::Claude)
        .program(script.to_str().unwrap())
        .cwd(cwd);
    let (empty, profiled) = (t.path().join("empty"), t.path().join("profiled"));
    fs::create_dir(&empty).unwrap();
    fs::create_dir(&profiled).unwrap();
    fs::write(
        profiled.join(".zprofile"),
        "export CLAUDE_CONFIG_DIR=/from/profile\n",
    )
    .unwrap();
    let chosen = PathBuf::from("/tmp/ieum test accts/a1b2c3d4");

    // SAFETY: 이 바이너리의 테스트는 이것 하나 — 다른 스레드가 환경을 읽지 않는다.
    unsafe {
        std::env::set_var("SHELL", "/bin/zsh");
        std::env::set_var("ZDOTDIR", &empty);
        std::env::set_var("CLAUDE_CONFIG_DIR", "/from/app");
    }
    // 이 Mac의 로그인: 앱이 물려받은 값은 넘기지 않는다.
    assert_eq!(ask(&provider, None), "unset");

    unsafe { std::env::set_var("ZDOTDIR", &profiled) };
    // 프로필이 값을 넣어도 고른 계정 폴더가 이긴다.
    assert_eq!(
        ask(&provider, Some(chosen.clone())),
        chosen.to_str().unwrap()
    );
    // 이 Mac의 로그인이면 사용자 프로필의 값 그대로.
    assert_eq!(ask(&provider, None), "/from/profile");
}
