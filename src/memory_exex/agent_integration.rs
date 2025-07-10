use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::context::preprocessing::{PreprocessedContext, ContextPreprocessor};
use crate::rag_exex::context_retrieval::{ContextRetriever, RetrievalConfig};
use crate::shared::types::{AgentContext, AgentMemoryState};
use crate::shared::communication::CrossExExMessage;

use super::memory_store::MemoryStore;
use super::types::{MemoryEntry, MemoryRequest, MemoryResponse, QueryCriteria};
use crate::memory_exex::{Memory, MemoryType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationConfig {
    pub max_context_window: usize,
    pub memory_priority_threshold: f64,
    pub enable_episodic_recall: bool,
    pub enable_semantic_linking: bool,
    pub context_relevance_threshold: f64,
}

impl Default for IntegrationConfig {
    fn default() -> Self {
        Self {
            max_context_window: 50,
            memory_priority_threshold: 0.7,
            enable_episodic_recall: true,
            enable_semantic_linking: true,
            context_relevance_threshold: 0.6,
        }
    }
}

pub struct AgentMemoryIntegration {
    config: IntegrationConfig,
    memory_store: Arc<MemoryStore>,
    context_retriever: Arc<ContextRetriever>,
    context_preprocessor: Arc<ContextPreprocessor>,
    agent_states: Arc<RwLock<HashMap<Address, AgentMemoryState>>>,
    event_tx: mpsc::Sender<IntegrationEvent>,
}

#[derive(Debug, Clone)]
pub enum IntegrationEvent {
    ContextAccessed {
        agent: Address,
        context_hash: B256,
        memory_types: Vec<MemoryType>,
    },
    MemoryLinked {
        agent: Address,
        source_memory: B256,
        target_memory: B256,
        link_strength: f64,
    },
    StateUpdated {
        agent: Address,
        old_state: AgentMemoryState,
        new_state: AgentMemoryState,
    },
}

impl AgentMemoryIntegration {
    pub fn new(
        config: IntegrationConfig,
        memory_store: Arc<MemoryStore>,
        context_retriever: Arc<ContextRetriever>,
        context_preprocessor: Arc<ContextPreprocessor>,
    ) -> (Self, mpsc::Receiver<IntegrationEvent>) {
        let (event_tx, event_rx) = mpsc::channel(1000);

        (
            Self {
                config,
                memory_store,
                context_retriever,
                context_preprocessor,
                agent_states: Arc::new(RwLock::new(HashMap::new())),
                event_tx,
            },
            event_rx,
        )
    }

    pub async fn build_memory_aware_context(
        &self,
        agent_address: Address,
        preprocessed_context: &PreprocessedContext,
    ) -> Result<Vec<B256>, IntegrationError> {
        // Get or create agent memory state
        let mut states = self.agent_states.write().await;
        let agent_state = states.entry(agent_address).or_insert_with(|| {
            AgentMemoryState {
                agent_address,
                working_memory: vec![],
                episodic_buffer: vec![],
                last_access_time: chrono::Utc::now().timestamp() as u64,
                context_switches: 0,
            }
        });

        // Retrieve relevant memories based on context
        let mut relevant_memories = Vec::new();

        // 1. Check working memory for immediate context
        for memory_hash in &agent_state.working_memory {
            if let Ok(Some(memory)) = self.memory_store.get(*memory_hash).await {
                if self.is_memory_relevant(&memory, preprocessed_context) {
                    relevant_memories.push(*memory_hash);
                }
            }
        }

        // 2. Episodic recall based on semantic similarity
        if self.config.enable_episodic_recall {
            let episodic_memories = self.retrieve_episodic_memories(
                agent_address,
                &preprocessed_context.semantic_tags,
            ).await?;
            relevant_memories.extend(episodic_memories);
        }

        // 3. Semantic linking to find related memories
        if self.config.enable_semantic_linking {
            let linked_memories = self.find_semantically_linked_memories(
                &relevant_memories,
                preprocessed_context,
            ).await?;
            relevant_memories.extend(linked_memories);
        }

        // Update agent state
        agent_state.working_memory = relevant_memories
            .iter()
            .take(self.config.max_context_window)
            .cloned()
            .collect();
        agent_state.last_access_time = chrono::Utc::now().timestamp() as u64;
        agent_state.context_switches += 1;

        // Emit event
        let _ = self.event_tx.send(IntegrationEvent::ContextAccessed {
            agent: agent_address,
            context_hash: preprocessed_context.transaction_hash,
            memory_types: vec![MemoryType::Working, MemoryType::Episodic],
        }).await;

        Ok(relevant_memories)
    }

    pub async fn store_agent_memory(
        &self,
        agent_address: Address,
        context: &PreprocessedContext,
        memory_type: MemoryType,
    ) -> Result<B256, IntegrationError> {
        let content = bincode::serialize(context)
            .map_err(|e| IntegrationError::SerializationError(e.to_string()))?;

        let memory_entry = MemoryEntry {
            content,
            memory_type,
            timestamp: context.timestamp,
            metadata: HashMap::new(),
        };

        let memory_hash = self.memory_store.store(memory_entry).await
            .map_err(|e| IntegrationError::StorageError(e.to_string()))?;

        // Update agent state
        let mut states = self.agent_states.write().await;
        if let Some(agent_state) = states.get_mut(&agent_address) {
            match memory_type {
                MemoryType::Working => {
                    agent_state.working_memory.push(memory_hash);
                    // Trim to max window size
                    if agent_state.working_memory.len() > self.config.max_context_window {
                        agent_state.working_memory.remove(0);
                    }
                }
                MemoryType::Episodic => {
                    agent_state.episodic_buffer.push(memory_hash);
                    // Keep last 100 episodic memories
                    if agent_state.episodic_buffer.len() > 100 {
                        agent_state.episodic_buffer.remove(0);
                    }
                }
                _ => {}
            }
        }

        Ok(memory_hash)
    }

    pub async fn handle_memory_request(
        &self,
        request: MemoryRequest,
    ) -> Result<MemoryResponse, IntegrationError> {
        match request {
            MemoryRequest::Store { content, memory_type } => {
                let memory_entry = MemoryEntry {
                    content: content.clone(),
                    memory_type,
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    metadata: HashMap::new(),
                };

                let hash = self.memory_store.store(memory_entry).await
                    .map_err(|e| IntegrationError::StorageError(e.to_string()))?;

                Ok(MemoryResponse::Stored { hash })
            }
            MemoryRequest::Retrieve { hash } => {
                let memory = self.memory_store.get(hash).await
                    .map_err(|e| IntegrationError::StorageError(e.to_string()))?;

                Ok(MemoryResponse::Retrieved { memory })
            }
            MemoryRequest::Query { criteria } => {
                // Implement query logic based on criteria
                let results = self.query_memories(criteria).await?;
                Ok(MemoryResponse::QueryResults { results })
            }
        }
    }

    async fn retrieve_episodic_memories(
        &self,
        agent_address: Address,
        semantic_tags: &[String],
    ) -> Result<Vec<B256>, IntegrationError> {
        let states = self.agent_states.read().await;
        let agent_state = states.get(&agent_address);

        if let Some(state) = agent_state {
            let mut episodic_memories = Vec::new();

            for memory_hash in &state.episodic_buffer {
                if let Ok(Some(memory)) = self.memory_store.get(*memory_hash).await {
                    // Check semantic relevance
                    if let Ok(context) = bincode::deserialize::<PreprocessedContext>(&memory.content) {
                        let relevance = self.calculate_semantic_relevance(
                            &context.semantic_tags,
                            semantic_tags,
                        );

                        if relevance > self.config.context_relevance_threshold {
                            episodic_memories.push(*memory_hash);
                        }
                    }
                }
            }

            Ok(episodic_memories)
        } else {
            Ok(vec![])
        }
    }

    async fn find_semantically_linked_memories(
        &self,
        base_memories: &[B256],
        context: &PreprocessedContext,
    ) -> Result<Vec<B256>, IntegrationError> {
        let mut linked_memories = Vec::new();

        for memory_hash in base_memories {
            // Get linked memories from the store
            if let Ok(links) = self.memory_store.get_links(*memory_hash).await {
                for (linked_hash, strength) in links {
                    if strength > self.config.memory_priority_threshold {
                        linked_memories.push(linked_hash);

                        // Emit linking event
                        let _ = self.event_tx.send(IntegrationEvent::MemoryLinked {
                            agent: context.agent_address,
                            source_memory: *memory_hash,
                            target_memory: linked_hash,
                            link_strength: strength,
                        }).await;
                    }
                }
            }
        }

        Ok(linked_memories)
    }

    fn is_memory_relevant(
        &self,
        memory: &MemoryEntry,
        context: &PreprocessedContext,
    ) -> bool {
        // Check if memory is recent enough
        let age = context.timestamp.saturating_sub(memory.timestamp);
        if age > 3600 { // Older than 1 hour
            return false;
        }

        // Check priority score
        if context.priority_score < self.config.memory_priority_threshold {
            return false;
        }

        // Additional relevance checks can be added here
        true
    }

    fn calculate_semantic_relevance(
        &self,
        tags1: &[String],
        tags2: &[String],
    ) -> f64 {
        let set1: std::collections::HashSet<_> = tags1.iter().collect();
        let set2: std::collections::HashSet<_> = tags2.iter().collect();

        let intersection = set1.intersection(&set2).count() as f64;
        let union = set1.union(&set2).count() as f64;

        if union == 0.0 {
            0.0
        } else {
            intersection / union // Jaccard similarity
        }
    }

    async fn query_memories(
        &self,
        criteria: serde_json::Value,
    ) -> Result<Vec<(B256, MemoryEntry)>, IntegrationError> {
        // Parse criteria and query memory store
        // This is a simplified implementation
        let memories = self.memory_store.query_all().await
            .map_err(|e| IntegrationError::StorageError(e.to_string()))?;

        // Filter based on criteria
        let filtered: Vec<_> = memories
            .into_iter()
            .filter(|(_, entry)| {
                // Apply filtering logic based on criteria
                true // Placeholder
            })
            .collect();

        Ok(filtered)
    }

    pub async fn get_agent_state(&self, agent_address: Address) -> Option<AgentMemoryState> {
        self.agent_states.read().await.get(&agent_address).cloned()
    }

    pub async fn clear_agent_memory(&self, agent_address: Address) {
        let mut states = self.agent_states.write().await;
        states.remove(&agent_address);
    }

    pub async fn get_memory_statistics(&self) -> MemoryStatistics {
        let states = self.agent_states.read().await;

        let total_agents = states.len();
        let total_working_memories: usize = states
            .values()
            .map(|s| s.working_memory.len())
            .sum();
        let total_episodic_memories: usize = states
            .values()
            .map(|s| s.episodic_buffer.len())
            .sum();

        MemoryStatistics {
            total_agents,
            total_working_memories,
            total_episodic_memories,
            average_context_switches: states
                .values()
                .map(|s| s.context_switches as f64)
                .sum::<f64>() / total_agents as f64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryStatistics {
    pub total_agents: usize,
    pub total_working_memories: usize,
    pub total_episodic_memories: usize,
    pub average_context_switches: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Retrieval error: {0}")]
    RetrievalError(String),
    #[error("Invalid configuration: {0}")]
    ConfigError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::preprocessing::ProcessingConfig;
    use crate::shared::types::AgentAction;
    use alloy::primitives::U256;

    #[tokio::test]
    async fn test_memory_integration() {
        let config = IntegrationConfig::default();
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        let context_preprocessor = Arc::new(ContextPreprocessor::new(ProcessingConfig::default()));

        let (integration, mut event_rx) = AgentMemoryIntegration::new(
            config,
            memory_store,
            context_retriever,
            context_preprocessor,
        );

        let agent_address = Address::from([1u8; 20]);
        let context = PreprocessedContext {
            transaction_hash: B256::from([2u8; 32]),
            agent_address,
            action_type: AgentAction::Transfer {
                to: Address::from([3u8; 20]),
                amount: U256::from(1000),
            },
            extracted_features: Default::default(),
            semantic_tags: vec!["transfer".to_string()],
            priority_score: 0.8,
            timestamp: 12345,
            compressed_data: None,
        };

        // Store memory
        let memory_hash = integration
            .store_agent_memory(agent_address, &context, MemoryType::Working)
            .await
            .unwrap();

        // Build context
        let memories = integration
            .build_memory_aware_context(agent_address, &context)
            .await
            .unwrap();

        assert!(!memories.is_empty());
        assert!(memories.contains(&memory_hash));

        // Check event
        if let Ok(event) = event_rx.try_recv() {
            match event {
                IntegrationEvent::ContextAccessed { agent, .. } => {
                    assert_eq!(agent, agent_address);
                }
                _ => panic!("Unexpected event type"),
            }
        }
    }
}