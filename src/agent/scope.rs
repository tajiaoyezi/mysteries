use crate::error::AgentError;
use crate::tool::{PermissionLevel, Tool};
use std::collections::BTreeSet;
use thiserror::Error;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// 一次 Agent run 的稳定标识。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunIdentity {
    run_id: Uuid,
    parent_run_id: Option<Uuid>,
}

impl RunIdentity {
    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    pub fn parent_run_id(&self) -> Option<Uuid> {
        self.parent_run_id
    }
}

/// 单次 run 可消费的编排预算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionBudget {
    pub max_iterations: u32,
    pub deadline: Option<Instant>,
    pub remaining_child_depth: u32,
}

impl ExecutionBudget {
    pub fn new(max_iterations: u32, deadline: Option<Instant>, remaining_child_depth: u32) -> Self {
        Self {
            max_iterations,
            deadline,
            remaining_child_depth,
        }
    }
}

/// 一次 run 可见且可授权的工具能力集合。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionCapabilities {
    tool_names: BTreeSet<String>,
    permission_levels: BTreeSet<PermissionLevel>,
}

impl ExecutionCapabilities {
    pub fn try_new<I, S, J>(tool_names: I, permission_levels: J) -> Result<Self, ScopeDeriveError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        J: IntoIterator<Item = PermissionLevel>,
    {
        let mut unique_tool_names = BTreeSet::new();
        for tool_name in tool_names {
            let tool_name = tool_name.into();
            if !unique_tool_names.insert(tool_name.clone()) {
                return Err(ScopeDeriveError::DuplicateToolName(tool_name));
            }
        }

        let mut unique_permission_levels = BTreeSet::new();
        for permission_level in permission_levels {
            if !unique_permission_levels.insert(permission_level.clone()) {
                return Err(ScopeDeriveError::DuplicatePermissionLevel(permission_level));
            }
        }

        Ok(Self {
            tool_names: unique_tool_names,
            permission_levels: unique_permission_levels,
        })
    }

    pub(crate) fn from_known_unique<I, S, J>(tool_names: I, permission_levels: J) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        J: IntoIterator<Item = PermissionLevel>,
    {
        Self {
            tool_names: tool_names.into_iter().map(Into::into).collect(),
            permission_levels: permission_levels.into_iter().collect(),
        }
    }

    pub fn tool_names(&self) -> &BTreeSet<String> {
        &self.tool_names
    }

    pub fn permission_levels(&self) -> &BTreeSet<PermissionLevel> {
        &self.permission_levels
    }

    pub fn allows(&self, tool: &dyn Tool) -> bool {
        self.tool_names.contains(tool.name())
            && self.permission_levels.contains(&tool.permission_level())
    }
}

/// child scope 请求违反不可扩权规则。
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ScopeDeriveError {
    #[error("duplicate tool capability: {0}")]
    DuplicateToolName(String),
    #[error("duplicate permission capability: {0:?}")]
    DuplicatePermissionLevel(PermissionLevel),
    #[error("tool capability is not allowed by parent: {0}")]
    ToolNotAllowed(String),
    #[error("permission capability is not allowed by parent: {0:?}")]
    PermissionLevelNotAllowed(PermissionLevel),
    #[error(
        "iteration budget cannot expand from parent {parent_max_iterations} to child {requested_max_iterations}"
    )]
    IterationBudgetExpansion {
        parent_max_iterations: u32,
        requested_max_iterations: u32,
    },
    #[error("child deadline cannot be later than or remove the parent deadline")]
    DeadlineExpansion,
    #[error("child depth budget is exhausted")]
    ChildDepthExhausted,
    #[error(
        "child depth budget must be less than parent remaining depth {parent_remaining_depth}"
    )]
    ChildDepthExpansion { parent_remaining_depth: u32 },
}

/// scope 自身触发的终止原因，与 Provider timeout 分离。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Cancelled,
    DeadlineExceeded,
}

/// 带 execution scope 的 Agent 入口错误。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ScopedAgentError {
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error("agent run cancelled")]
    Cancelled,
    #[error("agent run deadline exceeded")]
    DeadlineExceeded,
}

impl From<StopReason> for ScopedAgentError {
    fn from(reason: StopReason) -> Self {
        match reason {
            StopReason::Cancelled => Self::Cancelled,
            StopReason::DeadlineExceeded => Self::DeadlineExceeded,
        }
    }
}

/// 一次 Agent run 的瞬时、可 clone 控制上下文。
#[derive(Clone, Debug)]
pub struct AgentExecutionScope {
    identity: RunIdentity,
    cancellation: CancellationToken,
    budget: ExecutionBudget,
    capabilities: ExecutionCapabilities,
}

impl AgentExecutionScope {
    pub fn root(budget: ExecutionBudget, capabilities: ExecutionCapabilities) -> Self {
        Self {
            identity: RunIdentity {
                run_id: Uuid::new_v4(),
                parent_run_id: None,
            },
            cancellation: CancellationToken::new(),
            budget,
            capabilities,
        }
    }

    pub fn identity(&self) -> RunIdentity {
        self.identity
    }

    pub fn budget(&self) -> &ExecutionBudget {
        &self.budget
    }

    pub fn capabilities(&self) -> &ExecutionCapabilities {
        &self.capabilities
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub fn termination_reason(&self) -> Option<StopReason> {
        if self.cancellation.is_cancelled() {
            return Some(StopReason::Cancelled);
        }

        self.budget
            .deadline
            .filter(|deadline| *deadline <= Instant::now())
            .map(|_| StopReason::DeadlineExceeded)
    }

    pub async fn terminated(&self) -> StopReason {
        if let Some(deadline) = self.budget.deadline {
            tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => StopReason::Cancelled,
                _ = tokio::time::sleep_until(deadline) => StopReason::DeadlineExceeded,
            }
        } else {
            self.cancellation.cancelled().await;
            StopReason::Cancelled
        }
    }

    pub fn derive_child(
        &self,
        budget: ExecutionBudget,
        capabilities: ExecutionCapabilities,
    ) -> Result<Self, ScopeDeriveError> {
        if self.budget.remaining_child_depth == 0 {
            return Err(ScopeDeriveError::ChildDepthExhausted);
        }
        if budget.remaining_child_depth >= self.budget.remaining_child_depth {
            return Err(ScopeDeriveError::ChildDepthExpansion {
                parent_remaining_depth: self.budget.remaining_child_depth,
            });
        }
        if budget.max_iterations > self.budget.max_iterations {
            return Err(ScopeDeriveError::IterationBudgetExpansion {
                parent_max_iterations: self.budget.max_iterations,
                requested_max_iterations: budget.max_iterations,
            });
        }
        if matches!((self.budget.deadline, budget.deadline), (Some(_), None))
            || matches!(
                (self.budget.deadline, budget.deadline),
                (Some(parent), Some(child)) if child > parent
            )
        {
            return Err(ScopeDeriveError::DeadlineExpansion);
        }
        if let Some(tool_name) = capabilities
            .tool_names
            .iter()
            .find(|tool_name| !self.capabilities.tool_names.contains(*tool_name))
        {
            return Err(ScopeDeriveError::ToolNotAllowed(tool_name.clone()));
        }
        if let Some(permission_level) = capabilities
            .permission_levels
            .iter()
            .find(|level| !self.capabilities.permission_levels.contains(level))
        {
            return Err(ScopeDeriveError::PermissionLevelNotAllowed(
                permission_level.clone(),
            ));
        }

        Ok(Self {
            identity: RunIdentity {
                run_id: Uuid::new_v4(),
                parent_run_id: Some(self.identity.run_id),
            },
            cancellation: self.cancellation.child_token(),
            budget,
            capabilities,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentExecutionScope, ExecutionBudget, ExecutionCapabilities, ScopeDeriveError, StopReason,
    };
    use crate::tool::PermissionLevel;
    use futures_util::FutureExt as _;
    use std::collections::BTreeSet;
    use std::time::Duration;
    use tokio::time::Instant;

    fn capabilities(
        tool_names: &[&str],
        permission_levels: &[PermissionLevel],
    ) -> ExecutionCapabilities {
        ExecutionCapabilities::try_new(
            tool_names.iter().copied(),
            permission_levels.iter().cloned(),
        )
        .unwrap()
    }

    fn budget(
        max_iterations: u32,
        deadline: Option<Instant>,
        remaining_child_depth: u32,
    ) -> ExecutionBudget {
        ExecutionBudget::new(max_iterations, deadline, remaining_child_depth)
    }

    fn root_with_depth(remaining_child_depth: u32) -> AgentExecutionScope {
        AgentExecutionScope::root(
            budget(8, None, remaining_child_depth),
            capabilities(
                &["read_file", "grep"],
                &[PermissionLevel::ReadOnly, PermissionLevel::Network],
            ),
        )
    }

    #[test]
    fn root_scopes_have_unique_run_ids() {
        let first = root_with_depth(2);
        let second = root_with_depth(2);

        assert_ne!(first.identity().run_id(), second.identity().run_id());
        assert_eq!(first.identity().parent_run_id(), None);
        assert_eq!(second.identity().parent_run_id(), None);
    }

    #[test]
    fn child_identity_points_to_direct_parent_and_differs_from_ancestors() {
        let parent = root_with_depth(2);
        let child = parent
            .derive_child(
                budget(8, None, 1),
                capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
            )
            .unwrap();
        let grandchild = child
            .derive_child(
                budget(4, None, 0),
                capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
            )
            .unwrap();

        assert_ne!(child.identity().run_id(), parent.identity().run_id());
        assert_eq!(
            child.identity().parent_run_id(),
            Some(parent.identity().run_id())
        );
        assert_ne!(grandchild.identity().run_id(), child.identity().run_id());
        assert_ne!(grandchild.identity().run_id(), parent.identity().run_id());
        assert_eq!(
            grandchild.identity().parent_run_id(),
            Some(child.identity().run_id())
        );
    }

    #[test]
    fn cloning_scope_preserves_identity() {
        let scope = root_with_depth(1);
        let cloned = scope.clone();

        assert_eq!(cloned.identity(), scope.identity());
    }

    #[test]
    fn parent_cancellation_propagates_to_all_descendants() {
        let parent = root_with_depth(2);
        let child = parent
            .derive_child(
                budget(8, None, 1),
                capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
            )
            .unwrap();
        let grandchild = child
            .derive_child(
                budget(4, None, 0),
                capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
            )
            .unwrap();

        parent.cancel();

        assert!(child.is_cancelled());
        assert!(grandchild.is_cancelled());
        assert_eq!(
            child.terminated().now_or_never(),
            Some(StopReason::Cancelled)
        );
        assert_eq!(
            grandchild.terminated().now_or_never(),
            Some(StopReason::Cancelled)
        );
    }

    #[test]
    fn child_cancellation_does_not_cancel_parent_or_sibling() {
        let parent = root_with_depth(2);
        let child = parent
            .derive_child(
                budget(8, None, 1),
                capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
            )
            .unwrap();
        let sibling = parent
            .derive_child(
                budget(8, None, 1),
                capabilities(&["grep"], &[PermissionLevel::ReadOnly]),
            )
            .unwrap();

        child.cancel();

        assert!(child.is_cancelled());
        assert!(!parent.is_cancelled());
        assert!(!sibling.is_cancelled());
    }

    #[test]
    fn cancel_before_wait_is_observed_immediately() {
        let scope = root_with_depth(1);
        scope.cancel();

        assert_eq!(
            scope.terminated().now_or_never(),
            Some(StopReason::Cancelled)
        );
    }

    #[test]
    fn child_derived_from_cancelled_parent_starts_cancelled() {
        let parent = root_with_depth(1);
        parent.cancel();

        let child = parent
            .derive_child(
                budget(4, None, 0),
                capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
            )
            .unwrap();

        assert!(child.is_cancelled());
        assert_eq!(
            child.terminated().now_or_never(),
            Some(StopReason::Cancelled)
        );
    }

    #[test]
    fn child_budget_may_keep_or_tighten_iteration_limit() {
        let parent = root_with_depth(2);

        let same = parent.derive_child(
            budget(8, None, 1),
            capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
        );
        let tighter = parent.derive_child(
            budget(3, None, 1),
            capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
        );

        assert_eq!(same.unwrap().budget().max_iterations, 8);
        assert_eq!(tighter.unwrap().budget().max_iterations, 3);
    }

    #[test]
    fn child_budget_rejects_iteration_expansion() {
        let parent = root_with_depth(2);

        let error = parent
            .derive_child(
                budget(9, None, 1),
                capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
            )
            .unwrap_err();

        assert_eq!(
            error,
            ScopeDeriveError::IterationBudgetExpansion {
                parent_max_iterations: 8,
                requested_max_iterations: 9,
            }
        );
    }

    #[test]
    fn child_budget_rejects_later_or_removed_parent_deadline() {
        let now = Instant::now();
        let parent_deadline = now + Duration::from_secs(10);
        let parent = AgentExecutionScope::root(
            budget(8, Some(parent_deadline), 2),
            capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
        );

        assert!(parent
            .derive_child(
                budget(8, Some(parent_deadline), 1),
                capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
            )
            .is_ok());
        assert!(parent
            .derive_child(
                budget(8, Some(now + Duration::from_secs(5)), 1),
                capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
            )
            .is_ok());
        assert_eq!(
            parent
                .derive_child(
                    budget(8, Some(now + Duration::from_secs(11)), 1),
                    capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
                )
                .unwrap_err(),
            ScopeDeriveError::DeadlineExpansion
        );
        assert_eq!(
            parent
                .derive_child(
                    budget(8, None, 1),
                    capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
                )
                .unwrap_err(),
            ScopeDeriveError::DeadlineExpansion
        );
    }

    #[test]
    fn child_budget_rejects_exhausted_or_non_decreasing_depth() {
        let exhausted = root_with_depth(0);
        assert_eq!(
            exhausted
                .derive_child(
                    budget(4, None, 0),
                    capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
                )
                .unwrap_err(),
            ScopeDeriveError::ChildDepthExhausted
        );

        let parent = root_with_depth(2);
        assert_eq!(
            parent
                .derive_child(
                    budget(4, None, 2),
                    capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
                )
                .unwrap_err(),
            ScopeDeriveError::ChildDepthExpansion {
                parent_remaining_depth: 2,
            }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_termination_uses_virtual_time() {
        let deadline = Instant::now() + Duration::from_secs(5);
        let scope = AgentExecutionScope::root(
            budget(8, Some(deadline), 0),
            capabilities(&["read_file"], &[PermissionLevel::ReadOnly]),
        );
        let waiter = tokio::spawn(async move { scope.terminated().await });
        tokio::task::yield_now().await;

        tokio::time::advance(Duration::from_secs(5)).await;

        assert_eq!(waiter.await.unwrap(), StopReason::DeadlineExceeded);
    }

    #[test]
    fn capability_subset_derivation_succeeds() {
        let parent = root_with_depth(2);
        let child_capabilities = capabilities(&["read_file"], &[PermissionLevel::ReadOnly]);

        let child = parent
            .derive_child(budget(4, None, 1), child_capabilities.clone())
            .unwrap();

        assert_eq!(child.capabilities(), &child_capabilities);
    }

    #[test]
    fn capability_derivation_rejects_tool_outside_parent_without_partial_scope() {
        let parent = root_with_depth(2);
        let parent_identity = parent.identity();

        let result = parent.derive_child(
            budget(4, None, 1),
            capabilities(&["read_file", "unknown_tool"], &[PermissionLevel::ReadOnly]),
        );

        assert_eq!(
            result.unwrap_err(),
            ScopeDeriveError::ToolNotAllowed("unknown_tool".to_string())
        );
        assert_eq!(parent.identity(), parent_identity);
        assert_eq!(
            parent.capabilities().tool_names(),
            &BTreeSet::from(["grep".to_string(), "read_file".to_string()])
        );
    }

    #[test]
    fn capability_derivation_rejects_permission_level_outside_parent() {
        let parent = root_with_depth(2);

        let result = parent.derive_child(
            budget(4, None, 1),
            capabilities(
                &["read_file"],
                &[PermissionLevel::ReadOnly, PermissionLevel::Execute],
            ),
        );

        assert_eq!(
            result.unwrap_err(),
            ScopeDeriveError::PermissionLevelNotAllowed(PermissionLevel::Execute)
        );
    }

    #[test]
    fn capability_constructor_rejects_duplicate_tool_names() {
        let result =
            ExecutionCapabilities::try_new(["read_file", "read_file"], [PermissionLevel::ReadOnly]);

        assert_eq!(
            result.unwrap_err(),
            ScopeDeriveError::DuplicateToolName("read_file".to_string())
        );
    }

    #[test]
    fn capability_constructor_rejects_duplicate_permission_levels() {
        let result = ExecutionCapabilities::try_new(
            ["read_file"],
            [PermissionLevel::ReadOnly, PermissionLevel::ReadOnly],
        );

        assert!(result.is_err());
    }
}
