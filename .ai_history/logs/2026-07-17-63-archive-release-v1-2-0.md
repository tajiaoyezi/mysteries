# 2026-07-17 · 63 · archive-release-v1-2-0

## 决策
- 将 v1.2.0 定为首个自动化 GitHub Release | 选: annotated tag绑定已验证的master merge、双平台固定工具链构建、draft校验后公开 | 弃: 手工上传、lightweight tag、浮动toolchain、把archive commit作为tag目标 | 主导:讨论收敛 | 依据:release-delivery spec/CI/Release evidence
- 首次tag run保留失败现场并通过repair PR恢复 | 选: draft与asset全程绑定唯一Release/asset ID，repair merge后重新执行master门禁、清理未公开draft/tag、dry-run并取得新tag授权 | 弃: draft阶段使用public tag endpoint、手工公开draft、覆盖asset、复用旧SHA授权 | 主导:用户 | 依据:run 29238385225/PR #12/真实draft回归
- tag发布与OpenSpec archive分离 | 选:v1.2.0指向release merge，真实公开证据通过后再以archive commit同步spec、tasks和决策记录 | 弃:为证明发布自身追加递归evidence commit、移动已发布tag | 主导:讨论收敛 | 依据:design/release-delivery spec
- 发布权限保持最小化 | 选:workflow默认contents:read，仅publish job使用contents:write，公开验证匿名下载 | 弃:PAT、id-token、第三方release Action、checkout credential持久化 | 主导:讨论收敛 | 依据:dependency-security spec/workflow checks

## 变更
- PR #11：head bd170c18dfb6cf1e2fc76cc074b10df148843c01，merge f1c308fa5255b4a2f246ab680da2f9a08389d986，2026-07-13T08:20:48Z。
- 首次tag run 29238385225 attempt 1在Inspect draft Release through API失败；未公开draft保留供诊断，未手工公开或覆盖assets。
- repair PR #12：head a43fb5529200c1a975d7a0d0efd1e36cb9d06fce，merge 94de5e501c3d0fd3c1cef5078cfb1a07cb727d30，2026-07-18T02:49:33Z。
- master CI run 29627720689 attempt 1成功：
  - job 88035293037，Ubuntu fmt/clippy/test/build，success；
  - job 88035293044，Windows fmt/clippy/test/build，success；
  - 两个TESTED_REVISION均唯一等于94de5e501c3d0fd3c1cef5078cfb1a07cb727d30。
- Security run 29627720682 attempt 1成功：
  - job 88035293074，RustSec dependency audit，success；
  - TESTED_REVISION唯一等于release merge SHA。
- post-merge dry-run 29628007199 attempt 1成功，head SHA等于release merge：
  - metadata 88036094690、Windows package 88036111687、Linux package 88036111708、bundle 88036490592均success；
  - 三个RELEASE_REVISION均唯一等于release merge SHA；
  - publish 88036506916及两个public verify jobs按预期skipped。
- annotated tag：
  - tag object b441ae684c00d47cb990ba6c57ff895c4165e67f；
  - peeled commit 94de5e501c3d0fd3c1cef5078cfb1a07cb727d30；
  - remote direct/peeled refs唯一且未移动。
- tag Release run 29629469675 attempt 1，head SHA等于release merge，七个jobs全部success：
  - 88040211369 Validate release metadata；
  - 88040232426 Package release (windows-x86_64)；
  - 88040232424 Package release (linux-x86_64)；
  - 88040536023 Assemble release bundle；
  - 88040554192 Publish GitHub Release；
  - 88040580226 Verify published release (windows-x86_64)；
  - 88040580222 Verify published release (linux-x86_64)。
- 三个checkout jobs的RELEASE_REVISION均唯一等于release merge SHA；publish日志证明draft使用Release/asset ID读取和下载，公开后才使用tag/latest endpoints。
- GitHub Release：
  - ID 356011289；
  - URL https://github.com/tajiaoyezi/mysteries/releases/tag/v1.2.0；
  - publishedAt 2026-07-18T03:53:18Z；
  - draft=false、prerelease=false、latest=true。
- 公开assets：
  - mysteries-v1.2.0-x86_64-pc-windows-msvc.zip，4104799 bytes，sha256:942eba8ce3be4e58b2c963a118d3591af7b0b40900aca322b7d114aeb7f84e41；
  - mysteries-v1.2.0-x86_64-unknown-linux-gnu.tar.gz，4795223 bytes，sha256:cea41a70d8aa35bb458e5fe583afcb70aefb7d03af5bb27777ed56aace6e12c5；
  - SHA256SUMS，225 bytes，sha256:66b662acb072f99d35c078d5c9bdcceb616d58c8a8ac1c7e33ef8d56ea1ac668。
- SHA256SUMS内容：
  - 942eba8ce3be4e58b2c963a118d3591af7b0b40900aca322b7d114aeb7f84e41  mysteries-v1.2.0-x86_64-pc-windows-msvc.zip
  - cea41a70d8aa35bb458e5fe583afcb70aefb7d03af5bb27777ed56aace6e12c5  mysteries-v1.2.0-x86_64-unknown-linux-gnu.tar.gz
- Windows公开下载复核：checksum、LICENSE/README.md/mysteries.exe文件集、mysteries 1.2.0、--help均通过；Windows Terminal header为v1.2.0，正常退出，PowerShell立即可用；启动前后config/credential/session聚合指纹完全一致。
- Linux公开下载复核：job 88040580222匿名下载、checksum、文件集、executable bit、x86-64 ELF、GLIBC不高于2.35、--version与--help全部通过。
- strict OpenSpec验证：release-v1-2-0通过；全仓18 passed、0 failed；scope检查通过，无unstaged/staged/untracked/.snap.new。
- 1.0.0与1.1.0未补建tag；v1.2.0 binary/archive/checksum未提交Git；既有ci.yml与security-audit.yml相对发布前基线零diff。
- archive commit将同步release-delivery与dependency-security specs、把tasks更新为53/53并移动release-v1-2-0；archive后master领先v1.2.0 tag属于预期。

## 待决
- 无。macOS/ARM、crates.io、签名、SBOM与attestation继续作为后续独立change。

## 引用
- OpenSpec change: release-v1-2-0
- Specs: release-delivery、dependency-security
- PR: #11、#12
- Release run: 29629469675
- GitHub Release: v1.2.0
