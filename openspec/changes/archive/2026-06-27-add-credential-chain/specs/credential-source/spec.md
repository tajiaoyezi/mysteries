## ADDED Requirements

### Requirement: 凭据来源抽象 CredentialSource

系统 SHALL 通过 `CredentialSource` trait 暴露「按 provider 名解析 API key」的能力。`resolve(&self, provider: &str)` MUST 返回 `Option<SecretString>`:命中 → `Some`,未命中 → `None`;MUST NOT panic。trait MUST 为 `Send + Sync`,以容纳后续并发装配。内核取用密钥 MUST 经 `SecretString`,不得以明文 `String` 在边界传递。

#### Scenario: 经 trait object 解析命中

- **WHEN** 调用方持有 `Box<dyn CredentialSource>`,且该来源对 provider `"openai"` 有对应密钥
- **THEN** `resolve("openai")` 返回 `Some(SecretString)`,调用方无需知道具体来源类型

#### Scenario: 未命中返回 None 不 panic

- **WHEN** 对一个该来源无对应凭据的 provider 调用 `resolve`
- **THEN** 返回 `None`,不 panic

### Requirement: 环境变量凭据来源 EnvCredentialSource

`EnvCredentialSource` SHALL 将 provider 名映射到约定的环境变量(1.0 至少含 `"openai"` → `OPENAI_API_KEY`),命中变量则以其值构造 `SecretString` 返回,未设置 → `None`。其对环境的读取 MUST 可注入替换,以便单测离线、确定性,不依赖进程级真实环境状态。

#### Scenario: openai 映射命中环境变量

- **WHEN** 环境(或注入的等价 lookup)中 `OPENAI_API_KEY` 为某非空值,调用 `resolve("openai")`
- **THEN** 返回 `Some(SecretString)`,其 `expose_secret()` 等于该值

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
