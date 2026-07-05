use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Question {
    pub question: String,
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub allow_multi: bool,
    #[serde(default)]
    pub allow_other: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub selected: Vec<String>,
    pub supplement: Option<String>,
}

#[async_trait]
pub trait UserPrompter: Send + Sync {
    async fn prompt(&self, question: &Question) -> Answer;
}

pub struct MockPrompter {
    answer: Answer,
}

impl MockPrompter {
    pub fn new(answer: Answer) -> Self {
        Self { answer }
    }
}

#[async_trait]
impl UserPrompter for MockPrompter {
    async fn prompt(&self, _question: &Question) -> Answer {
        self.answer.clone()
    }
}

pub struct AskUserTool {
    prompter: Box<dyn UserPrompter>,
}

impl AskUserTool {
    pub fn new(prompter: Box<dyn UserPrompter>) -> Self {
        Self { prompter }
    }
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user a structured question with options"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": { "type": "string" },
                "options": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": { "type": "string" },
                            "description": { "type": "string" }
                        },
                        "required": ["label", "description"]
                    }
                },
                "allow_multi": { "type": "boolean" },
                "allow_other": { "type": "boolean" }
            },
            "required": ["question", "options"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn plan_only(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutcome {
        let question: Question = match serde_json::from_value(args) {
            Ok(question) => question,
            Err(err) => {
                return ToolOutcome {
                    content: format!("invalid question: {err}"),
                    is_error: true,
                    truncated: false,
                    exit: None,
                };
            }
        };

        let answer = self.prompter.prompt(&question).await;
        let mut content = format!("所选: {}", answer.selected.join(", "));
        if let Some(supplement) = &answer.supplement {
            content.push_str(&format!("\n补充: {supplement}"));
        }

        ToolOutcome {
            content,
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Answer, AskUserTool, MockPrompter};
    use crate::tool::{Tool, ToolContext};
    use serde_json::json;
    use std::path::PathBuf;

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("."),
            max_output_bytes: 4096,
        }
    }

    fn sample_question_args() -> serde_json::Value {
        json!({
            "question": "Which approach?",
            "options": [
                { "label": "A", "description": "Option A" },
                { "label": "B", "description": "Option B" }
            ]
        })
    }

    #[tokio::test]
    async fn ask_user_returns_selected_labels_and_supplement() {
        let tool = AskUserTool::new(Box::new(MockPrompter::new(Answer {
            selected: vec!["A".to_string()],
            supplement: Some("再考虑 X".to_string()),
        })));
        let outcome = tool.execute(sample_question_args(), &ctx()).await;

        assert!(!outcome.is_error);
        assert!(outcome.content.contains("A"));
        assert!(outcome.content.contains("再考虑 X"));
    }

    #[tokio::test]
    async fn ask_user_missing_question_returns_error() {
        let tool = AskUserTool::new(Box::new(MockPrompter::new(Answer {
            selected: vec!["A".to_string()],
            supplement: None,
        })));
        let outcome = tool
            .execute(json!({ "options": [] }), &ctx())
            .await;

        assert!(outcome.is_error);
    }
}
