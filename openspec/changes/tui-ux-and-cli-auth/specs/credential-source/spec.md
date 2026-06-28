## ADDED Requirements

### Requirement: 凭据写入(upsert)

系统 SHALL 提供向 `credentials` 文件 upsert 凭据的能力(如 `write_credential(path, provider, &SecretString)`):若文件已含该 provider 的 `provider = key` 行则**替换**其 key,否则**追加**一行;其他 provider 行与注释 MUST **保留**。取明文 MUST 经 `expose_secret()`(集中、可审计),明文 MUST NOT 入日志 / 错误信息。Unix 下新建 / 写入的凭据文件权限 SHALL 设为 `0600`(仅属主读写);路径由调用方注入,以便临时文件离线测试。写入失败 SHALL 返回错误(不 panic、不静默)。

#### Scenario: upsert 新增与替换并保留其他行

- **WHEN** `credentials` 初始含 `anthropic = sk-a`,先对 `openai` upsert `sk-o`,再对 `anthropic` upsert `sk-a2`
- **THEN** 文件含 `openai = sk-o` 与 `anthropic = sk-a2`(anthropic 被替换、非新增重复行),原有其他行保留

#### Scenario: 写入错误不含明文

- **WHEN** 写入失败(如路径不可写)
- **THEN** 返回错误,且错误信息不含 key 明文
