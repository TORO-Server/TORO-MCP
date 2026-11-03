//! 呼び出し元の確認

use serde_json::Value;

use super::success;
use crate::identity::Session;

pub fn get(session: Option<&Session>) -> Value {
    let Some(session) = session else {
        return success(
            "未認証状態です。Discord による OAuth 認可を行うと、ユーザー権限や Wiki 編集権限が有効化されます。",
        );
    };
    let permission = if session.can_edit() {
        "[YES] (TORO SERVER MEMBER)"
    } else {
        "[NO] (GUEST/NON-MEMBER)"
    };
    success(&format!(
        "[AUTH USER IDENTIFICATION]\n- USER: {}\n- DISCORD ID: {}\n- WIKI EDIT PERMISSION: {permission}\n- ROLES COUNT: {}",
        session.username(),
        session.id(),
        session.role_ids().len(),
    ))
}
