// Copyright (c) 2025 Monmouth Contributors
// 
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
// 
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
// 
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

#![warn(missing_debug_implementations, unreachable_pub)]
#![deny(unused_must_use, rust_2018_idioms)]

pub mod rag_exex;
pub mod memory_exex;
pub mod shared;
pub mod integrations;
pub mod context;
pub mod alh;
pub mod reorg;
pub mod agents;
pub mod enhanced_processor;

use eyre::Result;
use reth_exex::{ExExContext, ExExEvent, ExExNotification};
use reth_node_api::FullNodeComponents;
use reth_primitives::SealedBlockWithSenders;
use reth_tracing::tracing::{info, error};
use std::future::Future;
use tokio::sync::mpsc;

pub use rag_exex::RagExEx;
pub use memory_exex::MemoryExEx;
pub use shared::{AgentStandard, Metrics};

#[derive(Debug)]
pub struct MonmouthExExManager<Node: FullNodeComponents> {
    rag_exex: RagExEx<Node>,
    memory_exex: MemoryExEx<Node>,
    metrics: Metrics,
}

impl<Node: FullNodeComponents> MonmouthExExManager<Node> {
    pub async fn new(ctx: ExExContext<Node>) -> Result<Self> {
        let metrics = Metrics::new()?;
        
        let rag_exex = RagExEx::new(ctx.clone(), metrics.clone()).await?;
        let memory_exex = MemoryExEx::new(ctx.clone(), metrics.clone()).await?;
        
        Ok(Self {
            rag_exex,
            memory_exex,
            metrics,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        info!("Starting Monmouth RAG x Memory ExEx");
        
        let (rag_tx, mut rag_rx) = mpsc::channel(1000);
        let (memory_tx, mut memory_rx) = mpsc::channel(1000);
        
        let rag_handle = tokio::spawn({
            let mut rag_exex = self.rag_exex;
            async move {
                if let Err(e) = rag_exex.run(rag_tx).await {
                    error!("RAG ExEx error: {}", e);
                }
            }
        });
        
        let memory_handle = tokio::spawn({
            let mut memory_exex = self.memory_exex;
            async move {
                if let Err(e) = memory_exex.run(memory_tx).await {
                    error!("Memory ExEx error: {}", e);
                }
            }
        });
        
        loop {
            tokio::select! {
                Some(event) = rag_rx.recv() => {
                    self.handle_rag_event(event).await?;
                }
                Some(event) = memory_rx.recv() => {
                    self.handle_memory_event(event).await?;
                }
                else => break,
            }
        }
        
        rag_handle.await?;
        memory_handle.await?;
        
        Ok(())
    }
    
    async fn handle_rag_event(&mut self, event: RagEvent) -> Result<()> {
        match event {
            RagEvent::ContextRetrieved { agent_id, context } => {
                info!("Retrieved context for agent {}: {} tokens", agent_id, context.len());
                self.metrics.record_rag_query();
            }
            RagEvent::EmbeddingGenerated { tx_hash, embedding_size } => {
                info!("Generated embedding for tx {}: {} dimensions", tx_hash, embedding_size);
                self.metrics.record_embedding_generated();
            }
        }
        Ok(())
    }
    
    async fn handle_memory_event(&mut self, event: MemoryEvent) -> Result<()> {
        match event {
            MemoryEvent::MemoryStored { agent_id, memory_hash } => {
                info!("Stored memory for agent {}: {}", agent_id, memory_hash);
                self.metrics.record_memory_stored();
            }
            MemoryEvent::MemoryRetrieved { agent_id, memory_size } => {
                info!("Retrieved memory for agent {}: {} bytes", agent_id, memory_size);
                self.metrics.record_memory_retrieved();
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum RagEvent {
    ContextRetrieved {
        agent_id: String,
        context: Vec<String>,
    },
    EmbeddingGenerated {
        tx_hash: String,
        embedding_size: usize,
    },
}

#[derive(Debug, Clone)]
pub enum MemoryEvent {
    MemoryStored {
        agent_id: String,
        memory_hash: String,
    },
    MemoryRetrieved {
        agent_id: String,
        memory_size: usize,
    },
}

pub async fn run<Node: FullNodeComponents>(ctx: ExExContext<Node>) -> Result<()> {
    let manager = MonmouthExExManager::new(ctx).await?;
    manager.run().await
}