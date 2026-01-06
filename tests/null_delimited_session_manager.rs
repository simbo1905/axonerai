use axonerai::null_delimited_file_session_manager::NullDelimitedFileSessionManager;
use axonerai::session_manager::SessionManager;
use uuid::Uuid;

#[test]
fn null_delimited_round_trip_messages() {
    let base = std::env::temp_dir().join(format!("axonerai-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&base).unwrap();

    let sm = NullDelimitedFileSessionManager::new("ses-test".to_string(), base.clone()).unwrap();

    let mut s = sm.load().unwrap();
    assert_eq!(s.get_messages().len(), 0);

    s.add_message(axonerai::provider::Message {
        role: "user".to_string(),
        content: "hello".to_string(),
    });
    s.add_message(axonerai::provider::Message {
        role: "assistant".to_string(),
        content: "world\nline2".to_string(),
    });
    sm.save(&s).unwrap();

    let s2 = sm.load().unwrap();
    assert_eq!(s2.get_messages().len(), 2);
    assert_eq!(s2.get_messages()[0].role, "user");
    assert_eq!(s2.get_messages()[0].content, "hello");
    assert_eq!(s2.get_messages()[1].role, "assistant");
    assert_eq!(s2.get_messages()[1].content, "world\nline2");

    // cleanup best-effort
    let _ = std::fs::remove_dir_all(base);
}

