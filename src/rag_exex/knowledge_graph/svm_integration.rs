use alloy::primitives::{Address, B256};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::dijkstra;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

use crate::context::preprocessing::PreprocessedContext;
use crate::shared::types::{AgentAction, AgentContext};
use crate::shared::transaction_analyzer::TransactionAnalysis;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub node_type: NodeType,
    pub address: Address,
    pub metadata: NodeMetadata,
    pub last_updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    Agent,
    Contract,
    Pool,
    Token,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetadata {
    pub label: String,
    pub activity_score: f64,
    pub trust_score: f64,
    pub specialization: Vec<String>,
    pub transaction_count: u64,
    pub volume_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub edge_type: EdgeType,
    pub weight: f64,
    pub transaction_count: u64,
    pub last_interaction: u64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EdgeType {
    Transfer,
    Swap,
    LiquidityProvision,
    Delegation,
    Interaction,
}

pub struct SVMGraphIntegration {
    graph: Arc<RwLock<DiGraph<GraphNode, GraphEdge>>>,
    node_index_map: Arc<RwLock<HashMap<Address, NodeIndex>>>,
    temporal_edges: Arc<RwLock<VecDeque<TemporalEdge>>>,
    config: GraphConfig,
    metrics: Arc<RwLock<GraphMetrics>>,
    update_tx: mpsc::Sender<RelationshipUpdate>,
}

#[derive(Debug, Clone)]
struct TemporalEdge {
    from: Address,
    to: Address,
    edge_type: EdgeType,
    timestamp: u64,
    weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    pub max_temporal_edges: usize,
    pub edge_decay_rate: f64,
    pub min_edge_weight: f64,
    pub pagerank_damping: f64,
    pub prune_interval_seconds: u64,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            max_temporal_edges: 10000,
            edge_decay_rate: 0.95,
            min_edge_weight: 0.01,
            pagerank_damping: 0.85,
            prune_interval_seconds: 3600,
        }
    }
}

#[derive(Debug, Default)]
struct GraphMetrics {
    total_nodes: u64,
    total_edges: u64,
    pruned_edges: u64,
    relationship_updates: u64,
}

#[derive(Debug, Clone)]
pub struct RelationshipUpdate {
    pub from_agent: Address,
    pub to_entity: Address,
    pub action: AgentAction,
    pub timestamp: u64,
    pub transaction_hash: B256,
}

impl SVMGraphIntegration {
    pub fn new(config: GraphConfig) -> (Self, mpsc::Receiver<RelationshipUpdate>) {
        let (update_tx, update_rx) = mpsc::channel(1000);
        
        let integration = Self {
            graph: Arc::new(RwLock::new(DiGraph::new())),
            node_index_map: Arc::new(RwLock::new(HashMap::new())),
            temporal_edges: Arc::new(RwLock::new(VecDeque::with_capacity(config.max_temporal_edges))),
            config,
            metrics: Arc::new(RwLock::new(GraphMetrics::default())),
            update_tx,
        };
        
        // Start pruning task
        integration.start_pruning_task();
        
        (integration, update_rx)
    }

    pub async fn track_agent_relationship(
        &self,
        transaction: &TransactionAnalysis,
        context: &PreprocessedContext,
    ) -> Result<(), GraphError> {
        let from_agent = context.agent_address;
        
        match &context.action_type {
            AgentAction::Transfer { to, amount } => {
                self.update_transfer_relationship(from_agent, *to, amount.to::<f64>(), transaction).await?;
            }
            AgentAction::Swap { token_in, token_out, amount_in, .. } => {
                self.update_swap_relationship(from_agent, *token_in, *token_out, amount_in.to::<f64>(), transaction).await?;
            }
            AgentAction::AddLiquidity { token_a, token_b, amount_a, amount_b, .. } => {
                self.update_liquidity_relationship(from_agent, *token_a, *token_b, amount_a.to::<f64>() + amount_b.to::<f64>(), transaction).await?;
            }
            _ => {}
        }
        
        // Send update notification
        let update = RelationshipUpdate {
            from_agent,
            to_entity: transaction.to,
            action: context.action_type.clone(),
            timestamp: transaction.timestamp,
            transaction_hash: transaction.transaction_hash,
        };
        
        let _ = self.update_tx.send(update).await;
        self.increment_relationship_updates().await;
        
        Ok(())
    }

    pub async fn update_graph_from_patterns(
        &self,
        patterns: Vec<TransactionPattern>,
    ) -> Result<(), GraphError> {
        for pattern in patterns {
            match pattern {
                TransactionPattern::FrequentPair { agent1, agent2, frequency } => {
                    self.strengthen_relationship(agent1, agent2, frequency as f64).await?;
                }
                TransactionPattern::Hub { center, connected } => {
                    for node in connected {
                        self.mark_as_hub_connection(center, node).await?;
                    }
                }
                TransactionPattern::Bridge { node, clusters } => {
                    self.mark_as_bridge(node, clusters).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn calculate_relationship_strength(
        &self,
        agent1: Address,
        agent2: Address,
    ) -> f64 {
        let graph = self.graph.read().await;
        let node_map = self.node_index_map.read().await;
        
        if let (Some(&idx1), Some(&idx2)) = (node_map.get(&agent1), node_map.get(&agent2)) {
            if let Some(edge) = graph.find_edge(idx1, idx2) {
                return graph[edge].weight;
            }
        }
        
        0.0
    }

    pub async fn find_shortest_path(
        &self,
        from: Address,
        to: Address,
    ) -> Option<Vec<Address>> {
        let graph = self.graph.read().await;
        let node_map = self.node_index_map.read().await;
        
        let start_idx = node_map.get(&from)?;
        let end_idx = node_map.get(&to)?;
        
        let predecessors = dijkstra(&*graph, *start_idx, Some(*end_idx), |e| e.weight().weight);
        
        if !predecessors.contains_key(end_idx) {
            return None;
        }
        
        // Reconstruct path
        let mut path = Vec::new();
        let mut current = *end_idx;
        
        while current != *start_idx {
            path.push(graph[current].address);
            // Find predecessor
            for (node, _) in &predecessors {
                if graph.find_edge(*node, current).is_some() {
                    current = *node;
                    break;
                }
            }
        }
        
        path.push(from);
        path.reverse();
        
        Some(path)
    }

    pub async fn calculate_agent_influence(&self, agent: Address) -> f64 {
        let graph = self.graph.read().await;
        let node_map = self.node_index_map.read().await;
        
        if !node_map.contains_key(&agent) {
            return 0.0;
        }
        
        // Calculate simple reputation score based on node connectivity
        if let Some(&idx) = node_map.get(&agent) {
            let neighbors = graph.neighbors(idx).count();
            let edges = graph.edges(idx).count();
            // Simple scoring based on connectivity
            (neighbors as f64 + edges as f64) / (graph.node_count() as f64).max(1.0)
        } else {
            0.0
        }
    }

    pub async fn get_agent_network(
        &self,
        agent: Address,
        depth: usize,
    ) -> HashMap<Address, NetworkNode> {
        let graph = self.graph.read().await;
        let node_map = self.node_index_map.read().await;
        
        let mut network = HashMap::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        
        if let Some(&start_idx) = node_map.get(&agent) {
            queue.push_back((start_idx, 0));
            visited.insert(start_idx);
            
            while let Some((current_idx, current_depth)) = queue.pop_front() {
                if current_depth > depth {
                    break;
                }
                
                let node = &graph[current_idx];
                network.insert(node.address, NetworkNode {
                    address: node.address,
                    node_type: node.node_type.clone(),
                    depth: current_depth,
                    connections: graph.edges(current_idx).count(),
                });
                
                // Add neighbors
                for neighbor in graph.neighbors(current_idx) {
                    if !visited.contains(&neighbor) && current_depth < depth {
                        visited.insert(neighbor);
                        queue.push_back((neighbor, current_depth + 1));
                    }
                }
            }
        }
        
        network
    }

    async fn update_transfer_relationship(
        &self,
        from: Address,
        to: Address,
        amount: f64,
        transaction: &TransactionAnalysis,
    ) -> Result<(), GraphError> {
        let mut graph = self.graph.write().await;
        let mut node_map = self.node_index_map.write().await;
        
        // Ensure nodes exist
        let from_idx = self.ensure_node(&mut graph, &mut node_map, from, NodeType::Agent).await;
        let to_idx = self.ensure_node(&mut graph, &mut node_map, to, NodeType::Agent).await;
        
        // Update or create edge
        if let Some(edge) = graph.find_edge(from_idx, to_idx) {
            let edge_data = &mut graph[edge];
            edge_data.weight = (edge_data.weight * 0.9 + amount.log10() * 0.1).min(1.0);
            edge_data.transaction_count += 1;
            edge_data.last_interaction = transaction.timestamp;
        } else {
            graph.add_edge(from_idx, to_idx, GraphEdge {
                edge_type: EdgeType::Transfer,
                weight: amount.log10() / 10.0,
                transaction_count: 1,
                last_interaction: transaction.timestamp,
                metadata: HashMap::new(),
            });
            self.increment_total_edges().await;
        }
        
        // Add temporal edge
        self.add_temporal_edge(from, to, EdgeType::Transfer, transaction.timestamp, amount).await;
        
        Ok(())
    }

    async fn update_swap_relationship(
        &self,
        agent: Address,
        token_in: Address,
        token_out: Address,
        amount: f64,
        transaction: &TransactionAnalysis,
    ) -> Result<(), GraphError> {
        let mut graph = self.graph.write().await;
        let mut node_map = self.node_index_map.write().await;
        
        let agent_idx = self.ensure_node(&mut graph, &mut node_map, agent, NodeType::Agent).await;
        let token_in_idx = self.ensure_node(&mut graph, &mut node_map, token_in, NodeType::Token).await;
        let token_out_idx = self.ensure_node(&mut graph, &mut node_map, token_out, NodeType::Token).await;
        
        // Create swap relationships
        for (from_idx, to_idx) in [(agent_idx, token_in_idx), (token_out_idx, agent_idx)] {
            if graph.find_edge(from_idx, to_idx).is_none() {
                graph.add_edge(from_idx, to_idx, GraphEdge {
                    edge_type: EdgeType::Swap,
                    weight: 0.5,
                    transaction_count: 1,
                    last_interaction: transaction.timestamp,
                    metadata: HashMap::new(),
                });
                self.increment_total_edges().await;
            }
        }
        
        Ok(())
    }

    async fn update_liquidity_relationship(
        &self,
        agent: Address,
        token_a: Address,
        token_b: Address,
        total_value: f64,
        transaction: &TransactionAnalysis,
    ) -> Result<(), GraphError> {
        let mut graph = self.graph.write().await;
        let mut node_map = self.node_index_map.write().await;
        
        let agent_idx = self.ensure_node(&mut graph, &mut node_map, agent, NodeType::Agent).await;
        let pool_address = self.derive_pool_address(token_a, token_b);
        let pool_idx = self.ensure_node(&mut graph, &mut node_map, pool_address, NodeType::Pool).await;
        
        // Create liquidity provision edge
        if let Some(edge) = graph.find_edge(agent_idx, pool_idx) {
            let edge_data = &mut graph[edge];
            edge_data.weight = (edge_data.weight + total_value.log10() / 100.0).min(1.0);
            edge_data.transaction_count += 1;
        } else {
            graph.add_edge(agent_idx, pool_idx, GraphEdge {
                edge_type: EdgeType::LiquidityProvision,
                weight: total_value.log10() / 10.0,
                transaction_count: 1,
                last_interaction: transaction.timestamp,
                metadata: HashMap::new(),
            });
            self.increment_total_edges().await;
        }
        
        Ok(())
    }

    async fn ensure_node(
        &self,
        graph: &mut DiGraph<GraphNode, GraphEdge>,
        node_map: &mut HashMap<Address, NodeIndex>,
        address: Address,
        node_type: NodeType,
    ) -> NodeIndex {
        if let Some(&idx) = node_map.get(&address) {
            return idx;
        }
        
        let node = GraphNode {
            node_type,
            address,
            metadata: NodeMetadata {
                label: format!("{:?}", address),
                activity_score: 0.0,
                trust_score: 0.5,
                specialization: vec![],
                transaction_count: 0,
                volume_usd: 0.0,
            },
            last_updated: chrono::Utc::now().timestamp() as u64,
        };
        
        let idx = graph.add_node(node);
        node_map.insert(address, idx);
        
        let _ = self.increment_total_nodes().await;
        
        idx
    }

    async fn add_temporal_edge(
        &self,
        from: Address,
        to: Address,
        edge_type: EdgeType,
        timestamp: u64,
        weight: f64,
    ) {
        let mut temporal = self.temporal_edges.write().await;
        
        temporal.push_back(TemporalEdge {
            from,
            to,
            edge_type,
            timestamp,
            weight,
        });
        
        // Limit size
        while temporal.len() > self.config.max_temporal_edges {
            temporal.pop_front();
        }
    }

    async fn strengthen_relationship(
        &self,
        agent1: Address,
        agent2: Address,
        strength: f64,
    ) -> Result<(), GraphError> {
        let mut graph = self.graph.write().await;
        let node_map = self.node_index_map.read().await;
        
        if let (Some(&idx1), Some(&idx2)) = (node_map.get(&agent1), node_map.get(&agent2)) {
            if let Some(edge) = graph.find_edge(idx1, idx2) {
                graph[edge].weight = (graph[edge].weight + strength / 100.0).min(1.0);
            }
        }
        
        Ok(())
    }

    async fn mark_as_hub_connection(
        &self,
        hub: Address,
        connected: Address,
    ) -> Result<(), GraphError> {
        let mut graph = self.graph.write().await;
        let node_map = self.node_index_map.read().await;
        
        if let Some(&hub_idx) = node_map.get(&hub) {
            graph[hub_idx].metadata.activity_score += 0.1;
            graph[hub_idx].metadata.specialization.push("hub".to_string());
        }
        
        Ok(())
    }

    async fn mark_as_bridge(
        &self,
        bridge: Address,
        clusters: Vec<Vec<Address>>,
    ) -> Result<(), GraphError> {
        let mut graph = self.graph.write().await;
        let node_map = self.node_index_map.read().await;
        
        if let Some(&bridge_idx) = node_map.get(&bridge) {
            graph[bridge_idx].metadata.specialization.push("bridge".to_string());
            graph[bridge_idx].metadata.trust_score += 0.1;
        }
        
        Ok(())
    }

    fn derive_pool_address(&self, token_a: Address, token_b: Address) -> Address {
        // Simplified pool address derivation
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        
        // Sort tokens for consistency
        let (t1, t2) = if token_a < token_b {
            (token_a, token_b)
        } else {
            (token_b, token_a)
        };
        
        hasher.update(t1.as_bytes());
        hasher.update(t2.as_bytes());
        let hash = hasher.finalize();
        
        Address::from_slice(&hash[..20])
    }

    fn start_pruning_task(&self) {
        let graph = self.graph.clone();
        let node_map = self.node_index_map.clone();
        let config = self.config.clone();
        let metrics = self.metrics.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(config.prune_interval_seconds));
            
            loop {
                interval.tick().await;
                
                let mut graph = graph.write().await;
                let current_time = chrono::Utc::now().timestamp() as u64;
                let mut edges_to_remove = Vec::new();
                
                // Find edges to prune
                for edge in graph.edge_indices() {
                    let edge_data = &graph[edge];
                    let age = current_time - edge_data.last_interaction;
                    let decayed_weight = edge_data.weight * config.edge_decay_rate.powf(age as f64 / 86400.0);
                    
                    if decayed_weight < config.min_edge_weight {
                        edges_to_remove.push(edge);
                    }
                }
                
                // Remove edges
                for edge in edges_to_remove {
                    graph.remove_edge(edge);
                    let mut m = metrics.write().await;
                    m.pruned_edges += 1;
                    m.total_edges = m.total_edges.saturating_sub(1);
                }
            }
        });
    }

    async fn increment_total_nodes(&self) -> Result<(), tokio::sync::TryLockError> {
        if let Ok(mut metrics) = self.metrics.try_write() {
            metrics.total_nodes += 1;
        }
        Ok(())
    }

    async fn increment_total_edges(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.total_edges += 1;
    }

    async fn increment_relationship_updates(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.relationship_updates += 1;
    }

    pub async fn get_metrics(&self) -> GraphMetrics {
        self.metrics.read().await.clone()
    }
}

#[derive(Debug, Clone)]
pub enum TransactionPattern {
    FrequentPair {
        agent1: Address,
        agent2: Address,
        frequency: u64,
    },
    Hub {
        center: Address,
        connected: Vec<Address>,
    },
    Bridge {
        node: Address,
        clusters: Vec<Vec<Address>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkNode {
    pub address: Address,
    pub node_type: NodeType,
    pub depth: usize,
    pub connections: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("Node not found: {0}")]
    NodeNotFound(Address),
    #[error("Invalid relationship")]
    InvalidRelationship,
    #[error("Graph operation failed: {0}")]
    OperationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::U256;

    #[tokio::test]
    async fn test_graph_integration() {
        let config = GraphConfig::default();
        let (integration, mut update_rx) = SVMGraphIntegration::new(config);

        let agent1 = Address::from([1u8; 20]);
        let agent2 = Address::from([2u8; 20]);

        let transaction = TransactionAnalysis {
            transaction_hash: B256::from([3u8; 32]),
            timestamp: 12345,
            from: agent1,
            to: agent2,
            value: U256::from(1000),
            gas_used: 21000,
            routing_decision: crate::shared::communication::RoutingDecision::RAG,
            context: vec![],
        };

        let context = PreprocessedContext {
            transaction_hash: transaction.transaction_hash,
            agent_address: agent1,
            action_type: AgentAction::Transfer {
                to: agent2,
                amount: U256::from(1000),
            },
            extracted_features: Default::default(),
            semantic_tags: vec!["transfer".to_string()],
            priority_score: 0.8,
            timestamp: 12345,
            compressed_data: None,
        };

        integration.track_agent_relationship(&transaction, &context).await.unwrap();

        // Check relationship strength
        let strength = integration.calculate_relationship_strength(agent1, agent2).await;
        assert!(strength > 0.0);

        // Check update was sent
        if let Ok(update) = update_rx.try_recv() {
            assert_eq!(update.from_agent, agent1);
            assert_eq!(update.to_entity, agent2);
        }
    }
}