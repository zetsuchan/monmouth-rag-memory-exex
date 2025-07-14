//! Synchronization primitives for ExEx coordination
//! 
//! This module provides synchronization mechanisms to ensure proper ordering
//! between the Memory and RAG ExEx instances, preventing race conditions
//! where RAG might process data before Memory has committed it.

use std::sync::Arc;
use tokio::sync::{Notify, RwLock, Semaphore};
use dashmap::DashMap;
use eyre::Result;
use alloy::primitives::BlockNumber;

/// Synchronization coordinator for ExEx instances
#[derive(Debug, Clone)]
pub struct ExExSyncCoordinator {
    /// Notification for memory commit completion
    memory_commit_notify: Arc<Notify>,
    
    /// Block-specific notifications for fine-grained control
    block_notifications: Arc<DashMap<BlockNumber, Arc<Notify>>>,
    
    /// Semaphore to limit concurrent processing
    processing_semaphore: Arc<Semaphore>,
    
    /// Track which blocks have been committed by memory ExEx
    committed_blocks: Arc<RwLock<Vec<BlockNumber>>>,
    
    /// Maximum number of blocks to keep in committed history
    max_committed_history: usize,
}

impl ExExSyncCoordinator {
    /// Create a new synchronization coordinator
    pub fn new(max_concurrent_processing: usize) -> Self {
        Self {
            memory_commit_notify: Arc::new(Notify::new()),
            block_notifications: Arc::new(DashMap::new()),
            processing_semaphore: Arc::new(Semaphore::new(max_concurrent_processing)),
            committed_blocks: Arc::new(RwLock::new(Vec::new())),
            max_committed_history: 1000,
        }
    }
    
    /// Signal that memory ExEx has committed a block
    pub async fn signal_memory_commit(&self, block_number: BlockNumber) -> Result<()> {
        // Add to committed blocks
        {
            let mut committed = self.committed_blocks.write().await;
            committed.push(block_number);
            
            // Maintain size limit
            if committed.len() > self.max_committed_history {
                committed.drain(0..committed.len() - self.max_committed_history);
            }
        }
        
        // Notify general waiters
        self.memory_commit_notify.notify_waiters();
        
        // Notify block-specific waiters
        if let Some((_, notify)) = self.block_notifications.remove(&block_number) {
            notify.notify_waiters();
        }
        
        Ok(())
    }
    
    /// Wait for memory ExEx to commit a specific block
    pub async fn wait_for_memory_commit(&self, block_number: BlockNumber) -> Result<()> {
        // Check if already committed
        {
            let committed = self.committed_blocks.read().await;
            if committed.contains(&block_number) {
                return Ok(());
            }
        }
        
        // Create block-specific notification
        let notify = self.block_notifications
            .entry(block_number)
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone();
        
        // Wait for notification
        notify.notified().await;
        
        Ok(())
    }
    
    /// Acquire processing permit (for rate limiting)
    pub async fn acquire_processing_permit(&self) -> Result<tokio::sync::SemaphorePermit<'_>> {
        Ok(self.processing_semaphore.acquire().await?)
    }
    
    /// Check if a block has been committed by memory ExEx
    pub async fn is_block_committed(&self, block_number: BlockNumber) -> bool {
        let committed = self.committed_blocks.read().await;
        committed.contains(&block_number)
    }
    
    /// Get the latest committed block number
    pub async fn get_latest_committed_block(&self) -> Option<BlockNumber> {
        let committed = self.committed_blocks.read().await;
        committed.last().copied()
    }
    
    /// Clear old block notifications to prevent memory leak
    pub async fn cleanup_old_notifications(&self, keep_after_block: BlockNumber) {
        self.block_notifications.retain(|&block, _| block > keep_after_block);
    }
}

/// Synchronization handle for Memory ExEx
#[derive(Debug, Clone)]
pub struct MemorySyncHandle {
    coordinator: ExExSyncCoordinator,
}

impl MemorySyncHandle {
    pub fn new(coordinator: ExExSyncCoordinator) -> Self {
        Self { coordinator }
    }
    
    /// Signal that a block has been committed
    pub async fn commit_block(&self, block_number: BlockNumber) -> Result<()> {
        self.coordinator.signal_memory_commit(block_number).await
    }
}

/// Synchronization handle for RAG ExEx
#[derive(Debug, Clone)]
pub struct RagSyncHandle {
    coordinator: ExExSyncCoordinator,
}

impl RagSyncHandle {
    pub fn new(coordinator: ExExSyncCoordinator) -> Self {
        Self { coordinator }
    }
    
    /// Wait for memory to commit before processing
    pub async fn wait_for_block(&self, block_number: BlockNumber) -> Result<()> {
        self.coordinator.wait_for_memory_commit(block_number).await
    }
    
    /// Check if safe to process a block
    pub async fn can_process_block(&self, block_number: BlockNumber) -> bool {
        self.coordinator.is_block_committed(block_number).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_sync_coordinator() {
        let coordinator = ExExSyncCoordinator::new(10);
        let memory_handle = MemorySyncHandle::new(coordinator.clone());
        let rag_handle = RagSyncHandle::new(coordinator.clone());
        
        // Test block not yet committed
        assert!(!rag_handle.can_process_block(100).await);
        
        // Spawn task to wait for block
        let rag_handle_clone = rag_handle.clone();
        let wait_task = tokio::spawn(async move {
            rag_handle_clone.wait_for_block(100).await.unwrap();
        });
        
        // Give the task time to start waiting
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        // Commit the block from memory side
        memory_handle.commit_block(100).await.unwrap();
        
        // Wait task should complete
        wait_task.await.unwrap();
        
        // Block should now be marked as committed
        assert!(rag_handle.can_process_block(100).await);
    }
}