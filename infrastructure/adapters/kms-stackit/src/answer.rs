//! What STACKIT said when it refused a request.

/// At most this many characters of an answer that isn't STACKIT's are passed on, such as a load balancer's page.
const MAX_FOREIGN_ANSWER: usize = 300;

/// Why STACKIT refused a request, from the answer's `body`: STACKIT's own message when the body carries one, as the
/// KMS API (`message`) and the sign-in (`error_description`, `error`) do. Any other answer is cut to its start.
pub(crate) fn refusal_message(body: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|answer| {
        ["message", "error_description", "error"]
            .into_iter()
            .find_map(|field| answer.get(field)?.as_str().map(str::to_string))
    });
    message.unwrap_or_else(|| body.chars().take(MAX_FOREIGN_ANSWER).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_why_stackit_refused_a_request() {
        let message = refusal_message(r#"{"message": "The service account has no role on the project"}"#);

        assert_eq!(message, "The service account has no role on the project");
    }

    #[test]
    fn reports_why_stackit_refused_a_sign_in() {
        let message = refusal_message(r#"{"error": "invalid_client", "error_description": "Unknown issuer"}"#);

        assert_eq!(message, "Unknown issuer");
    }

    #[test]
    fn passes_on_only_the_start_of_an_answer_that_is_not_stackits() {
        let page = format!("<html>{}</html>", "x".repeat(400));

        let message = refusal_message(&page);

        assert_eq!(message, format!("<html>{}", "x".repeat(294)));
    }
}
