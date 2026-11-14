//! 회원가입 — 인증번호 발송·확인, 가입. 인증 헤더 없이, 오류 판정은 `BeClient::post_public`.

use serde_json::json;

use super::client::{BeClient, WebError};

impl BeClient {
    pub async fn send_signup_code(&self, email: &str) -> Result<(), WebError> {
        self.post_public(
            "/api/v1/auth/email/send-code",
            json!({"email": email, "purpose": "SIGNUP"}),
        )
        .await
    }

    pub async fn verify_signup_code(&self, email: &str, code: &str) -> Result<(), WebError> {
        self.post_public(
            "/api/v1/auth/email/verify-code",
            json!({"email": email, "code": code, "purpose": "SIGNUP"}),
        )
        .await
    }

    /// 비밀번호는 BE로만 보내고 저장하지 않는다.
    pub async fn register(&self, email: &str, password: &str, name: &str) -> Result<(), WebError> {
        self.post_public(
            "/api/v1/auth/register",
            json!({"email": email, "password": password, "name": name}),
        )
        .await
    }
}
