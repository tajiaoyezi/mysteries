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
                content: "计划已批准,按上述 plan 逐步执行;每开始一步先 update_plan 标记 in_progress、每完成一步 update_plan 标记 done 并附 validation 自检结果".to_string(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanProgressUpdate {
    pub step: usize,
    pub status: StepStatus,
    pub validation_result: Option<String>,
}

pub trait PlanProgressReporter: Send + Sync {
    fn report(&self, update: PlanProgressUpdate);
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReportedStatus {
    InProgress,
    Done,
}

#[derive(Debug, Deserialize)]
struct UpdatePlanArgs {
    step: usize,
    status: ReportedStatus,
    validation_result: Option<String>,
}

pub struct UpdatePlanTool {
    reporter: Box<dyn PlanProgressReporter>,
}

impl UpdatePlanTool {
    pub fn new(reporter: Box<dyn PlanProgressReporter>) -> Self {
        Self { reporter }
    }
}

#[async_trait]
impl Tool for UpdatePlanTool {
    fn name(&self) -> &str {
        "update_plan"
    }

    fn description(&self) -> &str {
        "Report progress on an approved plan step"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "step": { "type": "integer" },
                "status": {
                    "type": "string",
                    "enum": ["in_progress", "done"]
                },
                "validation_result": { "type": "string" }
            },
            "required": ["step", "status"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn plan_only(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutcome {
        let parsed: UpdatePlanArgs = match serde_json::from_value(args) {
            Ok(parsed) => parsed,
            Err(err) => {
                return ToolOutcome {
                    content: format!("invalid update_plan args: {err}"),
                    is_error: true,
                    truncated: false,
                    exit: None,
                };
            }
        };

        if parsed.step == 0 {
            return ToolOutcome {
                content: "step must be >= 1".to_string(),
                is_error: true,
                truncated: false,
                exit: None,
            };
        }

        let status = match parsed.status {
            ReportedStatus::InProgress => StepStatus::InProgress,
            ReportedStatus::Done => StepStatus::Done,
        };

        self.reporter.report(PlanProgressUpdate {
            step: parsed.step,
            status,
            validation_result: parsed.validation_result,
        });

        ToolOutcome {
            content: "进度已记录".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

pub struct MockPlanProgressReporter {
    pub updates: std::sync::Arc<std::sync::Mutex<Vec<PlanProgressUpdate>>>,
}

impl MockPlanProgressReporter {
    pub fn new() -> Self {
        Self {
            updates: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl Default for MockPlanProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanProgressReporter for MockPlanProgressReporter {
    fn report(&self, update: PlanProgressUpdate) {
        self.updates.lock().unwrap().push(update);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MockPlanApprover, MockPlanProgressReporter, PlanDecision, PlanProgressUpdate, StepStatus,
        SubmitPlanTool, UpdatePlanTool,
    };
    use crate::tool::{PermissionLevel, Tool, ToolContext};
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
        assert!(outcome.content.contains("update_plan"));
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
        let outcome = tool.execute(json!({ "title": "No steps" }), &ctx()).await;

        assert!(outcome.is_error);
    }

    fn update_plan_tool() -> UpdatePlanTool {
        UpdatePlanTool::new(Box::new(MockPlanProgressReporter::new()))
    }

    fn update_plan_tool_with_reporter(
        reporter: MockPlanProgressReporter,
    ) -> (
        UpdatePlanTool,
        std::sync::Arc<std::sync::Mutex<Vec<PlanProgressUpdate>>>,
    ) {
        let updates = reporter.updates.clone();
        (UpdatePlanTool::new(Box::new(reporter)), updates)
    }

    #[tokio::test]
    async fn update_plan_done_records_progress_with_validation() {
        let (tool, updates) = update_plan_tool_with_reporter(MockPlanProgressReporter::new());
        let outcome = tool
            .execute(
                json!({
                    "step": 2,
                    "status": "done",
                    "validation_result": "cargo test → 12 passed"
                }),
                &ctx(),
            )
            .await;

        assert!(!outcome.is_error);
        let recorded = updates.lock().unwrap();
        assert_eq!(
            recorded.as_slice(),
            &[PlanProgressUpdate {
                step: 2,
                status: StepStatus::Done,
                validation_result: Some("cargo test → 12 passed".to_string()),
            }]
        );
    }

    #[tokio::test]
    async fn update_plan_in_progress_without_validation() {
        let (tool, updates) = update_plan_tool_with_reporter(MockPlanProgressReporter::new());
        let outcome = tool
            .execute(json!({ "step": 1, "status": "in_progress" }), &ctx())
            .await;

        assert!(!outcome.is_error);
        let recorded = updates.lock().unwrap();
        assert_eq!(
            recorded.as_slice(),
            &[PlanProgressUpdate {
                step: 1,
                status: StepStatus::InProgress,
                validation_result: None,
            }]
        );
    }

    #[tokio::test]
    async fn update_plan_rejects_pending_status() {
        let tool = update_plan_tool();
        let outcome = tool
            .execute(json!({ "step": 1, "status": "pending" }), &ctx())
            .await;

        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn update_plan_rejects_missing_step() {
        let tool = update_plan_tool();
        let outcome = tool.execute(json!({ "status": "done" }), &ctx()).await;

        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn update_plan_rejects_bogus_status() {
        let tool = update_plan_tool();
        let outcome = tool
            .execute(json!({ "step": 1, "status": "bogus" }), &ctx())
            .await;

        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn update_plan_rejects_step_zero() {
        let tool = update_plan_tool();
        let outcome = tool
            .execute(json!({ "step": 0, "status": "done" }), &ctx())
            .await;

        assert!(outcome.is_error);
    }

    #[test]
    fn update_plan_is_not_plan_only() {
        let tool = update_plan_tool();
        assert!(!tool.plan_only());
    }

    #[test]
    fn update_plan_is_read_only() {
        let tool = update_plan_tool();
        assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
    }
}
