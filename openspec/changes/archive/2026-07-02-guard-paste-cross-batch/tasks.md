## 1. 纯函数:续读触发判定(RED 停点)

- [x] 1.1 `src/tui/input_batch.rs` 加 `pub fn would_submit_lone_enter(batch: &[Event]) -> bool`(实现 `classify_key_batch(&press_key_events(batch)).contains(&KeyIntent::Submit)`)。**先只写单测跑红**(新接口,贴红输出停等确认):`[Enter Press]` / `[Enter Press, Enter Release]` → `true`;`[Char, Enter]`(n=2)/ `[Char]` / 空批 → `false`;"`[Enter]` 后再 push 一个 `Char`" 的批 → `false`(经既有 `classify`,`Enter` 因 `n` 变大不再判 `Submit`——即"续批粘入后不再提交"的纯逻辑核心)。再写最小实现转绿。

## 2. drain 提交前续读接线

- [x] 2.1 `src/tui/mod.rs`:加具名常量 `const PASTE_CONTINUATION_GRACE: Duration = Duration::from_millis(10);`。
- [x] 2.2 `drain_event_batch` 改为续读循环(**不引墙钟、不记 last_batch_end、不改 `select!` 结构、不改 `process_event_batch`/`apply_batch_input_key`**):
  ```
  let mut batch = vec![ev0];
  loop {
      while poll(ZERO)? {
          batch.push(read()?);
          if batch.len() >= EVENT_BATCH_CAP { return Ok(batch); }
      }
      if !would_submit_lone_enter(&batch) || batch.len() >= EVENT_BATCH_CAP { break; }  // 正常打字/n>=2 换行/已达上限:零续读
      if poll(PASTE_CONTINUATION_GRACE)? {
          let ev = read()?;
          let is_key = matches!(&ev, Event::Key(_));
          batch.push(ev);
          if !is_key { break; }   // 续读只等键盘续批;鼠标 Moved/Focus/Resize 读入后即收批(交既有分治),避免 mouse capture 高频 Moved 令续读不退出
      } else {
          break;                  // 静默:真提交
      }
  }
  Ok(batch)
  ```
- [x] 2.3 确认续读**有界**:粘贴续批全是 `Event::Key`(Char/Enter 的 Press/Release),读到 `Char` 后 `n` 变大 → `would_submit_lone_enter` 转 false 退出;读到非 `Key` 事件(鼠标/焦点/resize)读入即 break;两条 CAP 检查覆盖抽干与续读两条路径。IO error 仍映射 `CliError::Io`(与既有 `drain` 一致);`process_event_batch` / `classify_key_batch` / `apply_batch_input_key` **一行不改**。

## 3. 校验

- [x] 3.1 `cargo test --lib` 全绿 + `cargo clippy --all-targets -- -D warnings` 零警告 + `openspec validate guard-paste-cross-batch --strict` 通过;**真机复核**:粘贴 20+ 行**不再自动发送**、手敲孤立 `Enter` 仍提交(多等 ~10ms 无感)、agent 流式生成时粘贴多行不误发、移鼠标后手敲 `Enter` 正常提交、**按 Enter 的同时持续移动鼠标不卡住提交/渲染**(诊断探针已回退,复核用正式构建)。
