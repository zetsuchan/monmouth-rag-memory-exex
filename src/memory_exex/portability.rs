use crate::memory_exex::{Memory, ExportFormat};
use eyre::Result;
use serde::{Serialize, Deserialize};
use std::io::Write;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPackage {
    pub agent_id: String,
    pub export_timestamp: std::time::SystemTime,
    pub format_version: String,
    pub memory_count: usize,
    pub memories: Vec<PortableMemory>,
    pub metadata: PackageMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableMemory {
    pub id: String,
    pub memory_type: String,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub timestamp: u64,
    pub importance: f64,
    pub access_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub source_chain: String,
    pub export_reason: Option<String>,
    pub compression: Option<String>,
    pub encryption: Option<String>,
}

#[derive(Debug)]
pub struct MemoryPortability;

impl MemoryPortability {
    pub fn new() -> Self {
        Self
    }
    
    pub async fn export_memories(
        &self,
        agent_id: &str,
        memories: Vec<Memory>,
        format: ExportFormat,
    ) -> Result<MemoryPackage> {
        let portable_memories: Vec<PortableMemory> = memories.into_iter()
            .map(|m| PortableMemory {
                id: m.id,
                memory_type: format!("{:?}", m.memory_type),
                content: base64::encode(&m.content),
                embedding: m.embedding,
                timestamp: m.timestamp.elapsed().as_secs(),
                importance: m.importance,
                access_count: m.access_count,
            })
            .collect();
        
        let package = MemoryPackage {
            agent_id: agent_id.to_string(),
            export_timestamp: std::time::SystemTime::now(),
            format_version: "1.0.0".to_string(),
            memory_count: portable_memories.len(),
            memories: portable_memories,
            metadata: PackageMetadata {
                source_chain: "monmouth".to_string(),
                export_reason: None,
                compression: match format {
                    ExportFormat::Compressed => Some("gzip".to_string()),
                    _ => None,
                },
                encryption: None,
            },
        };
        
        Ok(package)
    }
    
    pub async fn import_package(&self, package: MemoryPackage) -> Result<Vec<Memory>> {
        let mut memories = Vec::new();
        
        for portable in package.memories {
            let memory = Memory {
                id: portable.id,
                agent_id: package.agent_id.clone(),
                memory_type: self.parse_memory_type(&portable.memory_type),
                content: base64::decode(&portable.content)?,
                embedding: portable.embedding,
                timestamp: std::time::Instant::now() - std::time::Duration::from_secs(portable.timestamp),
                importance: portable.importance,
                access_count: portable.access_count,
            };
            memories.push(memory);
        }
        
        Ok(memories)
    }
    
    pub async fn export_to_file(
        &self,
        package: &MemoryPackage,
        path: &str,
        format: ExportFormat,
    ) -> Result<()> {
        match format {
            ExportFormat::Json => {
                let json = serde_json::to_string_pretty(package)?;
                std::fs::write(path, json)?;
            }
            ExportFormat::Binary => {
                let binary = bincode::serialize(package)?;
                std::fs::write(path, binary)?;
            }
            ExportFormat::Compressed => {
                let json = serde_json::to_string(package)?;
                let compressed = self.compress_data(json.as_bytes())?;
                std::fs::write(path, compressed)?;
            }
        }
        
        Ok(())
    }
    
    pub async fn import_from_file(&self, path: &str) -> Result<MemoryPackage> {
        let data = std::fs::read(path)?;
        
        if let Ok(package) = serde_json::from_slice::<MemoryPackage>(&data) {
            return Ok(package);
        }
        
        if let Ok(package) = bincode::deserialize::<MemoryPackage>(&data) {
            return Ok(package);
        }
        
        let decompressed = self.decompress_data(&data)?;
        let package = serde_json::from_slice::<MemoryPackage>(&decompressed)?;
        
        Ok(package)
    }
    
    pub async fn validate_package(&self, package: &MemoryPackage) -> Result<()> {
        if package.format_version != "1.0.0" {
            return Err(eyre::eyre!("Unsupported format version"));
        }
        
        if package.memory_count != package.memories.len() {
            return Err(eyre::eyre!("Memory count mismatch"));
        }
        
        for memory in &package.memories {
            base64::decode(&memory.content)?;
        }
        
        Ok(())
    }
    
    fn parse_memory_type(&self, type_str: &str) -> crate::memory_exex::MemoryType {
        match type_str {
            "ShortTerm" => crate::memory_exex::MemoryType::ShortTerm,
            "LongTerm" => crate::memory_exex::MemoryType::LongTerm,
            "Working" => crate::memory_exex::MemoryType::Working,
            "Episodic" => crate::memory_exex::MemoryType::Episodic,
            "Semantic" => crate::memory_exex::MemoryType::Semantic,
            _ => crate::memory_exex::MemoryType::LongTerm,
        }
    }
    
    fn compress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data)?;
        Ok(encoder.finish()?)
    }
    
    fn decompress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        use flate2::read::GzDecoder;
        use std::io::Read;
        
        let mut decoder = GzDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }
}