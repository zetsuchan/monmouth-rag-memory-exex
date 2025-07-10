use crate::integrations::crypto::{OperatorCapability, SecurityLevel};
use crate::integrations::eigenlayer::{TaskType, MemoryOpType};
use eyre::Result;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskArchetype {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: TaskCategory,
    pub template: TaskTemplate,
    pub default_config: ArchetypeConfig,
    pub validation_rules: Vec<ValidationRule>,
    pub performance_metrics: Vec<PerformanceMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskCategory {
    Oracle,
    Computation,
    Storage,
    Coordination,
    Validation,
    MachineLearning,
    Middleware,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTemplate {
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub processing_steps: Vec<ProcessingStep>,
    pub resource_requirements: ResourceRequirements,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingStep {
    pub step_id: String,
    pub description: String,
    pub operation: StepOperation,
    pub dependencies: Vec<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepOperation {
    DataRetrieval { source: String, query: String },
    Computation { algorithm: String, parameters: HashMap<String, serde_json::Value> },
    Validation { validation_type: String, criteria: Vec<String> },
    Aggregation { method: String, inputs: Vec<String> },
    Storage { destination: String, format: String },
    Communication { target: String, message_type: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    pub cpu_cores: Option<u32>,
    pub memory_gb: Option<u32>,
    pub storage_gb: Option<u32>,
    pub gpu_required: bool,
    pub network_bandwidth_mbps: Option<u32>,
    pub execution_time_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeConfig {
    pub minimum_operators: usize,
    pub quorum_threshold: u8,
    pub minimum_stake: u64,
    pub required_capabilities: HashSet<OperatorCapability>,
    pub security_level: SecurityLevel,
    pub timeout_seconds: u64,
    pub retry_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub rule_type: ValidationType,
    pub parameters: HashMap<String, serde_json::Value>,
    pub error_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationType {
    InputFormat,
    OutputFormat,
    ResourceLimits,
    SecurityCheck,
    BusinessLogic,
    ConsensusCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetric {
    pub metric_name: String,
    pub metric_type: MetricType,
    pub target_value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricType {
    Latency,
    Throughput,
    Accuracy,
    Reliability,
    CostEfficiency,
}

pub struct TaskArchetypeManager {
    archetypes: HashMap<String, TaskArchetype>,
}

impl TaskArchetypeManager {
    pub fn new() -> Self {
        let mut manager = Self {
            archetypes: HashMap::new(),
        };
        manager.initialize_default_archetypes();
        manager
    }
    
    fn initialize_default_archetypes(&mut self) {
        // Oracle Archetype
        let oracle_archetype = TaskArchetype {
            id: "oracle_data_feed".to_string(),
            name: "Oracle Data Feed".to_string(),
            description: "Standard oracle for fetching and validating off-chain data".to_string(),
            category: TaskCategory::Oracle,
            template: TaskTemplate {
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "data_source": {"type": "string"},
                        "query": {"type": "string"},
                        "validation_criteria": {"type": "array"}
                    },
                    "required": ["data_source", "query"]
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "data": {"type": "any"},
                        "timestamp": {"type": "number"},
                        "confidence": {"type": "number"},
                        "signatures": {"type": "array"}
                    },
                    "required": ["data", "timestamp"]
                }),
                processing_steps: vec![
                    ProcessingStep {
                        step_id: "fetch_data".to_string(),
                        description: "Fetch data from external source".to_string(),
                        operation: StepOperation::DataRetrieval {
                            source: "external_api".to_string(),
                            query: "user_defined".to_string(),
                        },
                        dependencies: vec![],
                        timeout: Some(30),
                    },
                    ProcessingStep {
                        step_id: "validate_data".to_string(),
                        description: "Validate data integrity and format".to_string(),
                        operation: StepOperation::Validation {
                            validation_type: "format_check".to_string(),
                            criteria: vec!["schema_compliance".to_string(), "data_freshness".to_string()],
                        },
                        dependencies: vec!["fetch_data".to_string()],
                        timeout: Some(10),
                    },
                    ProcessingStep {
                        step_id: "aggregate_results".to_string(),
                        description: "Aggregate results from multiple operators".to_string(),
                        operation: StepOperation::Aggregation {
                            method: "median".to_string(),
                            inputs: vec!["validated_data".to_string()],
                        },
                        dependencies: vec!["validate_data".to_string()],
                        timeout: Some(15),
                    },
                ],
                resource_requirements: ResourceRequirements {
                    cpu_cores: Some(1),
                    memory_gb: Some(1),
                    storage_gb: Some(1),
                    gpu_required: false,
                    network_bandwidth_mbps: Some(10),
                    execution_time_seconds: Some(60),
                },
            },
            default_config: ArchetypeConfig {
                minimum_operators: 3,
                quorum_threshold: 67,
                minimum_stake: 5_000_000_000_000_000_000, // 5 ETH
                required_capabilities: {
                    let mut caps = HashSet::new();
                    caps.insert(OperatorCapability::BLSEnabled);
                    caps
                },
                security_level: SecurityLevel::Medium,
                timeout_seconds: 300,
                retry_attempts: 3,
            },
            validation_rules: vec![
                ValidationRule {
                    rule_type: ValidationType::InputFormat,
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert("schema".to_string(), serde_json::json!("input_schema"));
                        params
                    },
                    error_message: "Input does not match required schema".to_string(),
                },
            ],
            performance_metrics: vec![
                PerformanceMetric {
                    metric_name: "data_freshness".to_string(),
                    metric_type: MetricType::Latency,
                    target_value: 5.0,
                    unit: "seconds".to_string(),
                },
                PerformanceMetric {
                    metric_name: "accuracy".to_string(),
                    metric_type: MetricType::Accuracy,
                    target_value: 99.5,
                    unit: "percentage".to_string(),
                },
            ],
        };
        
        // ML Inference Archetype
        let ml_inference_archetype = TaskArchetype {
            id: "ml_inference".to_string(),
            name: "Machine Learning Inference".to_string(),
            description: "Distributed ML model inference with consensus".to_string(),
            category: TaskCategory::MachineLearning,
            template: TaskTemplate {
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "model_id": {"type": "string"},
                        "input_data": {"type": "array"},
                        "inference_parameters": {"type": "object"}
                    },
                    "required": ["model_id", "input_data"]
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "predictions": {"type": "array"},
                        "confidence_scores": {"type": "array"},
                        "model_version": {"type": "string"},
                        "execution_time": {"type": "number"}
                    },
                    "required": ["predictions", "confidence_scores"]
                }),
                processing_steps: vec![
                    ProcessingStep {
                        step_id: "load_model".to_string(),
                        description: "Load ML model from storage".to_string(),
                        operation: StepOperation::DataRetrieval {
                            source: "model_registry".to_string(),
                            query: "model_id".to_string(),
                        },
                        dependencies: vec![],
                        timeout: Some(60),
                    },
                    ProcessingStep {
                        step_id: "preprocess_input".to_string(),
                        description: "Preprocess input data for inference".to_string(),
                        operation: StepOperation::Computation {
                            algorithm: "data_preprocessing".to_string(),
                            parameters: HashMap::new(),
                        },
                        dependencies: vec!["load_model".to_string()],
                        timeout: Some(30),
                    },
                    ProcessingStep {
                        step_id: "run_inference".to_string(),
                        description: "Execute ML model inference".to_string(),
                        operation: StepOperation::Computation {
                            algorithm: "model_inference".to_string(),
                            parameters: HashMap::new(),
                        },
                        dependencies: vec!["preprocess_input".to_string()],
                        timeout: Some(120),
                    },
                ],
                resource_requirements: ResourceRequirements {
                    cpu_cores: Some(4),
                    memory_gb: Some(8),
                    storage_gb: Some(10),
                    gpu_required: true,
                    network_bandwidth_mbps: Some(100),
                    execution_time_seconds: Some(300),
                },
            },
            default_config: ArchetypeConfig {
                minimum_operators: 3,
                quorum_threshold: 67,
                minimum_stake: 10_000_000_000_000_000_000, // 10 ETH
                required_capabilities: {
                    let mut caps = HashSet::new();
                    caps.insert(OperatorCapability::GPUCompute);
                    caps.insert(OperatorCapability::BLSEnabled);
                    caps.insert(OperatorCapability::HighMemory);
                    caps
                },
                security_level: SecurityLevel::High,
                timeout_seconds: 600,
                retry_attempts: 2,
            },
            validation_rules: vec![
                ValidationRule {
                    rule_type: ValidationType::ResourceLimits,
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert("max_execution_time".to_string(), serde_json::json!(300));
                        params
                    },
                    error_message: "Inference execution exceeded time limit".to_string(),
                },
            ],
            performance_metrics: vec![
                PerformanceMetric {
                    metric_name: "inference_latency".to_string(),
                    metric_type: MetricType::Latency,
                    target_value: 100.0,
                    unit: "milliseconds".to_string(),
                },
                PerformanceMetric {
                    metric_name: "throughput".to_string(),
                    metric_type: MetricType::Throughput,
                    target_value: 1000.0,
                    unit: "inferences_per_second".to_string(),
                },
            ],
        };
        
        // Data Availability Archetype
        let data_availability_archetype = TaskArchetype {
            id: "data_availability".to_string(),
            name: "Data Availability Service".to_string(),
            description: "Decentralized data availability and storage".to_string(),
            category: TaskCategory::Storage,
            template: TaskTemplate {
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "data": {"type": "string"},
                        "redundancy_level": {"type": "number"},
                        "retention_period": {"type": "number"}
                    },
                    "required": ["data"]
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "data_hash": {"type": "string"},
                        "storage_proofs": {"type": "array"},
                        "availability_commitment": {"type": "string"}
                    },
                    "required": ["data_hash", "storage_proofs"]
                }),
                processing_steps: vec![
                    ProcessingStep {
                        step_id: "erasure_encode".to_string(),
                        description: "Apply erasure coding to data".to_string(),
                        operation: StepOperation::Computation {
                            algorithm: "reed_solomon".to_string(),
                            parameters: HashMap::new(),
                        },
                        dependencies: vec![],
                        timeout: Some(60),
                    },
                    ProcessingStep {
                        step_id: "distribute_data".to_string(),
                        description: "Distribute data chunks to operators".to_string(),
                        operation: StepOperation::Storage {
                            destination: "distributed_storage".to_string(),
                            format: "encoded_chunks".to_string(),
                        },
                        dependencies: vec!["erasure_encode".to_string()],
                        timeout: Some(120),
                    },
                    ProcessingStep {
                        step_id: "generate_proofs".to_string(),
                        description: "Generate storage and availability proofs".to_string(),
                        operation: StepOperation::Validation {
                            validation_type: "storage_proof".to_string(),
                            criteria: vec!["data_integrity".to_string(), "availability".to_string()],
                        },
                        dependencies: vec!["distribute_data".to_string()],
                        timeout: Some(30),
                    },
                ],
                resource_requirements: ResourceRequirements {
                    cpu_cores: Some(2),
                    memory_gb: Some(4),
                    storage_gb: Some(100),
                    gpu_required: false,
                    network_bandwidth_mbps: Some(1000),
                    execution_time_seconds: Some(300),
                },
            },
            default_config: ArchetypeConfig {
                minimum_operators: 5,
                quorum_threshold: 75,
                minimum_stake: 15_000_000_000_000_000_000, // 15 ETH
                required_capabilities: {
                    let mut caps = HashSet::new();
                    caps.insert(OperatorCapability::BLSEnabled);
                    caps
                },
                security_level: SecurityLevel::High,
                timeout_seconds: 600,
                retry_attempts: 3,
            },
            validation_rules: vec![
                ValidationRule {
                    rule_type: ValidationType::SecurityCheck,
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert("encryption_required".to_string(), serde_json::json!(true));
                        params
                    },
                    error_message: "Data must be encrypted before storage".to_string(),
                },
            ],
            performance_metrics: vec![
                PerformanceMetric {
                    metric_name: "availability".to_string(),
                    metric_type: MetricType::Reliability,
                    target_value: 99.9,
                    unit: "percentage".to_string(),
                },
                PerformanceMetric {
                    metric_name: "retrieval_time".to_string(),
                    metric_type: MetricType::Latency,
                    target_value: 1.0,
                    unit: "seconds".to_string(),
                },
            ],
        };
        
        self.archetypes.insert(oracle_archetype.id.clone(), oracle_archetype);
        self.archetypes.insert(ml_inference_archetype.id.clone(), ml_inference_archetype);
        self.archetypes.insert(data_availability_archetype.id.clone(), data_availability_archetype);
    }
    
    pub fn get_archetype(&self, archetype_id: &str) -> Option<&TaskArchetype> {
        self.archetypes.get(archetype_id)
    }
    
    pub fn list_archetypes(&self) -> Vec<&TaskArchetype> {
        self.archetypes.values().collect()
    }
    
    pub fn get_archetypes_by_category(&self, category: &TaskCategory) -> Vec<&TaskArchetype> {
        self.archetypes
            .values()
            .filter(|archetype| std::mem::discriminant(&archetype.category) == std::mem::discriminant(category))
            .collect()
    }
    
    pub fn create_task_from_archetype(
        &self,
        archetype_id: &str,
        input_data: serde_json::Value,
        custom_config: Option<ArchetypeConfig>,
    ) -> Result<TaskType> {
        let archetype = self.archetypes.get(archetype_id)
            .ok_or_else(|| eyre::eyre!("Archetype {} not found", archetype_id))?;
        
        // Validate input against schema
        self.validate_input(&archetype.template.input_schema, &input_data)?;
        
        // Create task based on archetype category
        let task_type = match archetype.category {
            TaskCategory::Oracle => {
                TaskType::ValidationRequest {
                    data_hash: [0u8; 32], // Would be computed from input
                    validation_type: "oracle_data".to_string(),
                }
            }
            TaskCategory::MachineLearning => {
                TaskType::AgentCoordination {
                    agent_ids: vec!["ml_inference_agent".to_string()],
                    coordination_type: "ml_inference".to_string(),
                }
            }
            TaskCategory::Storage => {
                TaskType::MemoryOperation {
                    operation: MemoryOpType::Store,
                    memory_hash: [0u8; 32], // Would be computed from input
                }
            }
            _ => {
                TaskType::ValidationRequest {
                    data_hash: [0u8; 32],
                    validation_type: archetype.name.clone(),
                }
            }
        };
        
        Ok(task_type)
    }
    
    fn validate_input(&self, schema: &serde_json::Value, input: &serde_json::Value) -> Result<()> {
        // Basic schema validation (in production, use a proper JSON schema validator)
        if let Some(required) = schema.get("required") {
            if let Some(required_array) = required.as_array() {
                for required_field in required_array {
                    if let Some(field_name) = required_field.as_str() {
                        if !input.get(field_name).is_some() {
                            return Err(eyre::eyre!("Required field '{}' missing", field_name));
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    pub fn register_custom_archetype(&mut self, archetype: TaskArchetype) -> Result<()> {
        if self.archetypes.contains_key(&archetype.id) {
            return Err(eyre::eyre!("Archetype with ID '{}' already exists", archetype.id));
        }
        
        self.archetypes.insert(archetype.id.clone(), archetype);
        Ok(())
    }
    
    pub fn get_archetype_config(&self, archetype_id: &str) -> Option<&ArchetypeConfig> {
        self.archetypes.get(archetype_id).map(|a| &a.default_config)
    }
    
    pub fn estimate_execution_cost(
        &self,
        archetype_id: &str,
        operator_count: usize,
        execution_time: u64,
    ) -> Result<u64> {
        let archetype = self.archetypes.get(archetype_id)
            .ok_or_else(|| eyre::eyre!("Archetype {} not found", archetype_id))?;
        
        // Base cost calculation (simplified)
        let base_cost = archetype.default_config.minimum_stake / 1000; // Small fraction of stake
        let time_factor = execution_time / 60; // Per minute
        let operator_factor = operator_count as u64;
        
        let total_cost = base_cost * time_factor * operator_factor;
        
        Ok(total_cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_archetype_manager() {
        let manager = TaskArchetypeManager::new();
        
        // Test getting archetype
        let oracle_archetype = manager.get_archetype("oracle_data_feed");
        assert!(oracle_archetype.is_some());
        
        // Test listing archetypes
        let archetypes = manager.list_archetypes();
        assert_eq!(archetypes.len(), 3);
        
        // Test getting by category
        let oracle_archetypes = manager.get_archetypes_by_category(&TaskCategory::Oracle);
        assert_eq!(oracle_archetypes.len(), 1);
        
        // Test creating task from archetype
        let input_data = serde_json::json!({
            "data_source": "https://api.example.com",
            "query": "price/ETH/USD"
        });
        
        let task = manager.create_task_from_archetype("oracle_data_feed", input_data, None);
        assert!(task.is_ok());
    }
    
    #[test]
    fn test_cost_estimation() {
        let manager = TaskArchetypeManager::new();
        
        let cost = manager.estimate_execution_cost("ml_inference", 5, 300);
        assert!(cost.is_ok());
        assert!(cost.unwrap() > 0);
    }
}