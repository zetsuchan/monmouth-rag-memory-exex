//! Message bus implementation for inter-ExEx communication

use crate::inter_exex::messages::{ExExMessage, MessageType, NodeInfo};
use dashmap::DashMap;
use eyre::Result;
use reth_tracing::tracing::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Configuration for the message bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBusConfig {
    /// Buffer size for message channels
    pub channel_buffer_size: usize,
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Network interface to bind to
    pub bind_address: String,
    /// Discovery mechanism
    pub discovery_method: DiscoveryMethod,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_secs: u64,
}

impl Default for MessageBusConfig {
    fn default() -> Self {
        Self {
            channel_buffer_size: 1000,
            max_message_size: 1024 * 1024, // 1MB
            bind_address: "0.0.0.0:0".to_string(),
            discovery_method: DiscoveryMethod::Multicast,
            heartbeat_interval_secs: 10,
        }
    }
}

/// Discovery method for finding other ExEx nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryMethod {
    /// Multicast discovery
    Multicast,
    /// Static list of peers
    Static(Vec<String>),
    /// DNS-based discovery
    DNS(String),
}

/// Subscription handle for message types
pub struct Subscription {
    receiver: mpsc::Receiver<ExExMessage>,
}

/// Main message bus for inter-ExEx communication
pub struct MessageBus {
    /// Configuration
    config: MessageBusConfig,
    /// Known nodes in the network
    nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
    /// Subscriptions by message type
    subscriptions: Arc<DashMap<MessageType, Vec<mpsc::Sender<ExExMessage>>>>,
    /// Outgoing message sender
    outgoing_tx: mpsc::Sender<ExExMessage>,
    /// Shutdown signal
    shutdown: Arc<RwLock<bool>>,
}

impl MessageBus {
    /// Create a new message bus
    pub fn new(config: MessageBusConfig) -> Result<Self> {
        let (outgoing_tx, _outgoing_rx) = mpsc::channel(config.channel_buffer_size);
        
        Ok(Self {
            config,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            subscriptions: Arc::new(DashMap::new()),
            outgoing_tx,
            shutdown: Arc::new(RwLock::new(false)),
        })
    }

    /// Start the message bus
    pub async fn start(&self) -> Result<()> {
        info!("Starting message bus with config: {:?}", self.config);
        
        // Start discovery service
        match &self.config.discovery_method {
            DiscoveryMethod::Multicast => {
                debug!("Starting multicast discovery");
                // TODO: Implement multicast discovery
            }
            DiscoveryMethod::Static(peers) => {
                debug!("Using static peer list: {:?}", peers);
                // TODO: Connect to static peers
            }
            DiscoveryMethod::DNS(domain) => {
                debug!("Starting DNS discovery for domain: {}", domain);
                // TODO: Implement DNS discovery
            }
        }
        
        // Start heartbeat service
        self.start_heartbeat().await?;
        
        Ok(())
    }

    /// Announce node to the network
    pub async fn announce_node(&self, node_info: NodeInfo) -> Result<()> {
        debug!("Announcing node: {:?}", node_info);
        
        // Add to local node list
        let mut nodes = self.nodes.write().await;
        nodes.insert(node_info.node_id.clone(), node_info.clone());
        
        // Broadcast announcement
        let message = ExExMessage::new(
            MessageType::NodeAnnouncement,
            node_info.node_id.clone(),
            crate::inter_exex::messages::MessagePayload::NodeInfo(node_info),
        );
        
        self.broadcast_internal(message).await?;
        
        Ok(())
    }

    /// Broadcast a message to all nodes
    pub async fn broadcast(&self, message: ExExMessage) -> Result<()> {
        if message.validate() {
            self.broadcast_internal(message).await
        } else {
            Err(eyre::eyre!("Invalid message"))
        }
    }

    /// Send a message to a specific node
    pub async fn send_to(&self, target: &str, mut message: ExExMessage) -> Result<()> {
        message.target = Some(target.to_string());
        
        if message.validate() {
            self.send_internal(message).await
        } else {
            Err(eyre::eyre!("Invalid message"))
        }
    }

    /// Subscribe to messages of a specific type
    pub async fn subscribe(&self, message_type: MessageType) -> Result<mpsc::Receiver<ExExMessage>> {
        let (tx, rx) = mpsc::channel(self.config.channel_buffer_size);
        
        self.subscriptions
            .entry(message_type)
            .or_insert_with(Vec::new)
            .push(tx);
        
        Ok(rx)
    }

    /// Shutdown the message bus
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down message bus");
        
        let mut shutdown = self.shutdown.write().await;
        *shutdown = true;
        
        // Clear subscriptions
        self.subscriptions.clear();
        
        // Clear node list
        let mut nodes = self.nodes.write().await;
        nodes.clear();
        
        Ok(())
    }

    /// Internal broadcast implementation
    async fn broadcast_internal(&self, message: ExExMessage) -> Result<()> {
        // Deliver to local subscribers
        if let Some(subscribers) = self.subscriptions.get(&message.message_type) {
            for subscriber in subscribers.iter() {
                if let Err(e) = subscriber.send(message.clone()).await {
                    warn!("Failed to deliver message to subscriber: {}", e);
                }
            }
        }
        
        // Send to network
        if let Err(e) = self.outgoing_tx.send(message).await {
            error!("Failed to queue outgoing message: {}", e);
        }
        
        Ok(())
    }

    /// Internal send implementation
    async fn send_internal(&self, message: ExExMessage) -> Result<()> {
        // For now, just broadcast internally
        // TODO: Implement actual targeted sending
        self.broadcast_internal(message).await
    }

    /// Start heartbeat service
    async fn start_heartbeat(&self) -> Result<()> {
        let interval = self.config.heartbeat_interval_secs;
        let shutdown = self.shutdown.clone();
        let outgoing_tx = self.outgoing_tx.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval));
            
            loop {
                interval.tick().await;
                
                let is_shutdown = *shutdown.read().await;
                if is_shutdown {
                    break;
                }
                
                // Send heartbeat
                let heartbeat = ExExMessage::new(
                    MessageType::Heartbeat,
                    "self".to_string(), // TODO: Use actual node ID
                    crate::inter_exex::messages::MessagePayload::Empty,
                );
                
                if let Err(e) = outgoing_tx.send(heartbeat).await {
                    error!("Failed to send heartbeat: {}", e);
                }
            }
        });
        
        Ok(())
    }
}