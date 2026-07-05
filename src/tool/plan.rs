use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Plan {
    pub title: String,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlanStep {
    pub description: String,
    pub validation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanDecision {
    Approve,
    Reject(String),
}

#[async_trait]
pub trait PlanApprover: Send + Sync {
    async fn approve(&self, plan: &Plan) -> PlanDecision;
}

pub struct MockPlanApprover {
    decision: PlanDecision,
}

impl MockPlanApprover {
    pub fn new(decision: PlanDecision) -> Self {
        Self { decision }
    }
}

#[async_trait]
impl PlanApprover for MockPlanApprover {
    async fn approve(&self, _plan: &Plan) -> PlanDecision {
        self.decision.clone()
    }
}

pub struct SubmitPlanTool {
    approver: Box<dyn PlanApprover>,
}

impl SubmitPlanTool {
    pub fn new(approver: Box<dyn PlanApprover>) -> Self {
        Self { approver }
    }
}

#[async_trait]
impl Tool for SubmitPlanTool {
    fn name(&self) -> &str {
        "submit_plan"
    }

    fn description(&self) -> &str {
        "Submit a structured plan for user approval"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string" },
                            "validation": { "type": "string" }
                        },
                        "required": ["description", "validation"]
                    }
                }
            },
            "required": ["title", "steps"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn plan_only(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutcome {
        let plan: Plan = match serde_json::from_value(args) {
            Ok(plan) => plan,
            Err(err) => {
                return ToolOutcome {
                    content: format!("invalid plan: {err}"),
                    is_error: true,
                    truncated: false,
                    exit: None,
                };
            }
        };

        match self.approver.approve(&plan).await {
            PlanDecision::Approve => ToolOutcome {
                content: "计划已批准,按上述 plan 逐步执行、每步完成后自检其 validation".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            },
            PlanDecision::Reject(reason) => ToolOutcome {
                content: format!("计划被驳回:{reason};请修改"),
                is_error: true,
                truncated: false,
                exit: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MockPlanApprover, PlanDecision, SubmitPlanTool,
    };
    use crate::tool::{Tool, ToolContext};
    use serde_json::json;
    use std::path::PathBuf;

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("."),
            max_output_bytes: 4096,
        }
    }

    fn sample_plan_args() -> serde_json::Value {
        json!({
            "title": "Add plan mode",
            "steps": [
                {
                    "description": "Wire permission gate",
                    "validation": "cargo test permission passes"
                }
            ]
        })
    }

    #[tokio::test]
    async fn submit_plan_approve_returns_success() {
        let tool = SubmitPlanTool::new(Box::new(MockPlanApprover::new(PlanDecision::Approve)));
        let outcome = tool.execute(sample_plan_args(), &ctx()).await;

        assert!(!outcome.is_error);
        assert!(outcome.content.contains("计划已批准"));
    }

    #[tokio::test]
    async fn submit_plan_reject_returns_error_with_reason() {
        let tool = SubmitPlanTool::new(Box::new(MockPlanApprover::new(PlanDecision::Reject(
            "先补测试".to_string(),
        ))));
        let outcome = tool.execute(sample_plan_args(), &ctx()).await;

        assert!(outcome.is_error);
        assert!(outcome.content.contains("先补测试"));
    }

    #[tokio::test]
    async fn submit_plan_missing_steps_returns_error() {
        let tool = SubmitPlanTool::new(Box::new(MockPlanApprover::new(PlanDecision::Approve)));
        let outcome = tool
            .execute(json!({ "title": "No steps" }), &ctx())
            .await;

        assert!(outcome.is_error);
    }
}
