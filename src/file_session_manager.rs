use anyhow::{Result};
use std::path::PathBuf;
use std::fs;
use crate::session::{Session, SessionManager};

pub struct FileSessionManager{
    session_id: String,
    directory: PathBuf
}

impl FileSessionManager {
    pub fn new(session_id: String, base_dir: PathBuf) -> Result<Self, std::io::Error>{
        fs::create_dir_all(&base_dir.join(&session_id))?;
        Ok(Self{
            session_id,
            directory: base_dir
        })
    }
    fn session_path(&self)->PathBuf{
        self.directory.join(&self.session_id)
    }
}

impl SessionManager for FileSessionManager {
    fn save(&self, session: &Session)-> Result<()>{
        let location = self.session_path().join("messages.json");
        let message = serde_json::to_string(session)?;
        fs::write(&location, message)?;
        Ok(())
    }

    fn load(&self)->Result<Session>{
        let location = self.session_path().join("messages.json");
        let content = fs::read_to_string(location)?;
        let session = serde_json::from_str(&content)?;
        Ok(session)
    }

    fn exists(&self)->bool{
        let path = &self.session_path().join("messages.json");
        path.exists()
    }

    fn get_session_id(&self)->&str{
        &self.session_id
    }
}
