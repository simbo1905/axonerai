use serde::{Deserialize, Serialize};
use crate::provider::Message;
use anyhow::Result;

#[derive(Serialize,Deserialize, Debug, Clone)]
pub struct Session{
    pub session_id: String,
    pub messages: Vec<Message>,
    pub time_stamp: String
}

pub trait SessionManager: Send + Sync {
    fn load(&self) -> Result<Session>;
    fn save(&self, session: &Session) -> Result<()>;
    fn exists(&self) -> bool;
    fn get_session_id(&self) -> &str;
}

impl Session {

    pub fn new(session_id: String) -> Self {
        Self{
            session_id,
            messages: Vec::new(),
            time_stamp:  "2025-11-19T00:00:00Z".to_string(),
        }
    }
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    pub fn get_messages(&self) -> &Vec<Message> {
        &self.messages
    }


}
