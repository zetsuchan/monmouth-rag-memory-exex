//! Inter-ExEx Communication Infrastructure
//! 
//! This module provides the foundation for communication between multiple ExEx instances,
//! enabling coordinated transaction processing, state sharing, and load balancing.

pub mod bus;
pub mod messages;
pub mod protocol;

use std::sync::Arc;
use tokio::sync::RwLock;
use eyre::Result;

pub use bus::{MessageBus, MessageBusConfig};
pub use messages::{ExExMessage, MessageType, MessagePayload, NodeInfo, NodeType, NodeStatus};
pub use protocol::{Protocol, ProtocolVersion};

/// Main coordinator for inter-ExEx communication
pub struct InterExExCoordinator {
    /// Message bus for communication
    bus: Arc<MessageBus>,
    /// Protocol handler
    protocol: Arc<Protocol>,
    /// Local node information
    node_info: Arc<RwLock<NodeInfo>>,
}

impl InterExExCoordinator {
    /// Create a new coordinator with the given configuration
    pub fn new(config: MessageBusConfig, node_id: String) -> Result<Self> {
        let bus = Arc::new(MessageBus::new(config)?);
        let protocol = Arc::new(Protocol::new(ProtocolVersion::V1));
        let mut node_info = NodeInfo::new(node_id);
        node_info.node_type = NodeType::RAG; // We are a RAG ExEx
        let node_info = Arc::new(RwLock::new(node_info));

        Ok(Self {
            bus,
            protocol,
            node_info,
        })
    }

    /// Start the coordinator
    pub async fn start(&self) -> Result<()> {
        // Start the message bus
        self.bus.start().await?;
        
        // Register node with the network
        let info = self.node_info.read().await;
        self.bus.announce_node(info.clone()).await?;
        
        Ok(())
    }

    /// Send a message to other ExEx instances
    pub async fn broadcast(&self, message: ExExMessage) -> Result<()> {
        self.bus.broadcast(message).await
    }

    /// Send a message to a specific ExEx instance
    pub async fn send_to(&self, target: &str, message: ExExMessage) -> Result<()> {
        self.bus.send_to(target, message).await
    }

    /// Subscribe to messages of a specific type
    pub async fn subscribe(&self, message_type: MessageType) -> Result<tokio::sync::mpsc::Receiver<ExExMessage>> {
        self.bus.subscribe(message_type).await
    }

    /// Update local node information
    pub async fn update_node_info<F>(&self, updater: F) -> Result<()>
    where
        F: FnOnce(&mut NodeInfo),
    {
        let mut info = self.node_info.write().await;
        updater(&mut info);
        self.bus.announce_node(info.clone()).await
    }

    /// Get current protocol version
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol.version()
    }

    /// Shutdown the coordinator
    pub async fn shutdown(&self) -> Result<()> {
        self.bus.shutdown().await
    }
}