## 1. 队列取消让位浮层(red-green)

- [x] 1.1 **RED**:mod.rs tests 加两测并跑红——①`models_picker` 打开 + 排队非空,Esc → `handle_queue_cancel_key` 返回 false、不投 Interrupt、`last_cancel_at` 仍 None、队列原样;②`command_completion` 活跃(经 on_key 输入 `/` 触发)+ 排队非空,Esc → 同上。
- [x] 1.2 **GREEN**:`handle_queue_cancel_key` 早退条件加 `models_picker.is_some() || command_completion.is_some()`,两测转绿;既有 cancel 测试(首按/连按/pending 让位)保绿。

## 2. 推进闸门可测化 + 3.4 补课

- [x] 2.1 ui_rx 臂内联三步(算 `is_terminal` → `apply_ui_event` → 推进)抽成 `handle_agent_event(state, event, calling_model_started_at, first_token_at, input_tx)`,`select!` 臂只调它(等价重构,既有测试保绿)。
- [x] 2.2 四个集成测试(驱动 `handle_agent_event`)+ 中断断言:①TurnComplete+排队 ["B","C"] → 恰一条 `Prompt("B")`、transcript 尾 `User("B")`、phase Busy、"C" 留队;②Error 收场仍推进;③Interrupted 仍推进;④Idle 不推进、phase 不变,Idle 后提交入队不直发、随后 TurnComplete 恰发一条。另:既有 run_agent_task 中断测试补「Interrupted 后短窗内无尾随事件」断言。

## 3. 校验 + 记录

- [x] 3.1 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate fix-queue-cancel-modal-priority --strict` 过 + 既有快照零 churn。
- [x] 3.2 log 39 追加更正注(3.4 当时未实现,本 change 补课);新增决策记录 log 41。
