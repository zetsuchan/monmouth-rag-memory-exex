//! Protocol definitions for inter-ExEx communication

use serde::{Deserialize, Serialize};

/// Protocol versions supported
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolVersion {
    /// Version 1 - Initial protocol
    V1,
}

impl Default for ProtocolVersion {
    fn default() -> Self {
        Self::V1
    }
}

/// Protocol handler for message validation and versioning
pub struct Protocol {
    version: ProtocolVersion,
}

impl Protocol {
    /// Create a new protocol handler
    pub fn new(version: ProtocolVersion) -> Self {
        Self { version }
    }

    /// Get the current protocol version
    pub fn version(&self) -> ProtocolVersion {
        self.version
    }

    /// Check if a version is supported
    pub fn is_version_supported(&self, version: ProtocolVersion) -> bool {
        match version {
            ProtocolVersion::V1 => true,
        }
    }

    /// Validate message format for the protocol version
    pub fn validate_message(&self, message: &crate::inter_exex::messages::ExExMessage) -> bool {
        match self.version {
            ProtocolVersion::V1 => {
                // V1 validation rules
                message.version == 1 && message.validate()
            }
        }
    }
}