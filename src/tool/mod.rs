use crate::provider::ToolSchema;
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use thiserror::Error;

pub mod edit;
pub mod fs;
pub mod shell;
pub mod web;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;
    fn permission_level(&self) -> PermissionLevel;

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome;
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ToolRegistryError {
    #[error("duplicate tool registration: {0}")]
    Duplicate(String),
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) -> Result<(), ToolRegistryError> {
        let name = tool.name();
        if self
            .tools
            .iter()
            .any(|registered| registered.name() == name)
        {
            return Err(ToolRegistryError::Duplicate(name.to_string()));
        }

        self.tools.push(tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|tool| tool.name() == name)
            .map(|tool| tool.as_ref())
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools
            .iter()
            .map(|tool| ToolSchema {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.schema(),
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
    pub truncated: bool,
    pub exit: Option<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolContext {
    pub cwd: PathBuf,
    pub max_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionLevel {
    ReadOnly,
    Edit,
    Execute,
}

#[cfg(test)]
mod tests {
    use super::{PermissionLevel, Tool, ToolContext, ToolOutcome, ToolRegistry, ToolRegistryError};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::path::PathBuf;

    struct MockTool {
        name: &'static str,
        description: &'static str,
        permission_level: PermissionLevel,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            self.description
        }

        fn schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                },
                "required": ["input"]
            })
        }

        fn permission_level(&self) -> PermissionLevel {
            self.permission_level.clone()
        }

        async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: format!("{}:{}", self.name, args["input"].as_str().unwrap()),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
    }

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("."),
            max_output_bytes: 4096,
        }
    }

    #[tokio::test]
    async fn registry_registers_finds_and_executes_tools_by_name() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(MockTool {
                name: "read_mock",
                description: "Read mock data",
                permission_level: PermissionLevel::ReadOnly,
            }))
            .unwrap();

        let tool = registry.get("read_mock").unwrap();
        let outcome = tool.execute(json!({ "input": "abc" }), &ctx()).await;

        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
        assert_eq!(
            outcome,
            ToolOutcome {
                content: "read_mock:abc".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        );
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn registry_exposes_tool_schemas_for_model_requests() {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(MockTool {
                name: "read_mock",
                description: "Read mock data",
                permission_level: PermissionLevel::ReadOnly,
            }))
            .unwrap();
        registry
            .register(Box::new(MockTool {
                name: "write_mock",
                description: "Write mock data",
                permission_level: PermissionLevel::Edit,
            }))
            .unwrap();

        let schemas = registry.schemas();

        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0].name, "read_mock");
        assert_eq!(schemas[0].description, "Read mock data");
        assert_eq!(
            schemas[0].parameters,
            json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                },
                "required": ["input"]
            })
        );
        assert_eq!(schemas[1].name, "write_mock");
        assert_eq!(schemas[1].description, "Write mock data");
    }

    #[test]
    fn registry_rejects_duplicate_tool_name_without_overwriting_original() {
        let mut registry = ToolRegistry::new();

        let first = registry.register(Box::new(MockTool {
            name: "same",
            description: "First tool",
            permission_level: PermissionLevel::ReadOnly,
        }));
        let second = registry.register(Box::new(MockTool {
            name: "same",
            description: "Second tool",
            permission_level: PermissionLevel::Edit,
        }));

        assert_eq!(first, Ok(()));
        assert_eq!(
            second,
            Err(ToolRegistryError::Duplicate("same".to_string()))
        );
        assert_eq!(registry.get("same").unwrap().description(), "First tool");
        assert_eq!(registry.schemas().len(), 1);
    }

    #[test]
    fn registry_accepts_unique_tool_name() {
        let mut registry = ToolRegistry::new();

        let result = registry.register(Box::new(MockTool {
            name: "unique",
            description: "Unique tool",
            permission_level: PermissionLevel::ReadOnly,
        }));

        assert_eq!(result, Ok(()));
        assert!(registry.get("unique").is_some());
    }
}
