use crate::integrations::crypto::bls::{
    BlsKeyPair, BlsPublicKey, BlsSignature, AggregatedSignature, BlsAggregator,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio::time::{Duration, timeout};
use eyre::{Result, eyre};
use tracing::{info, warn, error};

#[derive(Debug, Clone)]
pub struct OperatorResponse {
    pub task_id: String,
    pub operator_address: String,
    pub signature: BlsSignature,
    pub public_key: BlsPublicKey,
    pub stake: u64,
    pub message: Vec<u8>,
}

#[derive(Debug)]
pub struct AggregationRequest {
    pub task_id: String,
    pub message: Vec<u8>,
    pub quorum_threshold: f64,
    pub timeout_duration: Duration,
    pub response_channel: oneshot::Sender<Result<AggregatedSignature>>,
}

#[derive(Debug, Clone)]
struct TaskAggregation {
    message: Vec<u8>,
    responses: Vec<(BlsSignature, BlsPublicKey, u64)>,
    total_stake: u64,
    required_stake: u64,
    start_time: std::time::Instant,
    timeout: Duration,
}

pub struct BlsAggregationService {
    tasks: Arc<RwLock<HashMap<String, TaskAggregation>>>,
    response_receiver: mpsc::Receiver<OperatorResponse>,
    response_sender: mpsc::Sender<OperatorResponse>,
    request_receiver: mpsc::Receiver<AggregationRequest>,
    request_sender: mpsc::Sender<AggregationRequest>,
}

impl BlsAggregationService {
    pub fn new() -> (Self, mpsc::Sender<OperatorResponse>, mpsc::Sender<AggregationRequest>) {
        let (response_tx, response_rx) = mpsc::channel(1000);
        let (request_tx, request_rx) = mpsc::channel(100);
        
        let service = BlsAggregationService {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            response_receiver: response_rx,
            response_sender: response_tx.clone(),
            request_receiver: request_rx,
            request_sender: request_tx.clone(),
        };
        
        (service, response_tx, request_tx)
    }
    
    pub async fn run(mut self) {
        info!("Starting BLS aggregation service");
        
        let tasks_clone = self.tasks.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Self::cleanup_expired_tasks(tasks_clone.clone()).await;
            }
        });
        
        loop {
            tokio::select! {
                Some(response) = self.response_receiver.recv() => {
                    if let Err(e) = self.handle_operator_response(response).await {
                        error!("Error handling operator response: {:?}", e);
                    }
                }
                Some(request) = self.request_receiver.recv() => {
                    if let Err(e) = self.handle_aggregation_request(request).await {
                        error!("Error handling aggregation request: {:?}", e);
                    }
                }
            }
        }
    }
    
    async fn handle_operator_response(&self, response: OperatorResponse) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        
        if let Some(task_agg) = tasks.get_mut(&response.task_id) {
            if response.message != task_agg.message {
                warn!("Message mismatch for task {}", response.task_id);
                return Ok(());
            }
            
            let is_valid = BlsKeyPair::verify_signature(
                &response.public_key,
                &response.message,
                &response.signature,
            )?;
            
            if !is_valid {
                warn!("Invalid signature from operator {}", response.operator_address);
                return Ok(());
            }
            
            let already_signed = task_agg.responses.iter().any(|(_, pk, _)| pk == &response.public_key);
            if already_signed {
                warn!("Operator {} already signed task {}", response.operator_address, response.task_id);
                return Ok(());
            }
            
            task_agg.responses.push((
                response.signature,
                response.public_key,
                response.stake,
            ));
            task_agg.total_stake += response.stake;
            
            info!(
                "Added signature for task {}: {}/{} stake",
                response.task_id,
                task_agg.total_stake,
                task_agg.required_stake
            );
            
            if task_agg.total_stake >= task_agg.required_stake {
                info!("Threshold reached for task {}, aggregating signatures", response.task_id);
                
                let task_agg_clone = task_agg.clone();
                tasks.remove(&response.task_id);
                drop(tasks);
                
                self.aggregate_and_notify(response.task_id, task_agg_clone).await?;
            }
        } else {
            warn!("Received response for unknown task: {}", response.task_id);
        }
        
        Ok(())
    }
    
    async fn handle_aggregation_request(&self, request: AggregationRequest) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        
        if tasks.contains_key(&request.task_id) {
            let _ = request.response_channel.send(Err(eyre!("Task already exists")));
            return Ok(());
        }
        
        let total_operators_stake = self.get_total_operators_stake().await;
        let required_stake = (total_operators_stake as f64 * request.quorum_threshold) as u64;
        
        let task_agg = TaskAggregation {
            message: request.message.clone(),
            responses: Vec::new(),
            total_stake: 0,
            required_stake,
            start_time: std::time::Instant::now(),
            timeout: request.timeout_duration,
        };
        
        tasks.insert(request.task_id.clone(), task_agg);
        
        let task_id = request.task_id.clone();
        let tasks_clone = self.tasks.clone();
        let timeout_duration = request.timeout_duration;
        
        tokio::spawn(async move {
            tokio::time::sleep(timeout_duration).await;
            
            let mut tasks = tasks_clone.write().await;
            if let Some(task_agg) = tasks.remove(&task_id) {
                warn!("Task {} timed out with {}/{} stake", 
                    task_id, task_agg.total_stake, task_agg.required_stake);
                
                if task_agg.responses.is_empty() {
                    let _ = request.response_channel.send(Err(eyre!("No responses received")));
                } else {
                    match BlsAggregator::aggregate_signatures(task_agg.responses) {
                        Ok(aggregated) => {
                            let _ = request.response_channel.send(Ok(aggregated));
                        }
                        Err(e) => {
                            let _ = request.response_channel.send(Err(e));
                        }
                    }
                }
            }
        });
        
        Ok(())
    }
    
    async fn aggregate_and_notify(
        &self,
        task_id: String,
        task_agg: TaskAggregation,
    ) -> Result<()> {
        match BlsAggregator::aggregate_signatures(task_agg.responses) {
            Ok(aggregated) => {
                info!("Successfully aggregated signatures for task {}", task_id);
                
                let is_valid = BlsAggregator::verify_aggregated_signature(
                    &aggregated,
                    &task_agg.message,
                )?;
                
                if !is_valid {
                    error!("Aggregated signature verification failed for task {}", task_id);
                }
                
                Ok(())
            }
            Err(e) => {
                error!("Failed to aggregate signatures for task {}: {:?}", task_id, e);
                Err(e)
            }
        }
    }
    
    async fn cleanup_expired_tasks(tasks: Arc<RwLock<HashMap<String, TaskAggregation>>>) {
        let mut tasks = tasks.write().await;
        let expired_tasks: Vec<String> = tasks
            .iter()
            .filter(|(_, task)| task.start_time.elapsed() > task.timeout)
            .map(|(id, _)| id.clone())
            .collect();
        
        for task_id in expired_tasks {
            info!("Removing expired task: {}", task_id);
            tasks.remove(&task_id);
        }
    }
    
    async fn get_total_operators_stake(&self) -> u64 {
        1_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_aggregation_service() {
        let (service, response_tx, request_tx) = BlsAggregationService::new();
        
        tokio::spawn(async move {
            service.run().await;
        });
        
        let message = b"test message";
        let (result_tx, result_rx) = oneshot::channel();
        
        let request = AggregationRequest {
            task_id: "test-task".to_string(),
            message: message.to_vec(),
            quorum_threshold: 0.66,
            timeout_duration: Duration::from_secs(10),
            response_channel: result_tx,
        };
        
        request_tx.send(request).await.unwrap();
        
        for i in 0..3 {
            let keypair = BlsKeyPair::generate().unwrap();
            let signature = keypair.sign_message(message).unwrap();
            
            let response = OperatorResponse {
                task_id: "test-task".to_string(),
                operator_address: format!("operator-{}", i),
                signature,
                public_key: keypair.public_key,
                stake: 400_000,
                message: message.to_vec(),
            };
            
            response_tx.send(response).await.unwrap();
        }
        
        match timeout(Duration::from_secs(5), result_rx).await {
            Ok(Ok(Ok(aggregated))) => {
                assert_eq!(aggregated.signers.len(), 3);
                assert_eq!(aggregated.total_stake, 1_200_000);
                
                let is_valid = BlsAggregator::verify_aggregated_signature(&aggregated, message).unwrap();
                assert!(is_valid);
            }
            _ => panic!("Failed to receive aggregation result"),
        }
    }
}