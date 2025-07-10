pub mod svm_integration;

pub use svm_integration::{SVMGraphIntegration, RelationshipUpdate};

// Main knowledge graph type
pub type KnowledgeGraph = SVMGraphIntegration;