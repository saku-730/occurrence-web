#[cfg(test)]
mod tests {
    use super::build_registration_completion_email;

    #[test]
    fn builds_registration_completion_email_with_completion_url() {
        let email = format!("mail-valid-{}@example.com", uuid::Uuid::new_v4());
        let app_base_url = "http://127.0.0.1:3000";
        let token = "test-token";

        let message = build_registration_completion_email(email, app_base_url, token);

        assert_eq!(message.to, email);
        assert!(message.subject.contains("registration"));
        assert!(
            message
                .body
                .contains("http://127.0.0.1:3000/auth/complete_registration?token=test-token")
        );
    }
}