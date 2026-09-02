use crate::provider::Message;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Session {
    session_id: String,
    messages: Vec<Message>,
    time_stamp: String,
}

impl Session {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            messages: Vec::new(),
            time_stamp: "2025-11-19T00:00:00Z".to_string(),
        }
    }
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    pub fn get_messages(&self) -> &Vec<Message> {
        &self.messages
    }
}

/// Tokens estimate for the model's context: bytes/4 (floor) over the JSON
/// serialization of the provider messages, plus the system prompt bytes when
/// one is loaded. Protocol events (ready/session_meta/echo) are never part
/// of the model's context and must not be counted.
pub fn estimate_tokens(messages: &[Message], system_prompt: Option<&str>) -> u64 {
    let mut bytes = serde_json::to_vec(messages).map_or(0, |v| v.len());
    if let Some(prompt) = system_prompt {
        bytes += prompt.len();
    }
    bytes as u64 / 4
}

/// Context tokens for a session read from its persisted agent-state file
/// (`<agent_state_dir>/<session_id>/messages.json`, the file
/// [`crate::file_session_manager::FileSessionManager`] maintains). 0 when the
/// file is missing or unreadable — a freshly-connected session has no model
/// context yet, no matter how many protocol frames the rollout carries.
pub fn context_tokens_on_disk(
    agent_state_dir: &Path,
    session_id: &str,
    system_prompt: Option<&str>,
) -> u64 {
    let path = agent_state_dir.join(session_id).join("messages.json");
    let Ok(content) = std::fs::read_to_string(path) else {
        return 0;
    };
    match serde_json::from_str::<Session>(&content) {
        Ok(session) => estimate_tokens(session.get_messages(), system_prompt),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ToolCall;
    use serde_json::json;

    #[test]
    fn session_with_native_tool_messages_round_trips_through_json() {
        let mut session = Session::new("s1".to_string());
        session.add_message(Message {
            role: "user".to_string(),
            content: "what is 2^4".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        session.add_message(Message {
            role: "assistant".to_string(),
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "call_7".to_string(),
                name: "calculator".to_string(),
                input: json!({"operation": "multiply", "a": 2.0, "b": 2.0}),
            }]),
            tool_call_id: None,
        });
        session.add_message(Message {
            role: "tool".to_string(),
            content: "4.0".to_string(),
            tool_calls: None,
            tool_call_id: Some("call_7".to_string()),
        });

        let bytes = serde_json::to_string(&session).unwrap();
        let back: Session = serde_json::from_str(&bytes).unwrap();
        let messages = back.get_messages();

        assert_eq!(messages.len(), 3);
        let calls = messages[1].tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].id, "call_7");
        assert_eq!(calls[0].name, "calculator");
        assert_eq!(
            calls[0].input,
            json!({"operation": "multiply", "a": 2.0, "b": 2.0})
        );
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_7"));
        assert_eq!(messages[2].content, "4.0");
    }

    #[test]
    fn old_session_file_without_tool_fields_still_loads() {
        let old = r#"{
            "session_id": "old-session",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "Using tool 'calculator' with input: {\"a\":2}"}
            ],
            "time_stamp": "2025-11-19T00:00:00Z"
        }"#;

        let session: Session = serde_json::from_str(old).unwrap();
        let messages = session.get_messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert!(messages[0].tool_calls.is_none());
        assert!(messages[0].tool_call_id.is_none());
        assert!(messages[1].tool_calls.is_none());
    }
}
