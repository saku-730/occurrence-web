use lettre::{
    message::Mailbox,
    transport::smtp::client::Tls,
    AsyncSmtpTransport,
    AsyncTransport,
    Message,
    Tokio1Executor,
};

use crate::config::SmtpConfig;

pub async fn send_mail(
    message: &MailMessage,
    smtp: &SmtpConfig
) -> Result<(), MailError> {
    let from: Mailbox = "no-reply@example.com"//送信元メールアドレス設定
        .parse()
        .map_err(|_| MailError::InvalidFromAddress)?;//エラー変換

    let to: Mailbox = message //宛先メールアドレス設定
        .to
        .parse()
        .map_err(|_| MailError::InvalidToAddress)?;

    let email = Message::builder() //メール作成
        .from(from)
        .to(to)
        .subject(&message.subject)
        .body(message.body.clone())
        .map_err(|_| MailError::BuildMessage)?;

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host) //SMTP送信設定
        .port(smtp.port)
        .tls(Tls::None)
        .build();

    mailer //メール送信
        .send(email)
        .await
        .map_err(|_| MailError::SendFailed)?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailMessage {
    pub to: String, //宛先
    pub subject: String, //件名
    pub body: String, //本文
}

#[derive(Debug)]
pub enum MailError {
    InvalidFromAddress,
    InvalidToAddress,
    BuildMessage,
    SendFailed,
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
    use crate::config::SmtpConfig;
    use super::{send_mail, MailMessage};
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

    #[tokio::test]
    async fn send_mail_sends_message_using_smtp_config() {
        let message = MailMessage {
            to: format!("mail-send-test-{}@example.com", uuid::Uuid::new_v4()),
            subject: "SMTP config test".to_string(),
            body: "This mail was sent using SmtpConfig.".to_string(),
        };

        let smtp = SmtpConfig {
            host: "127.0.0.1".to_string(),
            port: 1025,
            username: "".to_string(),
            password: "".to_string(),
            tls: "none".to_string(),
            from: "no-reply@example.com".to_string(),
        };

        send_mail(&message, &smtp)
            .await
            .expect("send_mail should send email using SmtpConfig");
    }
}