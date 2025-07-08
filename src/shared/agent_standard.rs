use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStandard {
    pub version: String,
    pub capabilities: Vec<AgentCapability>,
    pub interfaces: Vec<AgentInterface>,
    pub performance_metrics: PerformanceMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentCapability {
    NaturalLanguageProcessing,
    ContextRetrieval,
    MemoryManagement,
    ToolIntegration,
    MultiModalProcessing,
    CollaborativeExecution,
    ReinforcementLearning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInterface {
    pub name: String,
    pub version: String,
    pub methods: Vec<InterfaceMethod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceMethod {
    pub name: String,
    pub inputs: Vec<ParameterSpec>,
    pub outputs: Vec<ParameterSpec>,
    pub gas_estimate: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSpec {
    pub name: String,
    pub param_type: String,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub anps_score: f64,
    pub latency_ms: f64,
    pub throughput_tps: f64,
    pub accuracy: f64,
    pub cost_per_operation: f64,
}

impl Default for AgentStandard {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            capabilities: vec![
                AgentCapability::NaturalLanguageProcessing,
                AgentCapability::ContextRetrieval,
                AgentCapability::MemoryManagement,
            ],
            interfaces: vec![
                AgentInterface {
                    name: "IAgent".to_string(),
                    version: "1.0.0".to_string(),
                    methods: vec![
                        InterfaceMethod {
                            name: "process".to_string(),
                            inputs: vec![
                                ParameterSpec {
                                    name: "input".to_string(),
                                    param_type: "bytes".to_string(),
                                    optional: false,
                                },
                            ],
                            outputs: vec![
                                ParameterSpec {
                                    name: "response".to_string(),
                                    param_type: "bytes".to_string(),
                                    optional: false,
                                },
                            ],
                            gas_estimate: 100000,
                        },
                    ],
                },
            ],
            performance_metrics: PerformanceMetrics {
                anps_score: 0.0,
                latency_ms: 0.0,
                throughput_tps: 0.0,
                accuracy: 0.0,
                cost_per_operation: 0.0,
            },
        }
    }
}

#[derive(Debug)]
pub struct AgentRegistry {
    agents: HashMap<String, RegisteredAgent>,
}

#[derive(Debug, Clone)]
pub struct RegisteredAgent {
    pub id: String,
    pub address: String,
    pub standard: AgentStandard,
    pub reputation: f64,
    pub active: bool,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }
    
    pub fn register_agent(&mut self, agent: RegisteredAgent) {
        self.agents.insert(agent.id.clone(), agent);
    }
    
    pub fn get_agent(&self, agent_id: &str) -> Option<&RegisteredAgent> {
        self.agents.get(agent_id)
    }
    
    pub fn update_reputation(&mut self, agent_id: &str, new_reputation: f64) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.reputation = new_reputation;
        }
    }
    
    pub fn find_agents_by_capability(&self, capability: &AgentCapability) -> Vec<&RegisteredAgent> {
        self.agents.values()
            .filter(|agent| agent.standard.capabilities.iter().any(|c| 
                std::mem::discriminant(c) == std::mem::discriminant(capability)
            ))
            .collect()
    }
    
    pub fn calculate_anps(&self, agent_id: &str) -> f64 {
        if let Some(agent) = self.agents.get(agent_id) {
            let metrics = &agent.standard.performance_metrics;
            
            let latency_score = 1.0 / (1.0 + metrics.latency_ms / 100.0);
            let throughput_score = metrics.throughput_tps / 1000.0;
            let accuracy_score = metrics.accuracy;
            let cost_efficiency = 1.0 / (1.0 + metrics.cost_per_operation);
            
            (latency_score * 0.3 + throughput_score * 0.3 + accuracy_score * 0.3 + cost_efficiency * 0.1) * 100.0
        } else {
            0.0
        }
    }
}