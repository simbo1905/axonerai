//! item37d — the Context token count must reflect the model's context
//! (bytes/4 over the serialized provider messages, system prompt included
//! when loaded), never WS protocol frames (ready/session_meta/echo).

use std::path::PathBuf;

use axonerai::provider::Message;
use axonerai::rollout::{self, Rollout};
use axonerai::session::{context_tokens_on_disk, estimate_tokens};
use axonerai::wire::ServerMsg;
use axonerai::{FileSessionManager, Session};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("item37d-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn known_messages() -> Vec<Message> {
    vec![
        Message {
            role: "user".to_string(),
            content: "what is 2^4".to_string(),
            tool_calls: None,
            tool_call_id: None,
        },
        Message {
            role: "assistant".to_string(),
            content: "16".to_string(),
            tool_calls: None,
            tool_call_id: None,
        },
    ]
}

/// A session with known messages (user + assistant text) must produce exactly
/// bytes/4 over the serialized message JSON (plus system prompt bytes).
#[test]
fn known_messages_tokens_are_exact_bytes_over_four() {
    let messages = known_messages();
    let system = Some("You are a helpful assistant.");

    let mut expected = serde_json::to_vec(&messages).unwrap().len();
    expected += system.unwrap_or_default().len();
    let expected = (expected / 4) as u64;

    assert!(expected > 0, "known messages must estimate non-zero");
    assert_eq!(estimate_tokens(&messages, system), expected);
}

/// No system prompt → formula over messages alone; empty messages → 0.
#[test]
fn empty_messages_estimate_zero() {
    let messages: Vec<Message> = vec![];
    assert_eq!(estimate_tokens(&messages, None), 0);

    // Empty vec serializes to "[]" (2 bytes); formula = (2 + prompt)/4.
    let prompt = "sys";
    let expected = (serde_json::to_vec(&messages).unwrap().len() + prompt.len()) / 4;
    assert_eq!(estimate_tokens(&messages, Some(prompt)), expected as u64);
    assert_eq!(estimate_tokens(&messages, Some(prompt)), 1);
}

/// A freshly-connected session has NO messages → 0, NOT bytes/4 of the
/// ready/session_meta frames the connect appended to the rollout.
#[test]
fn fresh_session_is_zero_even_when_rollout_has_connect_frames() {
    let sessions_dir = temp_dir("fresh");
    let agent_state_dir = sessions_dir.join("agent-state");
    let id = "01890a5d-ac96-774b-bcce-b302099a8057";

    let rollout = Rollout::create(&sessions_dir, id).unwrap();
    // Simulate a WS connect: ready + session_meta frames, exactly as
    // ws_session appends them.
    rollout
        .append_event(
            &serde_json::to_value(&ServerMsg::Ready {
                version: "0.0.0",
                websocket_path: "/ws",
            })
            .unwrap(),
        )
        .unwrap();
    rollout
        .append_event(
            &serde_json::to_value(&ServerMsg::SessionMeta {
                session_id: id,
                title: "t",
                created_at: 1,
            })
            .unwrap(),
        )
        .unwrap();

    // Sanity: the protocol frames do occupy rollout bytes (the old
    // rollout-bytes/4 formula would report a non-zero, inflated count).
    let rollout_bytes = std::fs::metadata(rollout.path()).unwrap().len();
    assert!(rollout_bytes / 4 > 0, "connect frames must occupy bytes");

    assert_eq!(context_tokens_on_disk(&agent_state_dir, id, Some("sys")), 0);
}

/// Appending ready/session_meta protocol events to the rollout must NOT
/// change the context count of a session that has real messages.
#[test]
fn reconnect_protocol_frames_do_not_change_context_count() {
    let sessions_dir = temp_dir("reconnect");
    let agent_state_dir = sessions_dir.join("agent-state");
    let id = "01890a5d-ac96-774b-bcce-b302099a8057";

    let rollout = Rollout::create(&sessions_dir, id).unwrap();

    // Persist known messages via the same per-session agent-state file the
    // agent writes (sessions/agent-state/<uuid>/messages.json).
    let manager = FileSessionManager::new(id.to_string(), agent_state_dir.clone()).unwrap();
    let mut session = Session::new(id.to_string());
    for message in known_messages() {
        session.add_message(message);
    }
    manager.save(&session).unwrap();

    let before = context_tokens_on_disk(&agent_state_dir, id, Some("sys"));
    assert!(before > 0);

    // Two more WS connects: each appends ready + session_meta.
    for _ in 0..2 {
        rollout
            .append_event(
                &serde_json::to_value(&ServerMsg::Ready {
                    version: "0.0.0",
                    websocket_path: "/ws",
                })
                .unwrap(),
            )
            .unwrap();
        rollout
            .append_event(
                &serde_json::to_value(&ServerMsg::SessionMeta {
                    session_id: id,
                    title: "t",
                    created_at: 2,
                })
                .unwrap(),
            )
            .unwrap();
    }

    let after = context_tokens_on_disk(&agent_state_dir, id, Some("sys"));
    assert_eq!(after, before, "protocol frames must not inflate context");

    // The count must come from the agent-state file, not the rollout size.
    assert_ne!(
        after,
        std::fs::metadata(rollout.path()).unwrap().len() / 4,
        "rollout-bytes/4 is the fraud being fixed"
    );
}

/// A corrupt agent-state file must degrade to 0, not panic or guess.
#[test]
fn corrupt_agent_state_file_counts_zero() {
    let sessions_dir = temp_dir("corrupt");
    let agent_state_dir = sessions_dir.join("agent-state");
    let id = "01890a5d-ac96-774b-bcce-b302099a8057";
    std::fs::create_dir_all(agent_state_dir.join(id)).unwrap();
    std::fs::write(agent_state_dir.join(id).join("messages.json"), "{not json").unwrap();
    assert_eq!(context_tokens_on_disk(&agent_state_dir, id, None), 0);
}

/// The agent-state path the server queries must live under the sessions dir
/// (rollout::default_dir() is the same root the rollouts live in).
#[test]
fn agent_state_dir_is_scoped_per_session() {
    let sessions_dir = temp_dir("scope");
    let agent_state_dir = sessions_dir.join("agent-state");
    let other = "01890a5d-ac96-774b-bcce-b302099a8057";

    let manager = FileSessionManager::new(other.to_string(), agent_state_dir.clone()).unwrap();
    let mut session = Session::new(other.to_string());
    session.add_message(Message {
        role: "user".to_string(),
        content: "hello".to_string(),
        tool_calls: None,
        tool_call_id: None,
    });
    manager.save(&session).unwrap();

    // A different session id must not see this file.
    let missing = "01890a5d-ac96-774b-bcce-b302099a8058";
    assert_eq!(context_tokens_on_disk(&agent_state_dir, missing, None), 0);
    assert!(rollout::is_session_id(other));
}
