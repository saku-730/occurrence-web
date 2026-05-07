#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailMessage {
    pub to: String, //宛先
    pub subject: String, //件名
    pub body: String, //本文
}

pub fn build_registration_completion_email( //メールの宛先・件名・本文を作成
    to: &str, //宛先
    app_base_url: &str, //ドメイン
    token: &str, //トークン クエリパラメータ用
) -> MailMessage {
    let completion_url = format!( //メール登録用のURL作成
        "{}/auth/complete_registration?token={}", //登録完了用パス
        app_base_url.trim_end_matches('/'),//ドメイン
        token //トークンクエリパラメータ
    );

    MailMessage {
        to: to.to_string(), //宛先
        subject: "Complete your registration".to_string(), //件名
        body: format!(
            "Please complete your registration by opening the following URL:\n\n{}",
            completion_url
        ), //本文
    }
}

#[cfg(test)]
mod tests {
    use super::build_registration_completion_email;

    #[test]
    fn builds_registration_completion_email_with_completion_url() {
        let email = format!("mail-valid-{}@example.com", uuid::Uuid::new_v4());
        let app_base_url = "http://127.0.0.1:3000";
        let token = "test-token";

        let message = build_registration_completion_email(&email, app_base_url, token);

        assert_eq!(message.to, email);
        assert!(message.subject.contains("registration"));
        assert!(
            message
                .body
                .contains("http://127.0.0.1:3000/auth/complete_registration?token=test-token")
        );
    }
}