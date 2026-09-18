//! Requests stay bound to the session and request the user actually reviewed.
use crate::background::{self, PendingInput};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct Prompt {
    session: String,
    pending: Option<PendingInput>,
    question: usize,
    answers: serde_json::Map<String, Value>,
    submitted: bool,
}

impl Prompt {
    pub fn sync(&mut self, session: &str, pending: Option<PendingInput>) {
        if self.session != session || self.pending.as_ref().map(|p| &p.id) != pending.as_ref().map(|p| &p.id) {
            *self = Self { session: session.into(), pending, ..Self::default() };
        }
    }

    pub fn details(&self) -> Option<String> {
        let pending = self.pending.as_ref()?;
        if self.submitted { return Some("Response sent. Waiting for Codex…".into()); }
        if let Some(questions) = pending.params["questions"].as_array() {
            let question = questions.get(self.question)?;
            let mut text = format!("Question {} of {}: {}\n\n{}", self.question + 1, questions.len(),
                question["header"].as_str().unwrap_or_default(), question["question"].as_str().unwrap_or_default());
            if let Some(options) = question["options"].as_array() {
                for option in options { text.push_str(&format!("\n\n• {} — {}", option["label"].as_str().unwrap_or_default(),
                    option["description"].as_str().unwrap_or_default())); }
            }
            text.push_str("\n\nType your answer below and press Enter.");
            Some(text)
        } else { Some(pending.details.clone()) }
    }

    pub fn decision(&self, allow: bool) -> Option<(String, Value, Value)> {
        if self.submitted { return None; }
        let pending = self.pending.as_ref()?;
        let choices: &[&str] = if allow { &["accept"] } else { &["decline", "cancel"] };
        choices.iter().map(|decision| json!({"decision":decision}))
            .find(|result| background::valid_answer(pending, result))
            .map(|result| (self.session.clone(), pending.id.clone(), result))
    }

    pub fn sent(&mut self) { self.submitted = true; }

    // None means ordinary chat text. Err preserves the draft; Ok clears only this answer.
    pub fn answer(&mut self, text: &str) -> Option<Result<(), String>> {
        let pending = self.pending.as_ref()?;
        let questions = pending.params["questions"].as_array()?;
        if self.submitted { return Some(Err("Your answer is already being submitted.".into())); }
        if text.trim().is_empty() || text.encode_utf16().count() > 8192 {
            return Some(Err("Enter an answer of at most 8,192 characters.".into()));
        }
        let id = questions.get(self.question)?["id"].as_str()?;
        let mut answers = self.answers.clone();
        answers.insert(id.into(), json!({"answers":[text]}));
        if self.question + 1 == questions.len() {
            if let Err(error) = background::answer(&self.session, &pending.id, json!({"answers":answers})) {
                return Some(Err(error));
            }
            self.submitted = true;
        } else { self.answers = answers; self.question += 1; }
        Some(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_controls_remain_bound_to_the_reviewed_request() {
        let pending = PendingInput { id: json!(1), method: "item/commandExecution/requestApproval".into(),
            params: json!({"availableDecisions":["accept","cancel"]}), details: "Command: fixture".into() };
        let mut prompt = Prompt::default(); prompt.sync("first", Some(pending.clone()));
        assert_eq!(prompt.decision(false), Some(("first".into(), json!(1), json!({"decision":"cancel"}))));
        prompt.sent(); assert!(prompt.decision(true).is_none());
        prompt.sync("first", Some(pending.clone())); assert!(prompt.decision(true).is_none());
        prompt.sync("second", Some(pending)); assert_eq!(prompt.decision(true).unwrap().0, "second");
        prompt.sync("second", None); assert!(prompt.details().is_none());
    }

    #[test]
    fn question_progress_resets_for_a_different_session_and_keeps_failed_answers() {
        let pending = PendingInput { id: json!(1), method: "item/tool/requestUserInput".into(),
            params: json!({"questions":[{"id":"a","question":"First?"},{"id":"b","question":"Second?"}]}), details: String::new() };
        let mut prompt = Prompt::default(); prompt.sync("missing-task", Some(pending.clone()));
        assert!(prompt.answer(" ").unwrap().is_err());
        assert!(prompt.answer("First answer").unwrap().is_ok());
        assert!(prompt.details().unwrap().contains("Second?"));
        assert!(prompt.answer("Last answer").unwrap().is_err());
        assert!(prompt.details().unwrap().contains("Second?"), "a failed submission must remain retryable");
        prompt.sync("another-task", Some(pending));
        assert!(prompt.details().unwrap().contains("First?"));
        assert!(prompt.answers.is_empty());
        assert!(prompt.decision(true).is_none(), "questions cannot be approved as commands");
    }
}
