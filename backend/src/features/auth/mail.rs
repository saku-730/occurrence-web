use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::Mailbox,
    transport::smtp::{authentication::Credentials, client::Tls},
};

use crate::config::SmtpConfig;

// SMTP送信の詳細をhandlerから分離する。テストではMailpit、本番では外部SMTPを同じ経路で扱う。
pub async fn send_mail(message: &MailMessage, smtp: &SmtpConfig) -> Result<(), MailError> {
    let from: Mailbox = smtp //送信元メールアドレス設定
        .from
        .parse()
        .map_err(|_| MailError::InvalidFromAddress)?; //エラー変換

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

    let tls_mode = smtp.tls.trim().to_lowercase(); //tls の設定値を整形

    let mut builder = match tls_mode.as_str() {
        //tlsの値によって分岐
        // MailpitなどのローカルSMTPではTLSなしを許可する。
        "none" => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host) //開発 Mailpit
                .port(smtp.port)
                .tls(Tls::None)
        }

        // 外部SMTPで平文接続からTLSへ昇格する方式。
        "starttls" => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host) //本番Resend用
                .map_err(|_| MailError::BuildTransport)?
                .port(smtp.port)
        }

        // 最初からTLSで接続するSMTP用。
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host) //
            .map_err(|_| MailError::BuildTransport)?
            .port(smtp.port),

        _ => return Err(MailError::UnsupportedTlsMode), //TLS設定外
    };

    if !smtp.username.trim().is_empty() {
        let credentials = Credentials::new(
            //認証用情報組み立て
            smtp.username.clone(),
            smtp.password.clone(),
        );

        builder = builder.credentials(credentials);
    }

    let mailer = builder.build();

    mailer
        .send(email)
        .await
        .map_err(|_| MailError::SendFailed)?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailMessage {
    pub to: String,      //宛先
    pub subject: String, //件名
    pub body: String,    //本文
}

#[derive(Debug)]
pub enum MailError {
    InvalidFromAddress,
    InvalidToAddress,
    BuildMessage,
    BuildTransport,
    UnsupportedTlsMode,
    SendFailed,
}

// 登録完了URLはbackendのbase URLから作る。フロント/環境差分をConfigに閉じ込めるため。
pub fn build_registration_completion_email(
    //メールの宛先・件名・本文を作成
    to: &str,           //宛先
    app_base_url: &str, //ドメイン
    token: &str,        //トークン クエリパラメータ用
) -> MailMessage {
    let completion_url = format!(
        //メール登録用のURL作成
        "{}/auth/complete_registration?token={}", //登録完了用パス
        app_base_url.trim_end_matches('/'),       //ドメイン
        token                                     //トークンクエリパラメータ
    );

    MailMessage {
        to: to.to_string(),                                //宛先
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
    use super::{MailMessage, send_mail};
    use crate::config::SmtpConfig;

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
