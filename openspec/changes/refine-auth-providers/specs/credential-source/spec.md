## MODIFIED Requirements

### Requirement: 环境变量凭据来源 EnvCredentialSource

`EnvCredentialSource` SHALL 将**预设** provider 逻辑名映射到约定的环境变量:`"openai"` → `OPENAI_API_KEY`、`"anthropic"` → `ANTHROPIC_API_KEY`、`"deepseek"` → `DEEPSEEK_API_KEY`;命中变量则以其值构造 `SecretString` 返回,未设置 → `None`。**非预设(自定义)逻辑名** MUST 返回 `None`(不走 env;自定义 provider 仅经 file 凭据)—— env 变量名为预设约定,自定义逻辑名无法预知。其对环境的读取 MUST 可注入替换,以便单测离线、确定性,不依赖进程级真实环境状态。

#### Scenario: openai 映射命中环境变量

- **WHEN** 环境(或注入的等价 lookup)中 `OPENAI_API_KEY` 为某非空值,调用 `resolve("openai")`
- **THEN** 返回 `Some(SecretString)`,其 `expose_secret()` 等于该值

#### Scenario: anthropic 映射命中环境变量

- **WHEN** 环境(或注入的等价 lookup)中 `ANTHROPIC_API_KEY` 为某非空值,调用 `resolve("anthropic")`
- **THEN** 返回 `Some(SecretString)`,其 `expose_secret()` 等于该值

#### Scenario: deepseek 映射命中环境变量

- **WHEN** 环境(或注入的等价 lookup)中 `DEEPSEEK_API_KEY` 为某非空值,调用 `resolve("deepseek")`
- **THEN** 返回 `Some(SecretString)`,其 `expose_secret()` 等于该值(与 `openai` / `OPENAI_API_KEY` 分离)

#### Scenario: 自定义逻辑名不走 env

- **WHEN** 对一个非预设逻辑名(如 `"myllm"`)调用 `resolve`,即便注入 lookup 含某 `MYLLM_API_KEY`
- **THEN** 返回 `None`(自定义名不映射 env,仅经 file 凭据)

#### Scenario: 环境变量未设置

- **WHEN** 环境(或注入的 lookup)中不含目标变量,调用 `resolve("openai")`
- **THEN** 返回 `None`

## ADDED Requirements

### Requirement: 凭据移除 remove_credential

系统 SHALL 提供从 `credentials` 文件移除指定 provider 凭据的能力(如 `remove_credential(path, provider)`):删除匹配的 `provider = key` 行,**保留其他 provider 行与注释**;以**原子**方式写回(temp + rename,沿用 `write_credential` 的安全写姿势),Unix 下写回文件权限 SHALL 为 `0600`。无匹配行或文件不存在 SHALL **幂等 no-op 成功**(返回 `Ok`,不报错、不 panic)。移除过程 MUST NOT 把任何 key 明文写入日志 / 错误信息。路径由调用方注入,以便临时文件离线测试。行解析 / 保留逻辑 SHOULD 可由纯函数单测。

#### Scenario: 移除指定 provider 行、保留其他行与注释

- **WHEN** `credentials` 含 `# header`、`openai = sk-o`、`deepseek = sk-d` 三行,调用 `remove_credential(path, "deepseek")`
- **THEN** 文件不再含 `deepseek` 行,仍含 `openai = sk-o` 与 `# header`;`FileCredentialSource::resolve("deepseek")` 为 `None`、`resolve("openai")` 仍为 `Some(sk-o)`

#### Scenario: 无匹配行或文件缺失幂等成功

- **WHEN** 对不含目标 provider 的 `credentials`、或指向不存在的路径调用 `remove_credential`
- **THEN** 返回 `Ok`,不报错 / 不 panic,既有其他行不变

#### Scenario: 移除写入失败不泄明文

- **WHEN** `remove_credential` 写回失败(如路径不可写)
- **THEN** 返回错误,且错误信息不含任何 key 明文

#### Scenario: Unix 写回保持 0600 权限

- **WHEN** 在 Unix 下对一个含多 provider 行的 `credentials` 调用 `remove_credential` 移除其一
- **THEN** 写回后的文件权限为 `0600`(仅属主读写)
