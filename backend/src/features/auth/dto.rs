use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid;

// メールアドレスだけで仮登録を開始し、確認メールのtokenで本登録へ進む。
#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub email: String,
}

#[derive(Debug, Clone, Serialize, ToSchema, PartialEq, Eq)]
pub struct RegisterResponse {
    pub message: String,
    pub email: String,
}

// handler層で統一して返すエラー形式。内部エラーの詳細はmessageへ漏らさない。
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

// 仮登録tokenとユーザー名/passwordを受け取り、ユーザー作成を完了する。
#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteRegistrationRequest {
    pub token: String,
    pub user_name: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CompleteRegistrationResponse {
    pub message: String,
}

// session cookie発行のためのログイン入力。emailはservice側で正規化する。
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    pub message: String,
    pub email: String,
    pub user_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LogoutResponse {
    pub message: String,
}

// フロントが現在のログイン状態とロールを判断するための最小ユーザー情報。
#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentUserResponse {
    pub user_id: uuid::Uuid,
    pub email: String,
    pub user_name: String,
    pub role: String,
}
