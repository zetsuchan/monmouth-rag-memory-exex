pub mod svm_integration;

use dashmap::DashMap;
use eyre::Result;
use reth_primitives::TransactionSigned;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct AgentNode {
    pub id: String,
    pub reputation: f64,
    pub specializations: HashSet<String>,
    pub interaction_count: u64,
    pub last_active: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct AgentRelationship {
    pub agent_a: String,
    pub agent_b: String,
    pub relationship_type: RelationshipType,
    pub strength: f64,
    pub interactions: Vec<InteractionRecord>,
}

#[derive(Debug, Clone)]
pub enum RelationshipType {
    Collaborator,
    Competitor,
    ServiceProvider,
    ServiceConsumer,
    Peer,
}

#[derive(Debug, Clone)]
pub struct InteractionRecord {
    pub timestamp: std::time::Instant,
    pub tx_hash: String,
    pub interaction_type: String,
    pub outcome: f64,
}

#[derive(Debug)]
pub struct KnowledgeGraph {
    agents: Arc<DashMap<String, AgentNode>>,
    relationships: Arc<RwLock<Vec<AgentRelationship>>>,
    specialization_index: Arc<DashMap<String, HashSet<String>>>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(DashMap::new()),
            relationships: Arc::new(RwLock::new(Vec::new())),
            specialization_index: Arc::new(DashMap::new()),
        }
    }
    
    pub async fn update_agent_activity(&self, agent_id: &str, tx: &TransactionSigned) {
        let mut agent = self.agents.entry(agent_id.to_string())
            .or_insert_with(|| AgentNode {
                id: agent_id.to_string(),
                reputation: 0.5,
                specializations: HashSet::new(),
                interaction_count: 0,
                last_active: std::time::Instant::now(),
            });
        
        agent.interaction_count += 1;
        agent.last_active = std::time::Instant::now();
        
        if let Some(specialization) = self.infer_specialization(tx) {
            agent.specializations.insert(specialization.clone());
            
            self.specialization_index
                .entry(specialization)
                .or_insert_with(HashSet::new)
                .insert(agent_id.to_string());
        }
    }
    
    pub async fn record_interaction(
        &self,
        agent_a: &str,
        agent_b: &str,
        tx_hash: &str,
        interaction_type: &str,
        outcome: f64,
    ) -> Result<()> {
        let mut relationships = self.relationships.write().await;
        
        let existing = relationships.iter_mut().find(|r| {
            (r.agent_a == agent_a && r.agent_b == agent_b) ||
            (r.agent_a == agent_b && r.agent_b == agent_a)
        });
        
        if let Some(relationship) = existing {
            relationship.strength = (relationship.strength + outcome) / 2.0;
            relationship.interactions.push(InteractionRecord {
                timestamp: std::time::Instant::now(),
                tx_hash: tx_hash.to_string(),
                interaction_type: interaction_type.to_string(),
                outcome,
            });
        } else {
            relationships.push(AgentRelationship {
                agent_a: agent_a.to_string(),
                agent_b: agent_b.to_string(),
                relationship_type: self.infer_relationship_type(interaction_type),
                strength: outcome,
                interactions: vec![InteractionRecord {
                    timestamp: std::time::Instant::now(),
                    tx_hash: tx_hash.to_string(),
                    interaction_type: interaction_type.to_string(),
                    outcome,
                }],
            });
        }
        
        self.update_agent_reputation(agent_a, outcome).await;
        self.update_agent_reputation(agent_b, outcome).await;
        
        Ok(())
    }
    
    pub async fn get_agent_network(&self, agent_id: &str) -> Vec<AgentRelationship> {
        let relationships = self.relationships.read().await;
        relationships.iter()
            .filter(|r| r.agent_a == agent_id || r.agent_b == agent_id)
            .cloned()
            .collect()
    }
    
    pub async fn find_experts(&self, specialization: &str) -> Vec<AgentNode> {
        if let Some(agent_ids) = self.specialization_index.get(specialization) {
            let mut experts = Vec::new();
            for agent_id in agent_ids.iter() {
                if let Some(agent) = self.agents.get(agent_id) {
                    experts.push(agent.clone());
                }
            }
            experts.sort_by(|a, b| b.reputation.partial_cmp(&a.reputation).unwrap());
            experts
        } else {
            Vec::new()
        }
    }
    
    async fn update_agent_reputation(&self, agent_id: &str, outcome: f64) {
        if let Some(mut agent) = self.agents.get_mut(agent_id) {
            agent.reputation = (agent.reputation * 0.9) + (outcome * 0.1);
            agent.reputation = agent.reputation.max(0.0).min(1.0);
        }
    }
    
    fn infer_specialization(&self, tx: &TransactionSigned) -> Option<String> {
        if tx.value() > reth_primitives::U256::from(1000000000000000000u64) {
            Some("whale".to_string())
        } else if tx.gas_limit() > 1000000 {
            Some("complex_ops".to_string())
        } else {
            Some("standard".to_string())
        }
    }
    
    fn infer_relationship_type(&self, interaction_type: &str) -> RelationshipType {
        match interaction_type {
            "collaborate" => RelationshipType::Collaborator,
            "compete" => RelationshipType::Competitor,
            "provide" => RelationshipType::ServiceProvider,
            "consume" => RelationshipType::ServiceConsumer,
            _ => RelationshipType::Peer,
        }
    }
}