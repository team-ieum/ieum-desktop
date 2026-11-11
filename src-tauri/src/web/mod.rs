//! 이음 웹 계정 연결 — BE 기존 API만, 읽기 전용(로그인·갱신·로그아웃 외 GET만).

pub mod auth;
pub mod client;

pub use auth::{
    SESSION_ACCOUNT, Session, WebAccount, clear_session, load_session, save_settings,
    validate_server_url,
};
pub use client::{BeClient, WebError};
