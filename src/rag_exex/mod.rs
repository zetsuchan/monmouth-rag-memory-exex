pub mod vector_store;
pub mod embeddings;
pub mod context_retrieval;
pub mod intent_parser;
pub mod knowledge_graph;
pub mod agent_context;

use crate::{RagEvent, shared::Metrics};
use eyre::Result;
use reth_exex::{ExExContext, ExExEvent, ExExNotification};
use reth_node_api::FullNodeComponents;
use reth_primitives::{SealedBlockWithSenders, TransactionSigned};
use reth_tracing::tracing::{info, debug, error};
use tokio::sync::mpsc;
use std::sync::Arc;
use dashmap::DashMap;

use self::{
    vector_store::VectorStore,
    embeddings::EmbeddingPipeline,
    context_retrieval::ContextRetriever,
    intent_parser::IntentParser,
    knowledge_graph::KnowledgeGraph,
};

#[derive(Debug)]
pub struct RagExEx<Node: FullNodeComponents> {
    ctx: ExExContext<Node>,
    metrics: Metrics,
    vector_store: Arc<VectorStore>,
    embedding_pipeline: Arc<EmbeddingPipeline>,
    context_retriever: Arc<ContextRetriever>,
    intent_parser: Arc<IntentParser>,
    knowledge_graph: Arc<KnowledgeGraph>,
    agent_contexts: Arc<DashMap<String, AgentContext>>,
}

#[derive(Debug, Clone)]
pub struct AgentContext {
    pub agent_id: String,
    pub context_window: Vec<String>,
    pub max_tokens: usize,
    pub current_tokens: usize,
    pub last_activity: std::time::Instant,
}

impl<Node: FullNodeComponents> RagExEx<Node> {
    pub async fn new(ctx: ExExContext<Node>, metrics: Metrics) -> Result<Self> {
        info!("Initializing RAG ExEx");
        
        let vector_store = Arc::new(VectorStore::new().await?);
        let embedding_pipeline = Arc::new(EmbeddingPipeline::new()?);
        let context_retriever = Arc::new(ContextRetriever::new(vector_store.clone()));
        let intent_parser = Arc::new(IntentParser::new()?);
        let knowledge_graph = Arc::new(KnowledgeGraph::new());
        let agent_contexts = Arc::new(DashMap::new());
        
        Ok(Self {
            ctx,
            metrics,
            vector_store,
            embedding_pipeline,
            context_retriever,
            intent_parser,
            knowledge_graph,
            agent_contexts,
        })
    }
    
    pub async fn run(&mut self, event_tx: mpsc::Sender<RagEvent>) -> Result<()> {
        info!("Starting RAG ExEx");
        
        while let Some(notification) = self.ctx.notifications.recv().await {
            match notification {
                ExExNotification::ChainCommitted { new } => {
                    for block in new.blocks() {
                        self.process_block(&block, &event_tx).await?;
                    }
                }
                ExExNotification::ChainReorged { old, new } => {
                    info!("Chain reorg detected, reprocessing blocks");
                    for block in old.blocks() {
                        self.remove_block_embeddings(&block).await?;
                    }
                    for block in new.blocks() {
                        self.process_block(&block, &event_tx).await?;
                    }
                }
                ExExNotification::ChainReverted { old } => {
                    info!("Chain reverted, removing embeddings");
                    for block in old.blocks() {
                        self.remove_block_embeddings(&block).await?;
                    }
                }
            }
            
            self.ctx.events.send(ExExEvent::FinishedHeight(
                self.ctx.notifications.tip().number
            ))?;
        }
        
        Ok(())
    }
    
    async fn process_block(
        &self,
        block: &SealedBlockWithSenders,
        event_tx: &mpsc::Sender<RagEvent>,
    ) -> Result<()> {
        debug!("Processing block {} for RAG indexing", block.number);
        
        for (tx, sender) in block.transactions_with_sender() {
            self.process_transaction(tx, sender, event_tx).await?;
        }
        
        self.metrics.record_block_processed();
        Ok(())
    }
    
    async fn process_transaction(
        &self,
        tx: &TransactionSigned,
        sender: &reth_primitives::Address,
        event_tx: &mpsc::Sender<RagEvent>,
    ) -> Result<()> {
        let tx_hash = tx.hash().to_string();
        
        let embedding = self.embedding_pipeline.generate_embedding(tx).await?;
        let embedding_size = embedding.len();
        
        self.vector_store.store_embedding(&tx_hash, embedding).await?;
        
        if let Some(agent_id) = self.extract_agent_id(tx) {
            self.update_agent_context(&agent_id, &tx_hash).await?;
            
            self.knowledge_graph.update_agent_activity(&agent_id, tx).await;
            
            if let Some(intent) = self.intent_parser.parse_transaction(tx).await? {
                let context = self.context_retriever.retrieve_context(&agent_id, &intent).await?;
                
                event_tx.send(RagEvent::ContextRetrieved {
                    agent_id: agent_id.clone(),
                    context,
                }).await?;
            }
        }
        
        event_tx.send(RagEvent::EmbeddingGenerated {
            tx_hash,
            embedding_size,
        }).await?;
        
        Ok(())
    }
    
    async fn remove_block_embeddings(&self, block: &SealedBlockWithSenders) -> Result<()> {
        for tx in &block.transactions {
            let tx_hash = tx.hash().to_string();
            self.vector_store.remove_embedding(&tx_hash).await?;
        }
        Ok(())
    }
    
    async fn update_agent_context(&self, agent_id: &str, tx_hash: &str) -> Result<()> {
        let mut context = self.agent_contexts.entry(agent_id.to_string())
            .or_insert_with(|| AgentContext {
                agent_id: agent_id.to_string(),
                context_window: Vec::new(),
                max_tokens: 32000,
                current_tokens: 0,
                last_activity: std::time::Instant::now(),
            });
        
        context.context_window.push(tx_hash.to_string());
        context.last_activity = std::time::Instant::now();
        
        if context.context_window.len() > 100 {
            context.context_window.remove(0);
        }
        
        Ok(())
    }
    
    fn extract_agent_id(&self, tx: &TransactionSigned) -> Option<String> {
        if tx.input().len() >= 4 {
            let selector = &tx.input()[0..4];
            if selector == &[0x12, 0x34, 0x56, 0x78] {
                if tx.input().len() >= 36 {
                    let agent_bytes = &tx.input()[4..36];
                    return Some(hex::encode(agent_bytes));
                }
            }
        }
        None
    }
}