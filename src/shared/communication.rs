use eyre::Result;
use tokio::sync::mpsc;
use serde::{Serialize, Deserialize};
use alloy::primitives::B256;
use reth_primitives::TransactionSigned;
use crate::memory_exex::{Memory, MemoryType};

#[derive(Debug)]
pub struct InterExExChannel {
    pub svm_sender: mpsc::Sender<CrossExExMessage>,
    pub svm_receiver: mpsc::Receiver<CrossExExMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrossExExMessage {
    TransactionAnalysis {
        tx_hash: B256,
        routing_decision: crate::shared::ai_agent::RoutingDecision,
        context: Vec<u8>,
        timestamp: std::time::Instant,
    },
    MemoryRequest {
        agent_id: String,
        memory_filter: MemoryFilter,
        response_channel: mpsc::Sender<MemoryResponse>,
    },
    RAGQuery {
        agent_id: String,
        query: String,
        max_results: usize,
        response_channel: mpsc::Sender<RAGResponse>,
    },
    StateSync {
        block_number: u64,
        state_hash: [u8; 32],
        checkpoint_data: Vec<u8>,
    },
    AgentCoordination {
        coordinator_id: String,
        agents: Vec<String>,
        action: CoordinationAction,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFilter {
    pub memory_types: Option<Vec<MemoryType>>,
    pub min_importance: Option<f64>,
    pub max_age_seconds: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResponse {
    pub memories: Vec<Memory>,
    pub total_count: usize,
    pub filtered_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGResponse {
    pub results: Vec<RAGResult>,
    pub total_matches: usize,
    pub query_embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAGResult {
    pub content: String,
    pub relevance_score: f64,
    pub source_tx_hash: Option<B256>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinationAction {
    ConsensusRequest { topic: String, timeout_ms: u64 },
    DataSharing { data_type: String, payload: Vec<u8> },
    TaskDistribution { task_id: String, subtasks: Vec<String> },
}

#[derive(Debug)]
pub struct CrossExExCoordinator {
    channels: Vec<InterExExChannel>,
    pending_requests: dashmap::DashMap<String, PendingRequest>,
}

#[derive(Debug)]
struct PendingRequest {
    request_id: String,
    request_type: RequestType,
    timestamp: std::time::Instant,
    response_count: usize,
    expected_responses: usize,
}

#[derive(Debug)]
enum RequestType {
    Memory,
    RAG,
    Coordination,
}

impl CrossExExCoordinator {
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            pending_requests: dashmap::DashMap::new(),
        }
    }
    
    pub fn add_channel(&mut self, channel: InterExExChannel) {
        self.channels.push(channel);
    }
    
    pub async fn broadcast_message(&self, message: CrossExExMessage) -> Result<()> {
        for channel in &self.channels {
            channel.svm_sender.send(message.clone()).await?;
        }
        Ok(())
    }
    
    pub async fn request_memory(
        &self,
        agent_id: String,
        filter: MemoryFilter,
    ) -> Result<Vec<MemoryResponse>> {
        let (tx, mut rx) = mpsc::channel(10);
        let request_id = uuid::Uuid::new_v4().to_string();
        
        self.pending_requests.insert(
            request_id.clone(),
            PendingRequest {
                request_id: request_id.clone(),
                request_type: RequestType::Memory,
                timestamp: std::time::Instant::now(),
                response_count: 0,
                expected_responses: self.channels.len(),
            },
        );
        
        let message = CrossExExMessage::MemoryRequest {
            agent_id,
            memory_filter: filter,
            response_channel: tx,
        };
        
        self.broadcast_message(message).await?;
        
        let mut responses = Vec::new();
        while let Some(response) = rx.recv().await {
            responses.push(response);
            
            if let Some(mut pending) = self.pending_requests.get_mut(&request_id) {
                pending.response_count += 1;
                if pending.response_count >= pending.expected_responses {
                    break;
                }
            }
        }
        
        self.pending_requests.remove(&request_id);
        Ok(responses)
    }
    
    pub async fn request_rag_query(
        &self,
        agent_id: String,
        query: String,
        max_results: usize,
    ) -> Result<Vec<RAGResponse>> {
        let (tx, mut rx) = mpsc::channel(10);
        let request_id = uuid::Uuid::new_v4().to_string();
        
        self.pending_requests.insert(
            request_id.clone(),
            PendingRequest {
                request_id: request_id.clone(),
                request_type: RequestType::RAG,
                timestamp: std::time::Instant::now(),
                response_count: 0,
                expected_responses: self.channels.len(),
            },
        );
        
        let message = CrossExExMessage::RAGQuery {
            agent_id,
            query,
            max_results,
            response_channel: tx,
        };
        
        self.broadcast_message(message).await?;
        
        let mut responses = Vec::new();
        let timeout = tokio::time::Duration::from_secs(5);
        
        loop {
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Some(response)) => {
                    responses.push(response);
                    
                    if let Some(mut pending) = self.pending_requests.get_mut(&request_id) {
                        pending.response_count += 1;
                        if pending.response_count >= pending.expected_responses {
                            break;
                        }
                    }
                }
                Ok(None) | Err(_) => break,
            }
        }
        
        self.pending_requests.remove(&request_id);
        Ok(responses)
    }
    
    pub async fn cleanup_stale_requests(&self, max_age: std::time::Duration) {
        let now = std::time::Instant::now();
        let mut to_remove = Vec::new();
        
        for request in self.pending_requests.iter() {
            if now.duration_since(request.timestamp) > max_age {
                to_remove.push(request.request_id.clone());
            }
        }
        
        for request_id in to_remove {
            self.pending_requests.remove(&request_id);
        }
    }
}