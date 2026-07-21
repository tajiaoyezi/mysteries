# add-readonly-subagent 真机核验

不要一次跑完。每次只做一项，完成后再做下一项。

脚本会自动准备和清理隔离环境。所有fixture仅监听`127.0.0.1`，使用临时HOME和假credentials；你不需要设置`$Exe`、fixture或config。

## TODO

- [ ] **9.3** 在项目根目录复制下面一条命令，之后只照屏幕提示操作：

  ```powershell
  pwsh -NoProfile -File .\openspec\changes\add-readonly-subagent\manual-verification.ps1 -Section 9.3
  ```

  弹出`§9.3 stall ready`后，关闭弹窗、回到TUI，只按一次`Esc`。

- [ ] **9.4** 9.3完成后复制下面一条命令：

  ```powershell
  pwsh -NoProfile -File .\openspec\changes\add-readonly-subagent\manual-verification.ps1 -Section 9.4
  ```

  这一项全自动，不需要操作TUI。出现`SKIP`时保留脚本给出的原始OS原因即可。

- [ ] **9.5** 9.4完成后复制下面一条命令，之后只照屏幕提示操作：

  ```powershell
  pwsh -NoProfile -File .\openspec\changes\add-readonly-subagent\manual-verification.ps1 -Section 9.5
  ```

## 怎么算完成

每项结束后，把脚本最后的`AUTO检查汇总`和`人工结果回报`原样发给实施Agent。不要自行分析日志。

## 仅在异常退出时使用

```powershell
pwsh -NoProfile -File .\openspec\changes\add-readonly-subagent\manual-verification.ps1 -Action CleanupStale
```

脚本不会kill进程或修改`tasks.md`。
