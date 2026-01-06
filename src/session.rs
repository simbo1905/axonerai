use serde::{Deserialize, Serialize};
use crate::provider::Message;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Session {
    session_id: String,
    messages: Vec<Message>,
    time_stamp: String,
}

impl Session {
    pub fn new(session_id: String) -> Self {
        let timestamp = chrono::Utc::now().to_rfc3339();
        Self {
            session_id,
            messages: Vec::new(),
            time_stamp: timestamp,
        }
    }
    
    pub fn new_with_timestamp(session_id: String, timestamp: String) -> Self {
        Self {
            session_id,
            messages: Vec::new(),
            time_stamp: timestamp,
        }
    }
    
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    pub fn get_messages(&self) -> &Vec<Message> {
        &self.messages
    }
    
    pub fn get_id(&self) -> &str {
        &self.session_id
    }
    
    pub fn get_timestamp(&self) -> &str {
        &self.time_stamp
    }
    
    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }
}
