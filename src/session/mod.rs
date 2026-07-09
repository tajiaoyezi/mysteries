use crate::agent::message::Message;
use crate::tui::app::{ActivePlan, TranscriptBlock};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

const FIRST_USER_SUMMARY_CHARS: usize = 60;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub created_at: String,
    pub cwd: PathBuf,
    pub app_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub created_at: String,
    pub first_user: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SessionLine {
    Meta(SessionMeta),
    Msg(Message),
    Block(TranscriptBlock),
    Plan(ActivePlan),
}

#[derive(Clone, Debug)]
pub struct SessionStore {
    root: PathBuf,
}

struct SessionSummaryWithMtime {
    summary: SessionSummary,
    modified: SystemTime,
}

impl SessionStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn new_session_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    pub fn session_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.jsonl"))
    }

    pub fn write(
        &self,
        meta: &SessionMeta,
        history: &[Message],
        transcript: &[TranscriptBlock],
        plan: Option<&ActivePlan>,
    ) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let mut lines =
            Vec::with_capacity(1 + history.len() + transcript.len() + usize::from(plan.is_some()));
        lines.push(serialize_line(&SessionLine::Meta(meta.clone()))?);
        for message in history {
            lines.push(serialize_line(&SessionLine::Msg(message.clone()))?);
        }
        for block in transcript {
            lines.push(serialize_line(&SessionLine::Block(block.clone()))?);
        }
        if let Some(plan) = plan {
            lines.push(serialize_line(&SessionLine::Plan(plan.clone()))?);
        }

        fs::write(
            self.session_path(&meta.id),
            format!("{}\n", lines.join("\n")),
        )
    }

    // 四元组风格与既有 3 元组一致(design D3 弃具名 LoadedSession 以最小 churn);
    // clippy::type_complexity 在 4 元组临界触发,显式 allow。
    #[allow(clippy::type_complexity)]
    pub fn load(
        &self,
        id: &str,
    ) -> io::Result<(
        SessionMeta,
        Vec<Message>,
        Vec<TranscriptBlock>,
        Option<ActivePlan>,
    )> {
        let body = fs::read_to_string(self.session_path(id))?;
        let mut meta = None;
        let mut history = Vec::new();
        let mut transcript = Vec::new();
        let mut plan = None;

        for line in body.lines() {
            match parse_line(line)? {
                SessionLine::Meta(next_meta) => {
                    if meta.replace(next_meta).is_some() {
                        return Err(invalid_data("session contains more than one Meta line"));
                    }
                }
                SessionLine::Msg(message) => history.push(message),
                SessionLine::Block(block) => transcript.push(block),
                SessionLine::Plan(next_plan) => {
                    if plan.replace(next_plan).is_some() {
                        return Err(invalid_data("session contains more than one Plan line"));
                    }
                }
            }
        }

        let meta = meta.ok_or_else(|| invalid_data("session is missing Meta line"))?;
        Ok((meta, history, transcript, plan))
    }

    pub fn latest(&self) -> io::Result<Option<String>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let mut latest = None;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified()?;
            let is_newer = match latest.as_ref() {
                Some((_, latest_modified)) => modified > *latest_modified,
                None => true,
            };
            if is_newer {
                latest = Some((id.to_string(), modified));
            }
        }

        Ok(latest.map(|(id, _)| id))
    }

    pub fn list_sessions(&self) -> io::Result<Vec<SessionSummary>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        let mut summaries = Vec::new();

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified()?;
            match read_session_summary(&path) {
                Ok(summary) => summaries.push(SessionSummaryWithMtime { summary, modified }),
                Err(err) if err.kind() == io::ErrorKind::InvalidData => continue,
                Err(err) => return Err(err),
            }
        }

        summaries.sort_by_key(|entry| Reverse(entry.modified));
        Ok(summaries.into_iter().map(|entry| entry.summary).collect())
    }
}

fn read_session_summary(path: &std::path::Path) -> io::Result<SessionSummary> {
    let body = fs::read_to_string(path)?;
    let mut meta = None;
    let mut first_user = None;

    for line in body.lines() {
        match parse_line(line)? {
            SessionLine::Meta(next_meta) => {
                if meta.replace(next_meta).is_some() {
                    return Err(invalid_data("session contains more than one Meta line"));
                }
            }
            SessionLine::Msg(Message::User(text)) if first_user.is_none() => {
                first_user = Some(text.chars().take(FIRST_USER_SUMMARY_CHARS).collect());
            }
            SessionLine::Msg(_) | SessionLine::Block(_) | SessionLine::Plan(_) => {}
        }
    }

    let meta: SessionMeta = meta.ok_or_else(|| invalid_data("session is missing Meta line"))?;
    Ok(SessionSummary {
        id: meta.id,
        created_at: meta.created_at,
        first_user,
    })
}

fn serialize_line(line: &SessionLine) -> io::Result<String> {
    serde_json::to_string(line).map_err(|err| invalid_data(err.to_string()))
}

fn parse_line(line: &str) -> io::Result<SessionLine> {
    serde_json::from_str(line).map_err(|err| invalid_data(err.to_string()))
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[allow(clippy::ptr_arg)]
pub fn replace_system_head(history: &mut Vec<Message>, prompt: &str) {
    if let Some(Message::System(system)) = history.first_mut() {
        *system = prompt.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::{replace_system_head, SessionLine, SessionMeta, SessionStore, SessionSummary};
    use crate::agent::message::Message;
    use crate::provider::ToolCall;
    use crate::tool::plan::StepStatus;
    use crate::tui::app::{
        ActivePlan, ActiveStep, StatusSnapshot, ToolCard, ToolCardStatus, TranscriptBlock,
    };
    use serde_json::{json, Value};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::Duration;

    fn meta(id: &str) -> SessionMeta {
        SessionMeta {
            id: id.to_string(),
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            created_at: "2026-07-04T14:30:22Z".to_string(),
            cwd: PathBuf::from("workspace"),
            app_version: "1.1.0".to_string(),
        }
    }

    fn history() -> Vec<Message> {
        vec![
            Message::System("system prompt".to_string()),
            Message::User("run a command".to_string()),
            Message::Assistant {
                text: "I'll run it.".to_string(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "shell".to_string(),
                    arguments: json!({ "command": "echo hi" }),
                }],
                thinking: Vec::new(),
            },
            Message::ToolResult {
                call_id: "call-1".to_string(),
                content: "hi\n".to_string(),
                is_error: false,
            },
            Message::Assistant {
                text: "done".to_string(),
                tool_calls: vec![],
                thinking: Vec::new(),
            },
        ]
    }

    fn status_snapshot() -> StatusSnapshot {
        StatusSnapshot {
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            iteration: 1,
            max_iterations: 8,
            messages: 5,
            cwd: PathBuf::from("workspace"),
            tools: 7,
        }
    }

    fn tool_card(exit: Option<i32>, truncated: bool) -> ToolCard {
        ToolCard {
            id: "call-1".to_string(),
            name: "shell".to_string(),
            args: json!({ "command": "echo hi", "cwd": "workspace" }),
            readonly: false,
            status: ToolCardStatus::Error,
            output: Some("stderr".to_string()),
            truncated,
            exit,
        }
    }

    fn transcript() -> Vec<TranscriptBlock> {
        vec![
            TranscriptBlock::User("run a command".to_string()),
            TranscriptBlock::Tool(tool_card(Some(2), true)),
            TranscriptBlock::Assistant("done".to_string()),
        ]
    }

    fn active_plan(title: &str) -> ActivePlan {
        ActivePlan {
            title: title.to_string(),
            steps: vec![
                ActiveStep {
                    description: "第一步".to_string(),
                    validation: "cargo test".to_string(),
                    status: StepStatus::Done,
                    validation_result: Some("ok".to_string()),
                },
                ActiveStep {
                    description: "第二步".to_string(),
                    validation: "cargo clippy".to_string(),
                    status: StepStatus::Pending,
                    validation_result: None,
                },
            ],
        }
    }

    fn store_at(root: &Path) -> SessionStore {
        SessionStore::new(root.to_path_buf())
    }

    fn write_lines(root: &Path, id: &str, lines: &[SessionLine]) {
        fs::create_dir_all(root).expect("session root should be created");
        let store = store_at(root);
        let body = lines
            .iter()
            .map(|line| serde_json::to_string(line).expect("session line should serialize"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(store.session_path(id), format!("{body}\n"))
            .expect("session file should be written");
    }

    fn write_session(root: &Path, meta: SessionMeta, history: &[Message]) {
        store_at(root)
            .write(&meta, history, &[], None)
            .expect("session should be written");
    }

    fn single_tag(value: Value) -> String {
        let object = value.as_object().expect("session line should be object");
        assert_eq!(object.len(), 1);
        object
            .keys()
            .next()
            .expect("tag key should exist")
            .to_string()
    }

    fn looks_like_uuid_v4(id: &str) -> bool {
        let bytes = id.as_bytes();
        id.len() == 36
            && [8, 13, 18, 23].into_iter().all(|i| bytes[i] == b'-')
            && bytes[14] == b'4'
            && matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')
            && id
                .chars()
                .enumerate()
                .all(|(i, ch)| [8, 13, 18, 23].contains(&i) || ch.is_ascii_hexdigit())
    }

    #[test]
    fn session_line_uses_external_tags() {
        assert_eq!(
            single_tag(serde_json::to_value(SessionLine::Meta(meta("session-1"))).unwrap()),
            "Meta"
        );
        assert_eq!(
            single_tag(
                serde_json::to_value(SessionLine::Msg(Message::User("hi".to_string()))).unwrap()
            ),
            "Msg"
        );
        assert_eq!(
            single_tag(
                serde_json::to_value(SessionLine::Block(TranscriptBlock::Status(
                    status_snapshot()
                )))
                .unwrap()
            ),
            "Block"
        );
    }

    #[test]
    fn write_then_load_round_trips_meta_history_and_transcript() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let store = store_at(&root);
        let meta = meta("session-round-trip");
        let history = history();
        let transcript = transcript();

        store.write(&meta, &history, &transcript, None).unwrap();
        let loaded = store.load(&meta.id).unwrap();

        assert_eq!(loaded, (meta, history, transcript, None));
    }

    #[test]
    fn load_dispatches_lines_without_requiring_order() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let id = "session-interleaved";
        let meta = meta(id);
        let msg = Message::User("hi".to_string());
        let block = TranscriptBlock::Tool(tool_card(None, false));
        let plan = active_plan("交错计划");
        write_lines(
            &root,
            id,
            &[
                SessionLine::Block(block.clone()),
                SessionLine::Plan(plan.clone()),
                SessionLine::Meta(meta.clone()),
                SessionLine::Msg(msg.clone()),
            ],
        );

        let loaded = store_at(&root).load(id).unwrap();

        assert_eq!(loaded, (meta, vec![msg], vec![block], Some(plan)));
    }

    #[test]
    fn latest_returns_none_for_empty_directory() {
        let temp = tempfile::tempdir().unwrap();
        let store = store_at(&temp.path().join("sessions"));

        assert_eq!(store.latest().unwrap(), None);
    }

    #[test]
    fn latest_returns_newest_jsonl_and_ignores_other_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("older.jsonl"), "{}\n").unwrap();
        thread::sleep(Duration::from_millis(50));
        fs::write(root.join("ignored.txt"), "{}\n").unwrap();
        thread::sleep(Duration::from_millis(50));
        fs::write(root.join("newer.jsonl"), "{}\n").unwrap();
        let store = store_at(&root);

        assert_eq!(store.latest().unwrap(), Some("newer".to_string()));
    }

    #[test]
    fn list_sessions_returns_summaries_in_mtime_descending_order() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        write_session(
            &root,
            SessionMeta {
                id: "older-session".to_string(),
                created_at: "older-created".to_string(),
                ..meta("older-session")
            },
            &[Message::User("older first".to_string())],
        );
        thread::sleep(Duration::from_millis(50));
        write_session(
            &root,
            SessionMeta {
                id: "newer-session".to_string(),
                created_at: "newer-created".to_string(),
                ..meta("newer-session")
            },
            &[Message::User("newer first".to_string())],
        );

        let summaries = store_at(&root).list_sessions().unwrap();

        assert_eq!(
            summaries,
            vec![
                SessionSummary {
                    id: "newer-session".to_string(),
                    created_at: "newer-created".to_string(),
                    first_user: Some("newer first".to_string()),
                },
                SessionSummary {
                    id: "older-session".to_string(),
                    created_at: "older-created".to_string(),
                    first_user: Some("older first".to_string()),
                },
            ]
        );
    }

    #[test]
    fn list_sessions_truncates_first_user_to_sixty_chars() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        write_session(
            &root,
            meta("long-user-session"),
            &[Message::User("x".repeat(80))],
        );

        let summaries = store_at(&root).list_sessions().unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].first_user, Some("x".repeat(60)));
        assert!(summaries[0].first_user.as_ref().unwrap().chars().count() <= 60);
    }

    #[test]
    fn list_sessions_uses_none_when_session_has_no_user_message() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        write_session(
            &root,
            meta("system-only-session"),
            &[Message::System("system only".to_string())],
        );

        let summaries = store_at(&root).list_sessions().unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].first_user, None);
    }

    #[test]
    fn list_sessions_skips_damaged_files_without_failing_all() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("broken.jsonl"), "not json\n").unwrap();
        write_session(
            &root,
            meta("valid-session"),
            &[Message::User("valid first".to_string())],
        );

        let summaries = store_at(&root).list_sessions().unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "valid-session");
        assert_eq!(summaries[0].first_user, Some("valid first".to_string()));
    }

    #[test]
    fn list_sessions_returns_empty_vec_for_empty_directory() {
        let temp = tempfile::tempdir().unwrap();
        let store = store_at(&temp.path().join("sessions"));

        assert_eq!(store.list_sessions().unwrap(), Vec::<SessionSummary>::new());
    }

    #[test]
    fn list_sessions_ignores_non_jsonl_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        write_session(
            &root,
            meta("jsonl-session"),
            &[Message::User("jsonl first".to_string())],
        );
        fs::write(root.join("ignored.txt"), "not a session").unwrap();

        let summaries = store_at(&root).list_sessions().unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "jsonl-session");
    }

    #[test]
    fn new_session_id_is_uuid_v4_and_unique() {
        let first = SessionStore::new_session_id();
        let second = SessionStore::new_session_id();

        assert!(
            looks_like_uuid_v4(&first),
            "{first} should look like uuid v4"
        );
        assert!(
            looks_like_uuid_v4(&second),
            "{second} should look like uuid v4"
        );
        assert_ne!(first, second);
    }

    #[test]
    fn write_uses_meta_id_as_file_name_and_load_preserves_it() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let store = store_at(&root);
        let id = SessionStore::new_session_id();
        let meta = meta(&id);

        store.write(&meta, &history(), &transcript(), None).unwrap();

        let path = store.session_path(&id);
        assert!(path.exists(), "{} should exist", path.display());
        assert_eq!(path.file_name().unwrap(), format!("{id}.jsonl").as_str());
        let (loaded_meta, _, _, _) = store.load(&id).unwrap();
        assert_eq!(loaded_meta.id, id);
    }

    #[test]
    fn load_legacy_assistant_without_thinking_key_succeeds() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).unwrap();
        let id = "legacy-resume-session";
        let meta_line = serde_json::to_string(&SessionLine::Meta(meta(id))).unwrap();
        let legacy_msg = r#"{"Msg":{"Assistant":{"text":"done","tool_calls":[]}}}"#;
        fs::write(
            store_at(&root).session_path(id),
            format!("{meta_line}\n{legacy_msg}\n"),
        )
        .unwrap();

        let (_, history, _, _) = store_at(&root).load(id).unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0],
            Message::Assistant {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                thinking: Vec::new(),
            }
        );
    }

    #[test]
    fn load_returns_err_for_invalid_json_line() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).unwrap();
        let id = "session-invalid-json";
        fs::write(
            store_at(&root).session_path(id),
            format!(
                "{}\nnot json\n",
                serde_json::to_string(&SessionLine::Meta(meta(id))).unwrap()
            ),
        )
        .unwrap();

        assert!(store_at(&root).load(id).is_err());
    }

    #[test]
    fn load_returns_err_for_unknown_tag() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        fs::create_dir_all(&root).unwrap();
        let id = "session-unknown-tag";
        fs::write(
            store_at(&root).session_path(id),
            format!(
                "{}\n{{\"Unknown\":{{}}}}\n",
                serde_json::to_string(&SessionLine::Meta(meta(id))).unwrap()
            ),
        )
        .unwrap();

        assert!(store_at(&root).load(id).is_err());
    }

    #[test]
    fn load_returns_err_when_meta_line_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let id = "session-no-meta";
        write_lines(
            &root,
            id,
            &[SessionLine::Msg(Message::User("hi".to_string()))],
        );

        assert!(store_at(&root).load(id).is_err());
    }

    #[test]
    fn load_returns_err_when_meta_line_is_duplicated() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let id = "session-two-meta";
        write_lines(
            &root,
            id,
            &[
                SessionLine::Meta(meta(id)),
                SessionLine::Msg(Message::User("hi".to_string())),
                SessionLine::Meta(meta(id)),
            ],
        );

        assert!(store_at(&root).load(id).is_err());
    }

    #[test]
    fn second_write_rewrites_file_without_stale_messages() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let store = store_at(&root);
        let meta = meta("session-compact");
        let first_history = (0..10)
            .map(|i| Message::User(format!("before compact {i}")))
            .collect::<Vec<_>>();
        let second_history = (0..4)
            .map(|i| Message::User(format!("after compact {i}")))
            .collect::<Vec<_>>();

        store.write(&meta, &first_history, &[], None).unwrap();
        store.write(&meta, &second_history, &[], None).unwrap();

        let body = fs::read_to_string(store.session_path(&meta.id)).unwrap();
        let msg_lines = body
            .lines()
            .filter(|line| line.starts_with("{\"Msg\""))
            .count();
        assert_eq!(msg_lines, 4);
        assert!(!body.contains("before compact"));
    }

    #[test]
    fn empty_history_and_transcript_round_trip_as_meta_only() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let store = store_at(&root);
        let meta = meta("session-empty");

        store.write(&meta, &[], &[], None).unwrap();
        let (loaded_meta, loaded_history, loaded_transcript, loaded_plan) =
            store.load(&meta.id).unwrap();
        let body = fs::read_to_string(store.session_path(&meta.id)).unwrap();

        assert_eq!(loaded_meta, meta);
        assert!(loaded_history.is_empty());
        assert!(loaded_transcript.is_empty());
        assert!(loaded_plan.is_none());
        assert_eq!(body.lines().count(), 1);
        assert!(body.starts_with("{\"Meta\""));
    }

    #[test]
    fn replace_system_head_replaces_only_existing_system_head() {
        let mut history = vec![
            Message::System("old system".to_string()),
            Message::User("keep user".to_string()),
            Message::Assistant {
                text: "keep assistant".to_string(),
                tool_calls: vec![],
                thinking: Vec::new(),
            },
        ];

        replace_system_head(&mut history, "new system");

        assert_eq!(history[0], Message::System("new system".to_string()));
        assert_eq!(history[1], Message::User("keep user".to_string()));
        assert_eq!(
            history[2],
            Message::Assistant {
                text: "keep assistant".to_string(),
                tool_calls: vec![],
                thinking: Vec::new(),
            }
        );
    }

    #[test]
    fn replace_system_head_leaves_empty_history_unchanged() {
        let mut history = Vec::new();

        replace_system_head(&mut history, "new system");

        assert!(history.is_empty());
    }

    #[test]
    fn replace_system_head_leaves_non_system_head_unchanged() {
        let mut history = vec![
            Message::User("first user".to_string()),
            Message::System("not the head".to_string()),
        ];
        let original = history.clone();

        replace_system_head(&mut history, "new system");

        assert_eq!(history, original);
    }

    // --- §2.1 行为红灯（骨架期应运行期失败）---

    #[test]
    fn write_with_plan_then_load_returns_that_plan() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let store = store_at(&root);
        let meta = meta("session-with-plan");
        let plan = active_plan("计划 A");

        store
            .write(&meta, &history(), &transcript(), Some(&plan))
            .unwrap();
        let (_, _, _, loaded_plan) = store.load(&meta.id).unwrap();

        assert_eq!(loaded_plan, Some(plan));
    }

    #[test]
    fn second_write_keeps_only_latest_plan() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let store = store_at(&root);
        let meta = meta("session-plan-rewrite");
        let plan_a = active_plan("计划 A");
        let plan_b = active_plan("计划 B");

        store
            .write(&meta, &history(), &transcript(), Some(&plan_a))
            .unwrap();
        store
            .write(&meta, &history(), &transcript(), Some(&plan_b))
            .unwrap();

        let body = fs::read_to_string(store.session_path(&meta.id)).unwrap();
        let plan_lines: Vec<&str> = body
            .lines()
            .filter(|line| line.starts_with("{\"Plan\""))
            .collect();
        assert_eq!(plan_lines.len(), 1);

        let (_, _, _, loaded_plan) = store.load(&meta.id).unwrap();
        assert_eq!(loaded_plan, Some(plan_b));
    }

    #[test]
    fn load_returns_err_when_plan_line_is_duplicated() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let id = "session-two-plans";
        let plan_a = active_plan("计划 A");
        let plan_b = active_plan("计划 B");
        write_lines(
            &root,
            id,
            &[
                SessionLine::Meta(meta(id)),
                SessionLine::Plan(plan_a),
                SessionLine::Msg(Message::User("hi".to_string())),
                SessionLine::Plan(plan_b),
            ],
        );

        assert!(store_at(&root).load(id).is_err());
    }

    // --- 兼容 / 序列化 / 摘要守护（骨架期预期绿）---

    #[test]
    fn load_without_plan_line_returns_none() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let store = store_at(&root);
        let meta = meta("session-no-plan");

        store.write(&meta, &history(), &transcript(), None).unwrap();
        let (_, _, _, loaded_plan) = store.load(&meta.id).unwrap();

        assert_eq!(loaded_plan, None);
    }

    #[test]
    fn active_plan_and_step_status_serde_round_trip() {
        let plan = active_plan("serde 计划");
        let json = serde_json::to_value(&plan).unwrap();
        let restored: ActivePlan = serde_json::from_value(json).unwrap();
        assert_eq!(restored, plan);

        let status = StepStatus::InProgress;
        let status_json = serde_json::to_value(status).unwrap();
        let restored_status: StepStatus = serde_json::from_value(status_json).unwrap();
        assert_eq!(restored_status, StepStatus::InProgress);
        // 保留 Copy：赋值不 move
        let _copy = status;
        assert_eq!(status, StepStatus::InProgress);
    }

    #[test]
    fn list_sessions_ignores_plan_lines_and_keeps_first_user() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        let id = "session-list-with-plan";
        write_lines(
            &root,
            id,
            &[
                SessionLine::Meta(meta(id)),
                SessionLine::Plan(active_plan("列表计划")),
                SessionLine::Msg(Message::User("首条用户消息".to_string())),
                SessionLine::Msg(Message::Assistant {
                    text: "ok".to_string(),
                    tool_calls: vec![],
                    thinking: Vec::new(),
                }),
            ],
        );

        let summaries = store_at(&root).list_sessions().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, id);
        assert_eq!(summaries[0].first_user.as_deref(), Some("首条用户消息"));
    }
}
