use anyhow::Result;
use std::path::PathBuf;
use std::fs;
use crate::session::Session;
use crate::session_manager::SessionManager;

pub struct FileSessionManager{
    session_id: String,
    directory: PathBuf
}

impl FileSessionManager {
    pub fn new(session_id: String, base_dir: PathBuf) -> Result<Self>{
        fs::create_dir_all(&base_dir.join(&session_id))?;
        Ok(Self{
            session_id,
            directory: base_dir
        })
    }
    fn session_path(&self)->PathBuf{
        self.directory.join(&self.session_id)
    }

    pub fn save(&self, session: &Session)-> Result<()>{
        let location = self.session_path().join("messages.json");
        let message = serde_json::to_string(session)?;
        fs::write(&location, message)?;
        Ok(())
    }

    pub fn load(&self)->Result<Session>{
        let location = self.session_path().join("messages.json");
        let content = fs::read_to_string(location)?;
        let session = serde_json::from_str(&content)?;
        Ok(session)
    }
    pub fn exists(&self)->bool{

        let path = &self.session_path().join("messages.json");
        return if path.exists() {
            true
        } else { false }

    }

    pub fn get_session(&self)->&str{
        &self.session_id
    }

}

impl SessionManager for FileSessionManager {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn exists(&self) -> bool {
        FileSessionManager::exists(self)
    }

    fn load(&self) -> Result<Session> {
        Ok(FileSessionManager::load(self)?)
    }

    fn save(&self, session: &Session) -> Result<()> {
        Ok(FileSessionManager::save(self, session)?)
    }

    fn raw_session_path(&self) -> Option<PathBuf> {
        Some(self.session_path().join("messages.json"))
    }
}