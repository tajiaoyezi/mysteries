use crate::provider::Provider;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub(crate) struct RuntimeSnapshot {
    pub(crate) provider: Arc<dyn Provider>,
    pub(crate) model: String,
}

#[derive(Clone)]
pub(crate) struct AgentRuntime {
    state: Arc<RwLock<RuntimeSnapshot>>,
}

impl AgentRuntime {
    pub(crate) fn new(provider: Arc<dyn Provider>, model: String) -> Self {
        Self {
            state: Arc::new(RwLock::new(RuntimeSnapshot { provider, model })),
        }
    }

    pub(crate) fn snapshot(&self) -> RuntimeSnapshot {
        self.state
            .read()
            .expect("agent runtime lock poisoned")
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    pub(crate) fn set_provider(&self, provider: Arc<dyn Provider>) {
        self.state
            .write()
            .expect("agent runtime lock poisoned")
            .provider = provider;
    }

    pub(crate) fn set_model(&self, model: String) {
        self.state
            .write()
            .expect("agent runtime lock poisoned")
            .model = model;
    }

    pub(crate) fn replace_provider_model(&self, provider: Arc<dyn Provider>, model: String) {
        self.replace_provider_model_inner(provider, model, || {});
    }

    #[cfg(test)]
    pub(crate) fn replace_provider_model_with_hook<F>(
        &self,
        provider: Arc<dyn Provider>,
        model: String,
        before_commit: F,
    ) where
        F: FnOnce(),
    {
        self.replace_provider_model_inner(provider, model, before_commit);
    }

    fn replace_provider_model_inner<F>(
        &self,
        provider: Arc<dyn Provider>,
        model: String,
        before_commit: F,
    ) where
        F: FnOnce(),
    {
        let mut snapshot = self.state.write().expect("agent runtime lock poisoned");
        before_commit();
        *snapshot = RuntimeSnapshot { provider, model };
    }
}

#[cfg(test)]
mod tests {
    use super::AgentRuntime;
    use crate::agent::message::Message;
    use crate::agent::{Agent, ContextError, ContextStrategy};
    use crate::error::ProviderError;
    use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision};
    use crate::provider::mock::MockProvider;
    use crate::provider::{
        DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider, ThinkingBlock,
    };
    use crate::tool::delegate::DelegateTaskTool;
    use crate::tool::{ToolContext, ToolRegistry};
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::{mpsc, Arc, Mutex, TryLockError};
    use std::time::Duration;
    use tokio::sync::Notify;

    struct AllowAll;

    #[async_trait]
    impl PermissionDecider for AllowAll {
        async fn decide(&self, _check: PermissionCheck<'_>) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    struct NoopSink;

    impl DeltaSink for NoopSink {
        fn on_text(&self, _text: &str) {}
    }

    struct StalledProvider {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[derive(Default)]
    struct RecordingStrategy {
        providers: Arc<Mutex<Vec<String>>>,
        models: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ContextStrategy for RecordingStrategy {
        async fn prepare(
            &self,
            history: &[Message],
            _last_usage: Option<&crate::provider::Usage>,
        ) -> Result<Vec<Message>, ContextError> {
            Ok(history.to_vec())
        }

        fn set_provider(&mut self, provider: Arc<dyn Provider>) {
            self.providers
                .lock()
                .unwrap()
                .push(provider.name().to_string());
        }

        fn set_model(&mut self, model: String) {
            self.models.lock().unwrap().push(model);
        }
    }

    #[async_trait]
    impl Provider for StalledProvider {
        fn name(&self) -> &str {
            "stalled"
        }

        async fn complete(
            &self,
            _req: ModelRequest,
            _sink: &dyn DeltaSink,
        ) -> Result<ModelResponse, ProviderError> {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(ModelResponse {
                text: "released".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
                thinking: Vec::new(),
            })
        }
    }

    fn provider() -> Arc<dyn Provider> {
        Arc::new(MockProvider::new(Vec::new()))
    }

    fn agent(provider: Arc<dyn Provider>, model: &str) -> Agent {
        Agent::new(
            provider,
            ToolRegistry::new(),
            Box::new(AllowAll),
            model.to_string(),
            4,
        )
    }

    fn assert_provider_is(actual: &Arc<dyn Provider>, expected: &Arc<dyn Provider>) {
        assert!(
            Arc::ptr_eq(actual, expected),
            "runtime snapshot carried the wrong Provider Arc"
        );
    }

    #[test]
    fn snapshot_clones_one_provider_model_tuple() {
        let initial_provider = provider();
        let runtime = AgentRuntime::new(initial_provider.clone(), "model-a".to_string());

        let snapshot = runtime.snapshot();

        assert_provider_is(&snapshot.provider, &initial_provider);
        assert_eq!(snapshot.model, "model-a");
    }

    #[test]
    fn parent_and_delegate_constructor_seams_share_runtime_handle() {
        let runtime = AgentRuntime::new(provider(), "model-a".to_string());
        let parent =
            Agent::with_runtime(runtime.clone(), ToolRegistry::new(), Box::new(AllowAll), 4);
        let delegate = DelegateTaskTool::new(runtime.clone(), ToolRegistry::new());

        assert!(parent.runtime().ptr_eq(delegate.runtime()));
    }

    #[test]
    fn set_provider_updates_runtime_provider_only() {
        let old_provider = provider();
        let new_provider = provider();
        let mut agent = agent(old_provider, "model-a");
        let runtime = agent.runtime();

        agent.set_provider(new_provider.clone());
        let snapshot = runtime.snapshot();

        assert_provider_is(&snapshot.provider, &new_provider);
        assert_eq!(snapshot.model, "model-a");
    }

    #[test]
    fn set_model_updates_runtime_model_only_and_clears_thinking() {
        let initial_provider = provider();
        let mut agent = agent(initial_provider.clone(), "model-a");
        let runtime = agent.runtime();
        let mut history = vec![Message::Assistant {
            text: "answer".to_string(),
            tool_calls: Vec::new(),
            thinking: vec![ThinkingBlock {
                text: "secret".to_string(),
                signature: None,
                redacted: false,
            }],
        }];

        agent.set_model("model-b".to_string(), &mut history);
        let snapshot = runtime.snapshot();

        assert!(matches!(
            &history[0],
            Message::Assistant { thinking, .. } if thinking.is_empty()
        ));
        assert_provider_is(&snapshot.provider, &initial_provider);
        assert_eq!(snapshot.model, "model-b");
    }

    #[test]
    fn restore_model_updates_runtime_model_only_and_preserves_thinking() {
        let initial_provider = provider();
        let mut agent = agent(initial_provider.clone(), "model-a");
        let runtime = agent.runtime();
        let history = [Message::Assistant {
            text: "answer".to_string(),
            tool_calls: Vec::new(),
            thinking: vec![ThinkingBlock {
                text: "preserve".to_string(),
                signature: None,
                redacted: false,
            }],
        }];

        agent.restore_model("model-b".to_string());
        let snapshot = runtime.snapshot();

        assert!(matches!(
            &history[0],
            Message::Assistant { thinking, .. } if thinking == &vec![ThinkingBlock {
                text: "preserve".to_string(),
                signature: None,
                redacted: false,
            }]
        ));
        assert_provider_is(&snapshot.provider, &initial_provider);
        assert_eq!(snapshot.model, "model-b");
    }

    #[test]
    fn pair_replace_commits_complete_tuple() {
        let runtime = AgentRuntime::new(provider(), "model-a".to_string());
        let new_provider = provider();

        runtime.replace_provider_model(new_provider.clone(), "model-b".to_string());
        let snapshot = runtime.snapshot();

        assert_provider_is(&snapshot.provider, &new_provider);
        assert_eq!(snapshot.model, "model-b");
    }

    #[test]
    fn cloned_snapshot_remains_frozen_after_pair_replace() {
        let old_provider = provider();
        let runtime = AgentRuntime::new(old_provider.clone(), "model-a".to_string());
        let frozen = runtime.snapshot();

        runtime.replace_provider_model(provider(), "model-b".to_string());

        assert_provider_is(&frozen.provider, &old_provider);
        assert_eq!(frozen.model, "model-a");
    }

    #[test]
    fn controlled_pair_replace_never_exposes_a_torn_tuple() {
        let runtime = AgentRuntime::new(provider(), "model-a".to_string());
        let new_provider = provider();
        let writer_runtime = runtime.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let writer = std::thread::spawn(move || {
            writer_runtime.replace_provider_model_with_hook(
                new_provider,
                "model-b".to_string(),
                || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                },
            );
        });

        if entered_rx.recv_timeout(Duration::from_millis(200)).is_err() {
            let _ = writer.join();
            panic!("pair replace尚未实现：未进入受控atomic write section");
        }

        let reader_runtime = runtime.clone();
        let (blocked_tx, blocked_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let blocked = matches!(
                reader_runtime.state.try_read(),
                Err(TryLockError::WouldBlock)
            );
            blocked_tx.send(blocked).unwrap();
            snapshot_tx.send(reader_runtime.snapshot()).unwrap();
        });
        let reader_observed_write_lock = blocked_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("reader未到达受控pair lock");

        release_tx.send(()).unwrap();
        writer.join().unwrap();
        let snapshot = snapshot_rx.recv_timeout(Duration::from_secs(1));
        let reader_result = reader.join();

        assert!(
            reader_observed_write_lock,
            "reader必须实际碰到replace持有的完整pair write lock"
        );
        reader_result.unwrap();
        let snapshot = snapshot.unwrap();
        assert_eq!(snapshot.model, "model-b");
    }

    #[tokio::test]
    async fn runtime_snapshot_does_not_hold_lock_across_provider_await() {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let stalled_provider: Arc<dyn Provider> = Arc::new(StalledProvider {
            entered: entered.clone(),
            release: release.clone(),
        });
        let runtime = AgentRuntime::new(stalled_provider, "model-a".to_string());
        let snapshot = runtime.snapshot();
        let provider_task = tokio::spawn(async move {
            snapshot
                .provider
                .complete(
                    ModelRequest {
                        model: snapshot.model,
                        messages: Vec::new(),
                        tools: Vec::new(),
                        max_tokens: None,
                        thinking: None,
                    },
                    &NoopSink,
                )
                .await
        });
        entered.notified().await;

        let replacement_runtime = runtime.clone();
        let replacement = tokio::task::spawn_blocking(move || {
            replacement_runtime.replace_provider_model(provider(), "model-b".to_string());
        });
        let replacement_result =
            tokio::time::timeout(Duration::from_millis(200), replacement).await;
        release.notify_one();
        provider_task.await.unwrap().unwrap();

        replacement_result
            .expect("runtime write must not wait for Provider await")
            .expect("pair replacement task must not panic");
    }

    #[tokio::test]
    async fn pair_switch_routes_next_parent_request_to_matching_provider_model() {
        let next_provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "new pair".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: Vec::new(),
        }]));
        let next_provider_trait: Arc<dyn Provider> = next_provider.clone();
        let mut agent = agent(provider(), "model-a");
        agent.restore_provider_model(next_provider_trait, "model-b".to_string());
        let mut history = vec![Message::User("use new pair".to_string())];

        let text = agent
            .run(
                &mut history,
                &ToolContext {
                    cwd: PathBuf::from("."),
                    max_output_bytes: 4096,
                },
                &NoopSink,
            )
            .await
            .unwrap();

        assert_eq!(text, "new pair");
        let requests = next_provider.recorded_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].model, "model-b");
    }

    #[test]
    fn pair_switch_updates_strategy_and_preserves_interactive_restore_semantics() {
        let providers = Arc::new(Mutex::new(Vec::new()));
        let models = Arc::new(Mutex::new(Vec::new()));
        let strategy = RecordingStrategy {
            providers: providers.clone(),
            models: models.clone(),
        };
        let mut agent = agent(provider(), "model-a");
        agent.set_strategy(Box::new(strategy));
        let replacement: Arc<dyn Provider> = Arc::new(MockProvider::new(Vec::new()));
        let mut interactive_history = vec![Message::Assistant {
            text: "answer".to_string(),
            tool_calls: Vec::new(),
            thinking: vec![ThinkingBlock {
                text: "clear me".to_string(),
                signature: None,
                redacted: false,
            }],
        }];

        agent.set_provider_model(
            replacement.clone(),
            "model-b".to_string(),
            &mut interactive_history,
        );
        assert!(matches!(
            &interactive_history[0],
            Message::Assistant { thinking, .. } if thinking.is_empty()
        ));

        let restore_history = [Message::Assistant {
            text: "answer".to_string(),
            tool_calls: Vec::new(),
            thinking: vec![ThinkingBlock {
                text: "keep me".to_string(),
                signature: None,
                redacted: false,
            }],
        }];
        agent.restore_provider_model(replacement, "model-c".to_string());
        assert!(matches!(
            &restore_history[0],
            Message::Assistant { thinking, .. } if thinking[0].text == "keep me"
        ));

        assert_eq!(
            *providers.lock().unwrap(),
            vec!["mock".to_string(), "mock".to_string()]
        );
        assert_eq!(
            *models.lock().unwrap(),
            vec!["model-b".to_string(), "model-c".to_string()]
        );
        assert_eq!(agent.runtime().snapshot().model, "model-c");
    }
}
