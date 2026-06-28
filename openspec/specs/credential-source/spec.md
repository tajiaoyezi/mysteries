# credential-source Specification

## Purpose
TBD - created by archiving change add-credential-chain. Update Purpose after archive.
## Requirements
### Requirement: 凭据来源抽象 CredentialSource

系统 SHALL 通过 `CredentialSource` trait 暴露「按 provider 名解析 API key」的能力。`resolve(&self, provider: &str)` MUST 返回 `Option<SecretString>`:命中 → `Some`,未命中 → `None`;MUST NOT panic。trait MUST 为 `Send + Sync`,以容纳后续并发装配。内核取用密钥 MUST 经 `SecretString`,不得以明文 `String` 在边界传递。

#### Scenario: 经 trait object 解析命中

- **WHEN** 调用方持有 `Box<dyn CredentialSource>`,且该来源对 provider `"openai"` 有对应密钥
- **THEN** `resolve("openai")` 返回 `Some(SecretString)`,调用方无需知道具体来源类型

#### Scenario: 未命中返回 None 不 panic

- **WHEN** 对一个该来源无对应凭据的 provider 调用 `resolve`
- **THEN** 返回 `None`,不 panic

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

### Requirement: 文件凭据来源 FileCredentialSource

`FileCredentialSource` SHALL 从一个**给定路径**的凭据文件按 `provider = key` 行格式解析(忽略首尾空白、空行与 `#` 注释行),命中目标 provider 行 → 以其 key 构造 `SecretString` 返回;无匹配行、或文件不存在 → `None`,MUST NOT panic。路径由构造时注入,以便测试用临时文件、不依赖真实 FS 布局。

#### Scenario: 命中 provider 行

- **WHEN** 凭据文件含一行 `openai = sk-file-xxx`,以该文件路径构造来源并 `resolve("openai")`
- **THEN** 返回 `Some(SecretString)`,其 `expose_secret()` 等于 `sk-file-xxx`

#### Scenario: 无匹配行或文件缺失

- **WHEN** 凭据文件不含目标 provider 行,或路径指向不存在的文件,调用 `resolve`
- **THEN** 返回 `None`,不 panic

### Requirement: 凭据链优先级 CredentialChain

`CredentialChain` SHALL 串接多个 `CredentialSource`,按构造顺序依次 `resolve`,返回**首个** `Some` 并短路其余;约定 **env 优先于 file**;所有来源皆 `None` → 链返回 `None`。

#### Scenario: env 命中优先于 file

- **WHEN** 链为 `[env, file]`,二者对 `"openai"` 均命中但值不同,调用 `resolve("openai")`
- **THEN** 返回 env 来源的值(file 来源被短路,不参与)

#### Scenario: env 缺失回落 file

- **WHEN** 链为 `[env, file]`,env 来源未命中、file 来源命中,调用 `resolve("openai")`
- **THEN** 返回 file 来源的值

#### Scenario: 全部缺失

- **WHEN** 链中所有来源对该 provider 均未命中
- **THEN** 返回 `None`

### Requirement: 密钥不暴露(secrecy 脱敏)

密钥 MUST 以 `secrecy::SecretString` 承载;其 `Debug`(及 `Display`,若实现)输出 MUST NOT 含明文密钥,从类型层面保证 key 不入日志 / 错误信息。取明文 MUST 经显式 `expose_secret()`,使解封成为可审计的集中动作。

#### Scenario: Debug 输出脱敏

- **WHEN** 对一个持有密钥 `sk-secret-xxx` 的 `SecretString` 执行 `format!("{:?}", secret)`
- **THEN** 输出不含子串 `sk-secret-xxx`(呈 redaction 占位),明文仅可经 `expose_secret()` 取得

### Requirement: 凭据写入(upsert)

系统 SHALL 提供向 `credentials` 文件 upsert 凭据的能力(如 `write_credential(path, provider, &SecretString)`):若文件已含该 provider 的 `provider = key` 行则**替换**其 key,否则**追加**一行;其他 provider 行与注释 MUST **保留**。取明文 MUST 经 `expose_secret()`(集中、可审计),明文 MUST NOT 入日志 / 错误信息。Unix 下新建 / 写入的凭据文件权限 SHALL 设为 `0600`(仅属主读写);路径由调用方注入,以便临时文件离线测试。写入失败 SHALL 返回错误(不 panic、不静默)。

#### Scenario: upsert 新增与替换并保留其他行

- **WHEN** `credentials` 初始含 `anthropic = sk-a`,先对 `openai` upsert `sk-o`,再对 `anthropic` upsert `sk-a2`
- **THEN** 文件含 `openai = sk-o` 与 `anthropic = sk-a2`(anthropic 被替换、非新增重复行),原有其他行保留

#### Scenario: 写入错误不含明文

- **WHEN** 写入失败(如路径不可写)
- **THEN** 返回错误,且错误信息不含 key 明文

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

