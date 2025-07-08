use crate::memory_exex::Memory;
use dashmap::DashMap;
use eyre::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct EphemeralSession {
    pub id: String,
    pub agent_id: String,
    pub created_at: std::time::Instant,
    pub expires_at: std::time::Instant,
    pub memories: Vec<Memory>,
    pub metadata: SessionMetadata,
}

#[derive(Debug, Clone)]
pub struct SessionMetadata {
    pub context: String,
    pub goal: Option<String>,
    pub collaborators: Vec<String>,
    pub memory_limit: usize,
}

#[derive(Debug)]
pub struct EphemeralZoneManager {
    sessions: Arc<DashMap<String, EphemeralSession>>,
    agent_sessions: Arc<DashMap<String, Vec<String>>>,
    config: Arc<RwLock<ZoneConfig>>,
}

#[derive(Debug, Clone)]
pub struct ZoneConfig {
    pub default_duration: std::time::Duration,
    pub max_memory_per_session: usize,
    pub max_sessions_per_agent: usize,
}

impl Default for ZoneConfig {
    fn default() -> Self {
        Self {
            default_duration: std::time::Duration::from_secs(3600),
            max_memory_per_session: 1000,
            max_sessions_per_agent: 5,
        }
    }
}

impl EphemeralZoneManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            agent_sessions: Arc::new(DashMap::new()),
            config: Arc::new(RwLock::new(ZoneConfig::default())),
        }
    }
    
    pub async fn create_session(&self, agent_id: &str) -> Result<String> {
        let config = self.config.read().await;
        
        let mut agent_sessions = self.agent_sessions.entry(agent_id.to_string())
            .or_insert_with(Vec::new);
        
        if agent_sessions.len() >= config.max_sessions_per_agent {
            agent_sessions.remove(0);
        }
        
        let session_id = Uuid::new_v4().to_string();
        let now = std::time::Instant::now();
        
        let session = EphemeralSession {
            id: session_id.clone(),
            agent_id: agent_id.to_string(),
            created_at: now,
            expires_at: now + config.default_duration,
            memories: Vec::new(),
            metadata: SessionMetadata {
                context: String::new(),
                goal: None,
                collaborators: Vec::new(),
                memory_limit: config.max_memory_per_session,
            },
        };
        
        self.sessions.insert(session_id.clone(), session);
        agent_sessions.push(session_id.clone());
        
        Ok(session_id)
    }
    
    pub async fn add_memory(&self, session_id: &str, memory: Memory) -> Result<()> {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            if session.memories.len() < session.metadata.memory_limit {
                session.memories.push(memory);
            } else {
                session.memories.remove(0);
                session.memories.push(memory);
            }
        }
        Ok(())
    }
    
    pub async fn get_session_memories(&self, session_id: &str) -> Result<Vec<Memory>> {
        if let Some(session) = self.sessions.get(session_id) {
            Ok(session.memories.clone())
        } else {
            Ok(vec![])
        }
    }
    
    pub async fn end_session(&self, session_id: &str) -> Result<Vec<Memory>> {
        if let Some((_, session)) = self.sessions.remove(session_id) {
            if let Some(mut agent_sessions) = self.agent_sessions.get_mut(&session.agent_id) {
                agent_sessions.retain(|id| id != session_id);
            }
            Ok(session.memories)
        } else {
            Ok(vec![])
        }
    }
    
    pub async fn extend_session(&self, session_id: &str, duration: std::time::Duration) -> Result<()> {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.expires_at = std::time::Instant::now() + duration;
        }
        Ok(())
    }
    
    pub async fn update_metadata(&self, session_id: &str, metadata: SessionMetadata) -> Result<()> {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.metadata = metadata;
        }
        Ok(())
    }
    
    pub async fn add_collaborator(&self, session_id: &str, collaborator: String) -> Result<()> {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            if !session.metadata.collaborators.contains(&collaborator) {
                session.metadata.collaborators.push(collaborator);
            }
        }
        Ok(())
    }
    
    pub async fn cleanup_expired_sessions(&self) -> Result<()> {
        let now = std::time::Instant::now();
        let mut expired_sessions = Vec::new();
        
        for session in self.sessions.iter() {
            if session.expires_at < now {
                expired_sessions.push(session.id.clone());
            }
        }
        
        for session_id in expired_sessions {
            self.end_session(&session_id).await?;
        }
        
        Ok(())
    }
    
    pub async fn get_active_sessions(&self, agent_id: &str) -> Vec<String> {
        self.agent_sessions
            .get(agent_id)
            .map(|sessions| sessions.clone())
            .unwrap_or_default()
    }
    
    pub async fn merge_sessions(&self, session_ids: Vec<String>) -> Result<String> {
        let mut merged_memories = Vec::new();
        let mut agent_id = String::new();
        
        for session_id in &session_ids {
            if let Some(session) = self.sessions.get(session_id) {
                if agent_id.is_empty() {
                    agent_id = session.agent_id.clone();
                }
                merged_memories.extend(session.memories.clone());
            }
        }
        
        if agent_id.is_empty() {
            return Err(eyre::eyre!("No valid sessions found"));
        }
        
        let new_session_id = self.create_session(&agent_id).await?;
        
        if let Some(mut new_session) = self.sessions.get_mut(&new_session_id) {
            new_session.memories = merged_memories;
        }
        
        for session_id in session_ids {
            self.end_session(&session_id).await?;
        }
        
        Ok(new_session_id)
    }
}