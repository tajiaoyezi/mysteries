use serde_json::{json, Map, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const REPOSITORY: &str = "fixture-owner/mysteries";
const TAG: &str = "v1.3.1";
const RELEASE_ID: u64 = 42;
const NOTES: &str = "fixture release notes\n";
const WINDOWS_ASSET: &str = "mysteries-v1.3.1-x86_64-pc-windows-msvc.zip";
const LINUX_ASSET: &str = "mysteries-v1.3.1-x86_64-unknown-linux-gnu.tar.gz";
const CHECKSUM_ASSET: &str = "SHA256SUMS";
const REQUIRED_TOOLS: &[&str] = &[
    "awk",
    "cat",
    "cmp",
    "cp",
    "diff",
    "jq",
    "mkdir",
    "sha256sum",
    "sort",
    "stat",
];

#[derive(Clone)]
struct ResolvedTool {
    name: &'static str,
    path: String,
}

#[derive(Clone)]
struct BashToolchain {
    bash: PathBuf,
    tools: Vec<ResolvedTool>,
}

struct WorkflowScripts {
    create: String,
    upload: String,
    fetch: String,
    verify: String,
    download: String,
    publish: String,
    public_fetch: String,
    public_verify: String,
}

impl WorkflowScripts {
    fn load() -> Self {
        let workflow = workflow_text();
        Self {
            create: extract_step_script(&workflow, "Create draft Release"),
            upload: extract_step_script(&workflow, "Upload draft assets"),
            fetch: extract_step_script(&workflow, "Fetch draft Release metadata through API"),
            verify: extract_step_script(
                &workflow,
                "Verify draft Release metadata and body identity locally",
            ),
            download: extract_step_script(&workflow, "Download draft assets through API"),
            publish: extract_step_script(&workflow, "Publish verified Release as latest"),
            public_fetch: extract_step_script(
                &workflow,
                "Fetch public Release metadata through API",
            ),
            public_verify: extract_step_script(
                &workflow,
                "Verify public Release metadata and body identity locally",
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum PositiveCase {
    ListRemainsInvisible,
    DigestProvided,
    DraftAssetOrderDiffers,
}

#[derive(Clone, Copy, Debug)]
enum CreateMutation {
    TransportError,
    Status500,
    Status422,
    MissingId,
    ZeroId,
    FractionalId,
    StringId,
    WrongTag,
    WrongName,
    DraftFalse,
    PrereleaseTrue,
    BodyMismatch,
    BodyNotString,
    AssetsNonEmpty,
    AssetsNotArray,
    EmptyTarget,
    MissingTarget,
    ApiUrlWrongRepository,
    ApiUrlWrongId,
    UploadUrlWrongHost,
    UploadUrlWrongId,
    UploadUrlExtraQuery,
}

#[derive(Clone, Copy, Debug)]
enum UploadMutation {
    TransportError,
    Status500,
    ZeroId,
    FractionalId,
    WrongName,
    WrongState,
    WrongSize,
    WrongUrl,
    WrongDigest,
    DuplicateId,
    PartialSecondFailure,
}

#[derive(Clone, Copy, Debug)]
enum DraftMutation {
    WrongReleaseId,
    WrongTag,
    DraftFalse,
    PrereleaseTrue,
    EmptyTarget,
    BodyMismatch,
    AssetTupleMismatch,
}

const POSITIVE_CASES: &[(&str, PositiveCase)] = &[
    (
        "positive_list_remains_invisible",
        PositiveCase::ListRemainsInvisible,
    ),
    ("positive_digest_provided", PositiveCase::DigestProvided),
    (
        "positive_draft_asset_order_differs",
        PositiveCase::DraftAssetOrderDiffers,
    ),
];

const CREATE_CASES: &[(&str, CreateMutation)] = &[
    ("create_transport_nonzero", CreateMutation::TransportError),
    ("create_status_500", CreateMutation::Status500),
    ("create_status_422_conflict", CreateMutation::Status422),
    ("create_missing_id", CreateMutation::MissingId),
    ("create_zero_id", CreateMutation::ZeroId),
    ("create_fractional_id", CreateMutation::FractionalId),
    ("create_string_id", CreateMutation::StringId),
    ("create_wrong_tag", CreateMutation::WrongTag),
    ("create_wrong_name", CreateMutation::WrongName),
    ("create_draft_false", CreateMutation::DraftFalse),
    ("create_prerelease_true", CreateMutation::PrereleaseTrue),
    ("create_body_mismatch", CreateMutation::BodyMismatch),
    ("create_body_not_string", CreateMutation::BodyNotString),
    ("create_assets_non_empty", CreateMutation::AssetsNonEmpty),
    ("create_assets_not_array", CreateMutation::AssetsNotArray),
    ("create_empty_target", CreateMutation::EmptyTarget),
    ("create_missing_target", CreateMutation::MissingTarget),
    (
        "create_api_url_wrong_repository",
        CreateMutation::ApiUrlWrongRepository,
    ),
    ("create_api_url_wrong_id", CreateMutation::ApiUrlWrongId),
    (
        "create_upload_url_wrong_host",
        CreateMutation::UploadUrlWrongHost,
    ),
    (
        "create_upload_url_wrong_id",
        CreateMutation::UploadUrlWrongId,
    ),
    (
        "create_upload_url_extra_query",
        CreateMutation::UploadUrlExtraQuery,
    ),
];

const UPLOAD_CASES: &[(&str, UploadMutation)] = &[
    ("upload_transport_nonzero", UploadMutation::TransportError),
    ("upload_status_500", UploadMutation::Status500),
    ("upload_zero_id", UploadMutation::ZeroId),
    ("upload_fractional_id", UploadMutation::FractionalId),
    ("upload_wrong_name", UploadMutation::WrongName),
    ("upload_wrong_state", UploadMutation::WrongState),
    ("upload_wrong_size", UploadMutation::WrongSize),
    ("upload_wrong_url", UploadMutation::WrongUrl),
    ("upload_wrong_digest", UploadMutation::WrongDigest),
    ("upload_duplicate_id", UploadMutation::DuplicateId),
    (
        "upload_partial_second_failure",
        UploadMutation::PartialSecondFailure,
    ),
];

const DRAFT_CASES: &[(&str, DraftMutation)] = &[
    ("draft_wrong_release_id", DraftMutation::WrongReleaseId),
    ("draft_wrong_tag", DraftMutation::WrongTag),
    ("draft_false", DraftMutation::DraftFalse),
    ("draft_prerelease_true", DraftMutation::PrereleaseTrue),
    ("draft_empty_target", DraftMutation::EmptyTarget),
    ("draft_body_mismatch", DraftMutation::BodyMismatch),
    (
        "draft_asset_tuple_mismatch",
        DraftMutation::AssetTupleMismatch,
    ),
];

struct Fixture {
    root: TempDir,
    toolchain: BashToolchain,
}

impl Fixture {
    fn new(toolchain: &BashToolchain) -> Self {
        let root = tempfile::tempdir().expect("创建release workflow fixture目录");
        fs::create_dir_all(root.path().join("bundle")).expect("创建bundle目录");
        fs::create_dir_all(root.path().join("runner/fake-upload")).expect("创建fake upload目录");
        fs::create_dir_all(root.path().join("isolated-bin")).expect("创建isolated PATH目录");

        fs::write(root.path().join("bundle/release-notes.md"), NOTES)
            .expect("写入release notes fixture");
        fs::write(
            root.path().join(format!("bundle/{WINDOWS_ASSET}")),
            b"windows archive fixture\n",
        )
        .expect("写入Windows asset fixture");
        fs::write(
            root.path().join(format!("bundle/{LINUX_ASSET}")),
            b"linux archive fixture\n",
        )
        .expect("写入Linux asset fixture");
        fs::write(
            root.path().join(format!("bundle/{CHECKSUM_ASSET}")),
            b"checksum fixture\n",
        )
        .expect("写入checksum fixture");
        fs::write(root.path().join("fake-tools.sh"), fake_tools_script())
            .expect("写入fake curl/gh");
        for tool in &toolchain.tools {
            write_tool_wrapper(root.path(), tool.name);
        }
        write_network_deny_shim(root.path(), "curl");
        write_network_deny_shim(root.path(), "gh");

        let fixture = Self {
            root,
            toolchain: toolchain.clone(),
        };
        fixture.write_create_response(&baseline_create_response());
        fixture.write_create_status("201");
        fixture.write_upload_responses(&fixture.baseline_upload_responses());
        fixture.write_upload_statuses(&["201", "201", "201"]);
        fixture.write_draft_response(&fixture.baseline_draft_response());
        fixture
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn write_json(&self, relative: &str, value: &Value) {
        let bytes = serde_json::to_vec_pretty(value).expect("序列化fixture JSON");
        fs::write(self.path(relative), bytes).expect("写入fixture JSON");
    }

    fn read_json(&self, relative: &str) -> Value {
        let bytes = fs::read(self.path(relative)).expect("读取fixture JSON");
        serde_json::from_slice(&bytes).expect("解析fixture JSON")
    }

    fn write_create_response(&self, value: &Value) {
        self.write_json("runner/fake-create-response.json", value);
    }

    fn create_response(&self) -> Value {
        self.read_json("runner/fake-create-response.json")
    }

    fn write_create_status(&self, status: &str) {
        fs::write(self.path("runner/fake-create-status"), status).expect("写入Create status");
    }

    fn fail_create_transport(&self) {
        fs::write(self.path("runner/fake-create-transport-exit"), "56\n")
            .expect("写入Create transport error");
    }

    fn write_upload_responses(&self, values: &[Value]) {
        for (index, value) in values.iter().enumerate() {
            self.write_json(&format!("runner/fake-upload/{}.json", index + 1), value);
        }
    }

    fn upload_responses(&self) -> Vec<Value> {
        (1..=3)
            .map(|index| self.read_json(&format!("runner/fake-upload/{index}.json")))
            .collect()
    }

    fn write_upload_statuses(&self, statuses: &[&str]) {
        for (index, status) in statuses.iter().enumerate() {
            fs::write(
                self.path(&format!("runner/fake-upload/{}.status", index + 1)),
                status,
            )
            .expect("写入Upload status");
        }
    }

    fn fail_upload_transport_at(&self, call: usize) {
        fs::write(
            self.path("runner/fake-upload-transport-at"),
            format!("{call}\n"),
        )
        .expect("写入Upload transport error");
    }

    fn write_draft_response(&self, value: &Value) {
        self.write_json("runner/fake-draft-response.json", value);
    }

    fn draft_response(&self) -> Value {
        self.read_json("runner/fake-draft-response.json")
    }

    fn baseline_upload_responses(&self) -> Vec<Value> {
        [WINDOWS_ASSET, LINUX_ASSET, CHECKSUM_ASSET]
            .iter()
            .enumerate()
            .map(|(index, asset)| {
                let id = 1001 + index as u64;
                let size = fs::metadata(self.path(&format!("bundle/{asset}")))
                    .expect("读取asset metadata")
                    .len();
                json!({
                    "id": id,
                    "name": asset,
                    "state": "uploaded",
                    "size": size,
                    "url": format!(
                        "https://api.github.com/repos/{REPOSITORY}/releases/assets/{id}"
                    ),
                    "digest": null
                })
            })
            .collect()
    }

    fn baseline_draft_response(&self) -> Value {
        json!({
            "id": RELEASE_ID,
            "tag_name": TAG,
            "name": format!("mysteries {TAG}"),
            "draft": true,
            "prerelease": false,
            "target_commitish": "fixture-candidate",
            "body": NOTES,
            "assets": self.baseline_upload_responses()
        })
    }

    fn run_step(&self, name: &str, script: &str, with_token: bool) -> Output {
        let script_name = format!("{name}.sh");
        let guarded = format!(
            "set -euo pipefail\n\
             PATH=isolated-bin\n\
             export PATH\n\
             test \"$PATH\" = isolated-bin\n\
             test \"$(type -t curl)\" = function\n\
             test \"$(type -t gh)\" = function\n\
             {script}"
        );
        fs::write(self.path(&script_name), guarded).expect("写入提取后的workflow step");

        let mut command = Command::new(&self.toolchain.bash);
        command
            .arg(&script_name)
            .current_dir(self.root.path())
            .env("PATH", "isolated-bin")
            .env("BASH_ENV", "./fake-tools.sh")
            .env("RUNNER_TEMP", "runner")
            .env("GITHUB_OUTPUT", "runner/github-output")
            .env("GITHUB_REPOSITORY", REPOSITORY)
            .env("TAG", TAG)
            .env("WINDOWS_ASSET", WINDOWS_ASSET)
            .env("LINUX_ASSET", LINUX_ASSET)
            .env("RELEASE_ID", RELEASE_ID.to_string())
            .env(
                "UPLOAD_URL",
                format!(
                    "https://uploads.github.com/repos/{REPOSITORY}/releases/{RELEASE_ID}/assets{{?name,label}}"
                ),
            )
            .env(
                "FAKE_CREATE_RESPONSE",
                "runner/fake-create-response.json",
            )
            .env("FAKE_CREATE_STATUS", "runner/fake-create-status")
            .env(
                "FAKE_CREATE_TRANSPORT_EXIT",
                "runner/fake-create-transport-exit",
            )
            .env("FAKE_UPLOAD_DIR", "runner/fake-upload")
            .env("FAKE_UPLOAD_COUNT", "runner/upload-count")
            .env(
                "FAKE_UPLOAD_TRANSPORT_AT",
                "runner/fake-upload-transport-at",
            )
            .env("FAKE_CURL_LOG", "runner/curl.log")
            .env("FAKE_GH_LOG", "runner/gh.log")
            .env("FAKE_DRAFT_RESPONSE", "runner/fake-draft-response.json")
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN");
        for tool in &self.toolchain.tools {
            command.env(tool_env_name(tool.name), &tool.path);
        }
        if cfg!(windows) {
            // Windows jq默认以text mode写重定向文件；fixture用官方--binary开关保持
            // Git仓库内release notes的LF字节身份，真实publish job仍在Ubuntu执行。
            command.env("FIXTURE_JQ_BINARY", "1");
        }
        if with_token {
            command.env("GH_TOKEN", "fixture-token-not-a-credential");
        }
        command.output().expect("执行提取后的workflow step")
    }

    fn curl_calls(&self) -> Vec<String> {
        read_lines_if_exists(&self.path("runner/curl.log"))
    }

    fn gh_calls(&self) -> Vec<String> {
        read_lines_if_exists(&self.path("runner/gh.log"))
    }

    fn upload_count(&self) -> usize {
        fs::read_to_string(self.path("runner/upload-count"))
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    }

    fn github_output(&self) -> String {
        fs::read_to_string(self.path("runner/github-output")).unwrap_or_default()
    }

    fn sha256(&self, asset: &str) -> String {
        let sha256sum = self
            .toolchain
            .tools
            .iter()
            .find(|tool| tool.name == "sha256sum")
            .expect("fixture必须解析sha256sum");
        let output = Command::new(&self.toolchain.bash)
            .args(["-c", &format!("\"$FIXTURE_SHA256SUM\" bundle/{asset}")])
            .current_dir(self.root.path())
            .env("FIXTURE_SHA256SUM", &sha256sum.path)
            .output()
            .expect("计算fixture SHA-256");
        assert!(
            output.status.success(),
            "sha256sum失败: {}",
            output_details(&output)
        );
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .expect("sha256sum输出hash")
            .to_string()
    }
}

#[test]
fn release_workflow_static_contract_is_fail_closed() {
    let workflow = workflow_text();
    let scripts = WorkflowScripts::load();

    for (name, script) in [
        ("Create", scripts.create.as_str()),
        ("Upload", scripts.upload.as_str()),
        ("GET", scripts.fetch.as_str()),
        ("Verify", scripts.verify.as_str()),
        ("Download", scripts.download.as_str()),
        ("Publish", scripts.publish.as_str()),
        ("Public GET", scripts.public_fetch.as_str()),
        ("Public Verify", scripts.public_verify.as_str()),
    ] {
        assert!(
            script.starts_with("set -euo pipefail\n"),
            "{name} step没有提取到真实Bash body"
        );
    }

    assert_eq!(scripts.create.matches("--request POST").count(), 1);
    assert!(scripts.create.contains("test \"$status\" = \"201\""));
    assert!(scripts.create.contains(".upload_url == $upload_url"));
    assert!(scripts.create.contains("release_id=%s"));
    assert!(scripts.create.contains("api_url=%s"));
    assert!(scripts.create.contains("upload_url=%s"));
    assert!(scripts.upload.contains("test \"$status\" = \"201\""));
    assert!(scripts
        .upload
        .contains("$RUNNER_TEMP/uploaded-assets.ndjson"));
    assert!(scripts
        .fetch
        .contains("gh api \"repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID\" > draft.json"));
    assert!(scripts.verify.contains("$RUNNER_TEMP/uploaded-assets.json"));
    assert!(scripts.verify.contains("$RUNNER_TEMP/draft-assets.json"));
    assert!(scripts.publish.contains("gh api --method PATCH"));
    assert!(scripts
        .publish
        .contains("\"repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID\""));
    assert!(scripts.publish.contains("-F draft=false"));
    assert!(scripts.publish.contains("-f make_latest=true"));

    assert_step_has_lines(
        &workflow,
        "Create draft Release",
        &[
            "        id: draft_release",
            "          GH_TOKEN: ${{ github.token }}",
        ],
    );
    assert_step_has_lines(
        &workflow,
        "Upload draft assets",
        &[
            "          GH_TOKEN: ${{ github.token }}",
            "          RELEASE_ID: ${{ steps.draft_release.outputs.release_id }}",
            "          UPLOAD_URL: ${{ steps.draft_release.outputs.upload_url }}",
        ],
    );
    assert_step_has_lines(
        &workflow,
        "Fetch draft Release metadata through API",
        &[
            "          GH_TOKEN: ${{ github.token }}",
            "          RELEASE_ID: ${{ steps.draft_release.outputs.release_id }}",
        ],
    );
    assert_step_has_lines(
        &workflow,
        "Verify draft Release metadata and body identity locally",
        &["          RELEASE_ID: ${{ steps.draft_release.outputs.release_id }}"],
    );
    assert_step_has_lines(
        &workflow,
        "Download draft assets through API",
        &[
            "          GH_TOKEN: ${{ github.token }}",
            "          RELEASE_ID: ${{ steps.draft_release.outputs.release_id }}",
        ],
    );
    assert_step_has_lines(
        &workflow,
        "Publish verified Release as latest",
        &[
            "          GH_TOKEN: ${{ github.token }}",
            "          RELEASE_ID: ${{ steps.draft_release.outputs.release_id }}",
        ],
    );
    assert_step_has_lines(
        &workflow,
        "Fetch public Release metadata through API",
        &[
            "          GH_TOKEN: ${{ github.token }}",
            "          RELEASE_ID: ${{ steps.draft_release.outputs.release_id }}",
        ],
    );
    assert_step_has_lines(
        &workflow,
        "Verify public Release metadata and body identity locally",
        &["          RELEASE_ID: ${{ steps.draft_release.outputs.release_id }}"],
    );
    assert_step_order(
        &workflow,
        &[
            "Create draft Release",
            "Upload draft assets",
            "Fetch draft Release metadata through API",
            "Verify draft Release metadata and body identity locally",
            "Download draft assets through API",
            "Publish verified Release as latest",
            "Fetch public Release metadata through API",
            "Verify public Release metadata and body identity locally",
        ],
    );

    let post_create = workflow_between_steps(
        &workflow,
        "Create draft Release",
        "Fetch public Release metadata through API",
    );
    for forbidden in [
        "gh api --paginate",
        "releases?per_page",
        "gh release create",
        "gh release upload",
        "--clobber",
        "--retry",
        "--request DELETE",
        "--method DELETE",
        "gh release delete",
        "/releases/tags/",
    ] {
        assert!(
            !post_create.contains(forbidden),
            "Create之后出现禁止路径 `{forbidden}`"
        );
    }
    for (name, script) in [
        ("Create", scripts.create.as_str()),
        ("Upload", scripts.upload.as_str()),
        ("GET", scripts.fetch.as_str()),
        ("Download", scripts.download.as_str()),
        ("Publish", scripts.publish.as_str()),
        ("Public GET", scripts.public_fetch.as_str()),
    ] {
        assert_no_network_tool_bypass(name, script);
    }
}

#[test]
fn release_workflow_create_upload_and_get_fixture_matrix() {
    let case_count =
        POSITIVE_CASES.len() + CREATE_CASES.len() + UPLOAD_CASES.len() + DRAFT_CASES.len();
    assert_eq!(case_count, 43, "release workflow fixture矩阵数量漂移");

    let Some(toolchain) = bash_with_required_tools() else {
        return;
    };
    let scripts = WorkflowScripts::load();
    let mut failures = Vec::new();
    let mut executed_cases = 0;

    for (name, case) in POSITIVE_CASES {
        executed_cases += 1;
        if let Err(error) = run_positive_case(&toolchain, &scripts, *case) {
            failures.push(format!("{name}: {error}"));
        }
    }
    for (name, mutation) in CREATE_CASES {
        executed_cases += 1;
        if let Err(error) = run_create_negative_case(&toolchain, &scripts, *mutation) {
            failures.push(format!("{name}: {error}"));
        }
    }
    for (name, mutation) in UPLOAD_CASES {
        executed_cases += 1;
        if let Err(error) = run_upload_negative_case(&toolchain, &scripts, *mutation) {
            failures.push(format!("{name}: {error}"));
        }
    }
    for (name, mutation) in DRAFT_CASES {
        executed_cases += 1;
        if let Err(error) = run_draft_negative_case(&toolchain, &scripts, *mutation) {
            failures.push(format!("{name}: {error}"));
        }
    }
    assert_eq!(executed_cases, 43, "release workflow fixture未执行全部case");
    eprintln!("EXECUTED: {executed_cases} release workflow fixture cases");

    assert!(
        failures.is_empty(),
        "{} / 43 release workflow fixture cases失败:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn run_positive_case(
    toolchain: &BashToolchain,
    scripts: &WorkflowScripts,
    case: PositiveCase,
) -> Result<(), String> {
    let fixture = Fixture::new(toolchain);
    match case {
        PositiveCase::ListRemainsInvisible => {}
        PositiveCase::DigestProvided => {
            let mut uploads = fixture.upload_responses();
            let mut draft = fixture.draft_response();
            for (index, asset) in [WINDOWS_ASSET, LINUX_ASSET, CHECKSUM_ASSET]
                .iter()
                .enumerate()
            {
                let digest = format!("sha256:{}", fixture.sha256(asset));
                uploads[index]["digest"] = json!(digest);
                draft["assets"][index]["digest"] = uploads[index]["digest"].clone();
            }
            fixture.write_upload_responses(&uploads);
            fixture.write_draft_response(&draft);
        }
        PositiveCase::DraftAssetOrderDiffers => {
            let mut draft = fixture.draft_response();
            draft["assets"]
                .as_array_mut()
                .expect("draft assets array")
                .reverse();
            fixture.write_draft_response(&draft);
        }
    }

    let create_output = fixture.run_step("create", &scripts.create, true);
    if let Err(error) = expect_success("Create", create_output) {
        return Err(format!(
            "{error}; curl={:?}; response={}; extracted_body={:?}",
            fixture.curl_calls(),
            fs::read_to_string(fixture.path("runner/create-release.json"))
                .unwrap_or_else(|_| "<missing>".to_string()),
            fs::read_to_string(fixture.path("runner/create-release-body")).ok()
        ));
    }
    let output = fixture.github_output();
    for expected in [
        "release_id=42",
        "api_url=https://api.github.com/repos/fixture-owner/mysteries/releases/42",
        "upload_url=https://uploads.github.com/repos/fixture-owner/mysteries/releases/42/assets{?name,label}",
    ] {
        if !output.lines().any(|line| line == expected) {
            return Err(format!("Create output缺少 `{expected}`: {output:?}"));
        }
    }

    expect_success("Upload", fixture.run_step("upload", &scripts.upload, true))?;
    expect_success("GET", fixture.run_step("fetch", &scripts.fetch, true))?;
    expect_success("Verify", fixture.run_step("verify", &scripts.verify, false))?;

    let curl_calls = fixture.curl_calls();
    if curl_calls.len() != 4 {
        return Err(format!("预期1次Create+3次Upload，实际{curl_calls:?}"));
    }
    if fixture.upload_count() != 3 {
        return Err(format!("预期3次Upload，实际{}", fixture.upload_count()));
    }
    let gh_calls = fixture.gh_calls();
    if gh_calls != [format!("api repos/{REPOSITORY}/releases/{RELEASE_ID}")] {
        return Err(format!("GET未只绑定captured ID: {gh_calls:?}"));
    }
    if fixture.path("published.json").exists() {
        return Err("fixture不应执行public PATCH".to_string());
    }
    Ok(())
}

fn run_create_negative_case(
    toolchain: &BashToolchain,
    scripts: &WorkflowScripts,
    mutation: CreateMutation,
) -> Result<(), String> {
    let fixture = Fixture::new(toolchain);
    apply_create_mutation(&fixture, mutation);

    let output = fixture.run_step("create-negative", &scripts.create, true);
    if matches!(mutation, CreateMutation::TransportError) && output.status.code() != Some(56) {
        return Err(format!(
            "Create transport case未返回预期exit 56: {}",
            output_details(&output)
        ));
    }
    expect_failure("Create negative", output)?;
    if fixture.curl_calls().len() != 1 {
        return Err(format!("Create失败后发生重试: {:?}", fixture.curl_calls()));
    }
    if fixture.upload_count() != 0 {
        return Err("Create identity失败后仍执行了Upload".to_string());
    }
    if !fixture.github_output().is_empty() {
        return Err(format!(
            "Create identity失败后仍写出identity: {:?}",
            fixture.github_output()
        ));
    }
    let reached_body_check = fixture.path("runner/create-release-body").exists();
    let expected_body_check = matches!(
        mutation,
        CreateMutation::BodyMismatch | CreateMutation::BodyNotString
    );
    if reached_body_check != expected_body_check {
        return Err(format!(
            "Create未在预期边界失败: mutation={mutation:?}, reached_body_check={reached_body_check}"
        ));
    }
    Ok(())
}

fn run_upload_negative_case(
    toolchain: &BashToolchain,
    scripts: &WorkflowScripts,
    mutation: UploadMutation,
) -> Result<(), String> {
    let fixture = Fixture::new(toolchain);
    let expected_calls = apply_upload_mutation(&fixture, mutation);

    let output = fixture.run_step("upload-negative", &scripts.upload, true);
    if matches!(mutation, UploadMutation::TransportError) && output.status.code() != Some(56) {
        return Err(format!(
            "Upload transport case未返回预期exit 56: {}",
            output_details(&output)
        ));
    }
    expect_failure("Upload negative", output)?;
    if fixture.upload_count() != expected_calls {
        return Err(format!(
            "Upload失败边界错误: 预期{expected_calls}次，实际{}次",
            fixture.upload_count()
        ));
    }
    let recorded = read_lines_if_exists(&fixture.path("runner/uploaded-assets.ndjson")).len();
    let expected_recorded = match mutation {
        UploadMutation::DuplicateId => 3,
        UploadMutation::PartialSecondFailure => 1,
        _ => 0,
    };
    if recorded != expected_recorded {
        return Err(format!(
            "Upload未在预期边界失败: mutation={mutation:?}, \
             recorded={recorded}, expected={expected_recorded}"
        ));
    }
    if fs::metadata(fixture.path("runner/uploaded-assets.json"))
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
    {
        return Err("Upload负向case意外形成完整asset tuple集合".to_string());
    }
    if !fixture.gh_calls().is_empty() || fixture.path("draft.json").exists() {
        return Err("Upload失败后仍执行了draft GET".to_string());
    }
    if fixture.path("published.json").exists() {
        return Err("Upload失败后仍执行了public PATCH".to_string());
    }
    Ok(())
}

fn run_draft_negative_case(
    toolchain: &BashToolchain,
    scripts: &WorkflowScripts,
    mutation: DraftMutation,
) -> Result<(), String> {
    let fixture = Fixture::new(toolchain);
    apply_draft_mutation(&fixture, mutation);

    expect_success(
        "Upload before GET",
        fixture.run_step("upload-before-get", &scripts.upload, true),
    )?;
    expect_success(
        "GET before Verify",
        fixture.run_step("get-before-verify", &scripts.fetch, true),
    )?;
    expect_failure(
        "Verify negative",
        fixture.run_step("verify-negative", &scripts.verify, false),
    )?;
    let reached_tuple_check = fixture.path("runner/draft-assets.json").exists();
    let reached_body_check = fixture.path("runner/draft-body").exists();
    match mutation {
        DraftMutation::BodyMismatch => {
            if !reached_tuple_check || !reached_body_check {
                return Err("draft body负向case未通过metadata/tuple前置检查".to_string());
            }
        }
        DraftMutation::AssetTupleMismatch => {
            if !reached_tuple_check || reached_body_check {
                return Err("draft tuple负向case未在tuple比较处失败".to_string());
            }
        }
        _ => {
            if reached_tuple_check || reached_body_check {
                return Err(format!(
                    "draft metadata负向case越过预期边界: mutation={mutation:?}"
                ));
            }
        }
    }
    if fixture.gh_calls() != [format!("api repos/{REPOSITORY}/releases/{RELEASE_ID}")] {
        return Err(format!(
            "draft验证未只使用captured ID: {:?}",
            fixture.gh_calls()
        ));
    }
    if fixture.path("published.json").exists() {
        return Err("draft identity失败后仍执行了public PATCH".to_string());
    }
    Ok(())
}

fn apply_create_mutation(fixture: &Fixture, mutation: CreateMutation) {
    if matches!(mutation, CreateMutation::TransportError) {
        fixture.fail_create_transport();
        return;
    }
    if matches!(mutation, CreateMutation::Status500) {
        fixture.write_create_status("500");
        return;
    }
    if matches!(mutation, CreateMutation::Status422) {
        fixture.write_create_status("422");
        return;
    }

    let mut value = fixture.create_response();
    match mutation {
        CreateMutation::TransportError
        | CreateMutation::Status500
        | CreateMutation::Status422 => unreachable!(),
        CreateMutation::MissingId => {
            object_mut(&mut value).remove("id");
        }
        CreateMutation::ZeroId => value["id"] = json!(0),
        CreateMutation::FractionalId => value["id"] = json!(42.5),
        CreateMutation::StringId => value["id"] = json!("42"),
        CreateMutation::WrongTag => value["tag_name"] = json!("v9.9.9"),
        CreateMutation::WrongName => value["name"] = json!("wrong name"),
        CreateMutation::DraftFalse => value["draft"] = json!(false),
        CreateMutation::PrereleaseTrue => value["prerelease"] = json!(true),
        CreateMutation::BodyMismatch => value["body"] = json!("different notes\n"),
        CreateMutation::BodyNotString => value["body"] = json!({"text": NOTES}),
        CreateMutation::AssetsNonEmpty => value["assets"] = json!([{"id": 1}]),
        CreateMutation::AssetsNotArray => value["assets"] = json!({}),
        CreateMutation::EmptyTarget => value["target_commitish"] = json!(""),
        CreateMutation::MissingTarget => {
            object_mut(&mut value).remove("target_commitish");
        }
        CreateMutation::ApiUrlWrongRepository => {
            value["url"] = json!(
                "https://api.github.com/repos/other/repository/releases/42"
            )
        }
        CreateMutation::ApiUrlWrongId => {
            value["url"] = json!(
                "https://api.github.com/repos/fixture-owner/mysteries/releases/99"
            )
        }
        CreateMutation::UploadUrlWrongHost => {
            value["upload_url"] = json!(
                "https://api.github.com/repos/fixture-owner/mysteries/releases/42/assets{?name,label}"
            )
        }
        CreateMutation::UploadUrlWrongId => {
            value["upload_url"] = json!(
                "https://uploads.github.com/repos/fixture-owner/mysteries/releases/99/assets{?name,label}"
            )
        }
        CreateMutation::UploadUrlExtraQuery => {
            value["upload_url"] = json!(
                "https://uploads.github.com/repos/fixture-owner/mysteries/releases/42/assets{?name,label}&extra=1"
            )
        }
    }
    fixture.write_create_response(&value);
}

fn apply_upload_mutation(fixture: &Fixture, mutation: UploadMutation) -> usize {
    let mut uploads = fixture.upload_responses();
    let mut statuses = ["201", "201", "201"];
    let expected_calls = match mutation {
        UploadMutation::TransportError => {
            fixture.fail_upload_transport_at(1);
            1
        }
        UploadMutation::Status500 => {
            statuses[0] = "500";
            1
        }
        UploadMutation::ZeroId => {
            uploads[0]["id"] = json!(0);
            1
        }
        UploadMutation::FractionalId => {
            uploads[0]["id"] = json!(1001.5);
            1
        }
        UploadMutation::WrongName => {
            uploads[0]["name"] = json!("wrong-name.zip");
            1
        }
        UploadMutation::WrongState => {
            uploads[0]["state"] = json!("new");
            1
        }
        UploadMutation::WrongSize => {
            let size = uploads[0]["size"].as_u64().expect("upload size");
            uploads[0]["size"] = json!(size + 1);
            1
        }
        UploadMutation::WrongUrl => {
            uploads[0]["url"] =
                json!("https://api.github.com/repos/fixture-owner/mysteries/releases/assets/9999");
            1
        }
        UploadMutation::WrongDigest => {
            uploads[0]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
            1
        }
        UploadMutation::DuplicateId => {
            uploads[1]["id"] = uploads[0]["id"].clone();
            uploads[1]["url"] = uploads[0]["url"].clone();
            3
        }
        UploadMutation::PartialSecondFailure => {
            statuses[1] = "500";
            2
        }
    };
    fixture.write_upload_responses(&uploads);
    fixture.write_upload_statuses(&statuses);
    expected_calls
}

fn apply_draft_mutation(fixture: &Fixture, mutation: DraftMutation) {
    let mut draft = fixture.draft_response();
    match mutation {
        DraftMutation::WrongReleaseId => draft["id"] = json!(99),
        DraftMutation::WrongTag => draft["tag_name"] = json!("v9.9.9"),
        DraftMutation::DraftFalse => draft["draft"] = json!(false),
        DraftMutation::PrereleaseTrue => draft["prerelease"] = json!(true),
        DraftMutation::EmptyTarget => draft["target_commitish"] = json!(""),
        DraftMutation::BodyMismatch => draft["body"] = json!("different notes\n"),
        DraftMutation::AssetTupleMismatch => {
            draft["assets"][0]["id"] = json!(9001);
            draft["assets"][0]["url"] =
                json!("https://api.github.com/repos/fixture-owner/mysteries/releases/assets/9001");
        }
    }
    fixture.write_draft_response(&draft);
}

fn baseline_create_response() -> Value {
    json!({
        "id": RELEASE_ID,
        "tag_name": TAG,
        "name": format!("mysteries {TAG}"),
        "draft": true,
        "prerelease": false,
        "target_commitish": "fixture-candidate",
        "body": NOTES,
        "assets": [],
        "url": format!(
            "https://api.github.com/repos/{REPOSITORY}/releases/{RELEASE_ID}"
        ),
        "upload_url": format!(
            "https://uploads.github.com/repos/{REPOSITORY}/releases/{RELEASE_ID}/assets{{?name,label}}"
        )
    })
}

fn object_mut(value: &mut Value) -> &mut Map<String, Value> {
    value.as_object_mut().expect("fixture JSON object")
}

fn expect_success(step: &str, output: Output) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("{step}应成功: {}", output_details(&output)))
    }
}

fn expect_failure(step: &str, output: Output) -> Result<(), String> {
    if output.status.success() {
        Err(format!("{step}应fail-closed但返回成功"))
    } else {
        Ok(())
    }
}

fn output_details(output: &Output) -> String {
    format!(
        "status={:?}, stdout={:?}, stderr={:?}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn workflow_text() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml"))
        .expect("读取release workflow")
}

fn extract_step_script(workflow: &str, step_name: &str) -> String {
    let lines: Vec<&str> = workflow.lines().collect();
    let marker = format!("      - name: {step_name}");
    let step_index = lines
        .iter()
        .position(|line| *line == marker)
        .unwrap_or_else(|| panic!("workflow缺少step `{step_name}`"));
    let run_index = (step_index + 1..lines.len())
        .find(|index| lines[*index] == "        run: |" || lines[*index].starts_with("      - "))
        .filter(|index| lines[*index] == "        run: |")
        .unwrap_or_else(|| panic!("workflow step `{step_name}`缺少run body"));

    let mut script = Vec::new();
    for line in &lines[run_index + 1..] {
        if line.starts_with("      - ") {
            break;
        }
        if !line.is_empty() && !line.starts_with("          ") {
            break;
        }
        if line.is_empty() {
            script.push("");
        } else {
            script.push(
                line.strip_prefix("          ")
                    .unwrap_or_else(|| panic!("step `{step_name}`存在非法run缩进: {line:?}")),
            );
        }
    }
    let mut body = script.join("\n");
    body.push('\n');
    body
}

fn workflow_between_steps(workflow: &str, start: &str, end: &str) -> String {
    let start_marker = format!("      - name: {start}");
    let end_marker = format!("      - name: {end}");
    let start_index = workflow
        .find(&start_marker)
        .unwrap_or_else(|| panic!("workflow缺少step `{start}`"));
    let end_index = workflow[start_index..]
        .find(&end_marker)
        .map(|offset| start_index + offset)
        .unwrap_or_else(|| panic!("workflow缺少step `{end}`"));
    workflow[start_index..end_index].to_string()
}

fn step_block(workflow: &str, step_name: &str) -> String {
    let lines: Vec<&str> = workflow.lines().collect();
    let marker = format!("      - name: {step_name}");
    let start = lines
        .iter()
        .position(|line| *line == marker)
        .unwrap_or_else(|| panic!("workflow缺少step `{step_name}`"));
    let end = (start + 1..lines.len())
        .find(|index| lines[*index].starts_with("      - name: "))
        .unwrap_or(lines.len());
    lines[start..end].join("\n")
}

fn assert_step_has_lines(workflow: &str, step_name: &str, expected: &[&str]) {
    let block = step_block(workflow, step_name);
    for line in expected {
        assert!(
            block.lines().any(|actual| actual == *line),
            "workflow step `{step_name}`缺少精确wiring `{line}`"
        );
    }
}

fn assert_step_order(workflow: &str, step_names: &[&str]) {
    let mut previous = None;
    for step_name in step_names {
        let marker = format!("      - name: {step_name}");
        let index = workflow
            .find(&marker)
            .unwrap_or_else(|| panic!("workflow缺少step `{step_name}`"));
        if let Some((previous_name, previous_index)) = previous {
            assert!(
                previous_index < index,
                "workflow step顺序错误: `{previous_name}`必须早于`{step_name}`"
            );
        }
        previous = Some((*step_name, index));
    }
}

fn assert_no_network_tool_bypass(step_name: &str, script: &str) {
    for forbidden in [
        "command curl",
        "command gh",
        "env curl",
        "env gh",
        "curl.exe",
        "gh.exe",
    ] {
        assert!(
            !script.contains(forbidden),
            "{step_name} step绕过isolated network shim: `{forbidden}`"
        );
    }
    for line in script.lines() {
        let command = line.split_whitespace().next().unwrap_or("");
        let normalized = command.trim_matches(['\'', '"']).replace('\\', "/");
        assert!(
            !normalized.ends_with("/curl") && !normalized.ends_with("/gh"),
            "{step_name} step使用absolute network tool: `{command}`"
        );
    }
}

fn bash_with_required_tools() -> Option<BashToolchain> {
    let bash = find_bash();
    let Some(bash) = bash else {
        return skip_or_fail("未找到Bash，release workflow动态fixture未执行");
    };
    let query = format!(
        "for tool in {}; do command -v \"$tool\" || exit 1; done",
        REQUIRED_TOOLS.join(" ")
    );
    let output = Command::new(&bash)
        .args(["-lc", &query])
        .output()
        .expect("检查Bash fixture依赖");
    if !output.status.success() {
        return skip_or_fail(&format!(
            "Bash缺少jq/coreutils，release workflow动态fixture未执行: {}",
            output_details(&output)
        ));
    }
    let paths: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    if paths.len() != REQUIRED_TOOLS.len() {
        return skip_or_fail("Bash fixture依赖解析数量漂移");
    }
    let tools = REQUIRED_TOOLS
        .iter()
        .zip(paths)
        .map(|(name, path)| ResolvedTool { name, path })
        .collect();
    Some(BashToolchain { bash, tools })
}

fn skip_or_fail(message: &str) -> Option<BashToolchain> {
    if env::var_os("CI").is_some() {
        panic!("{message}；CI必须执行该fixture");
    }
    eprintln!("SKIP: {message}");
    None
}

fn find_bash() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(override_path) = env::var_os("MYSTERIES_TEST_BASH") {
        candidates.push(PathBuf::from(override_path));
    }
    #[cfg(windows)]
    {
        if let Some(program_files) = env::var_os("ProgramFiles") {
            candidates.push(PathBuf::from(program_files).join("Git/bin/bash.exe"));
        }
        if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
            candidates.push(PathBuf::from(program_files_x86).join("Git/bin/bash.exe"));
        }
        if let Some(path) = env::var_os("PATH") {
            candidates.extend(env::split_paths(&path).map(|entry| entry.join("bash.exe")));
        }
    }
    #[cfg(not(windows))]
    {
        candidates.extend([PathBuf::from("/usr/bin/bash"), PathBuf::from("/bin/bash")]);
        if let Some(path) = env::var_os("PATH") {
            candidates.extend(env::split_paths(&path).map(|entry| entry.join("bash")));
        }
    }

    let query = format!(
        "for tool in {}; do command -v \"$tool\" >/dev/null || exit 1; done",
        REQUIRED_TOOLS.join(" ")
    );
    candidates.into_iter().find_map(|candidate| {
        let candidate = fs::canonicalize(candidate).ok()?;
        let version_ok = Command::new(&candidate)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        (version_ok
            && Command::new(&candidate)
                .args(["-lc", &query])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false))
        .then_some(candidate)
    })
}

fn tool_env_name(name: &str) -> String {
    format!("FIXTURE_REAL_{}", name.to_ascii_uppercase())
}

fn write_tool_wrapper(root: &Path, name: &str) {
    let env_name = tool_env_name(name);
    let jq_binary = if name == "jq" {
        format!(
            "if [ \"${{FIXTURE_JQ_BINARY:-0}}\" = 1 ]; then\n  exec \"${{{env_name}:?}}\" --binary \"$@\"\nfi\n"
        )
    } else {
        String::new()
    };
    let script = format!("#!/bin/sh\nset -eu\n{jq_binary}exec \"${{{env_name}:?}}\" \"$@\"\n");
    let path = root.join("isolated-bin").join(name);
    fs::write(&path, script).unwrap_or_else(|error| panic!("写入{name} wrapper失败: {error}"));
    make_executable(&path);
}

fn write_network_deny_shim(root: &Path, name: &str) {
    let script =
        format!("#!/bin/sh\nprintf '%s\\n' 'isolated fixture拒绝绕过fake {name}' >&2\nexit 96\n");
    let path = root.join("isolated-bin").join(name);
    fs::write(&path, script).unwrap_or_else(|error| panic!("写入{name} deny shim失败: {error}"));
    make_executable(&path);
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).expect("读取wrapper权限").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("设置wrapper executable权限");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

fn read_lines_if_exists(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .map(|content| content.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

fn fake_tools_script() -> &'static str {
    r#"
curl() {
  local output=""
  local write_out=""
  local request=""
  local data_binary=""
  local url=""
  local silent=0
  local show_error=0
  local output_count=0
  local write_out_count=0
  local request_count=0
  local data_binary_count=0
  local -a headers=()
  while (($#)); do
    case "$1" in
      --output)
        (($# >= 2)) || return 94
        output_count=$((output_count + 1))
        output="$2"
        shift 2
        ;;
      --write-out)
        (($# >= 2)) || return 94
        write_out_count=$((write_out_count + 1))
        write_out="$2"
        shift 2
        ;;
      --request)
        (($# >= 2)) || return 94
        request_count=$((request_count + 1))
        request="$2"
        shift 2
        ;;
      --header)
        (($# >= 2)) || return 94
        headers+=("$2")
        shift 2
        ;;
      --data-binary)
        (($# >= 2)) || return 94
        data_binary_count=$((data_binary_count + 1))
        data_binary="$2"
        shift 2
        ;;
      --silent)
        silent=$((silent + 1))
        shift
        ;;
      --show-error)
        show_error=$((show_error + 1))
        shift
        ;;
      *)
        [[ -z "$url" ]] || return 94
        url="$1"
        shift
        ;;
    esac
  done
  printf '%s\n' "$url" >> "$FAKE_CURL_LOG" || return 94

  [[ "$silent" -eq 1 ]] || return 94
  [[ "$show_error" -eq 1 ]] || return 94
  [[ "$output_count" -eq 1 ]] || return 94
  [[ "$write_out_count" -eq 1 ]] || return 94
  [[ "$request_count" -eq 1 ]] || return 94
  [[ "$data_binary_count" -eq 1 ]] || return 94
  [[ "$write_out" == "%{http_code}" ]] || return 94
  [[ "$request" == "POST" ]] || return 94
  [[ "${#headers[@]}" -eq 5 ]] || return 94
  [[ "${headers[0]}" == "Accept: application/vnd.github+json" ]] || return 94
  [[ "${headers[1]}" == "Authorization: Bearer fixture-token-not-a-credential" ]] || return 94
  [[ "${headers[2]}" == "User-Agent: mysteries-release-workflow" ]] || return 94
  [[ "${headers[3]}" == "X-GitHub-Api-Version: 2022-11-28" ]] || return 94

  if [[ "$url" == "https://api.github.com/repos/$GITHUB_REPOSITORY/releases" ]]; then
    [[ "${headers[4]}" == "Content-Type: application/json" ]] || return 94
    [[ "$output" == "$RUNNER_TEMP/create-release.json" ]] || return 94
    [[ "$data_binary" == "@$RUNNER_TEMP/create-release-request.json" ]] || return 94
    [[ -f "${data_binary#@}" ]] || return 94
    jq -e \
      --arg tag "$TAG" \
      --arg name "mysteries $TAG" \
      --rawfile body bundle/release-notes.md '
        type == "object" and
        (keys == [
          "body",
          "draft",
          "generate_release_notes",
          "name",
          "prerelease",
          "tag_name"
        ]) and
        .tag_name == $tag and
        .name == $name and
        .body == $body and
        .draft == true and
        .prerelease == false and
        .generate_release_notes == false
      ' "${data_binary#@}" >/dev/null || return 94
    if [[ -f "$FAKE_CREATE_TRANSPORT_EXIT" ]]; then
      local transport_exit
      read -r transport_exit < "$FAKE_CREATE_TRANSPORT_EXIT" || return 94
      return "$transport_exit"
    fi
    [[ -f "$FAKE_CREATE_RESPONSE" ]] || return 94
    [[ -f "$FAKE_CREATE_STATUS" ]] || return 94
    cp "$FAKE_CREATE_RESPONSE" "$output" || return 94
    cat "$FAKE_CREATE_STATUS" || return 94
    return 0
  fi

  if [[ "$url" == https://uploads.github.com/* ]]; then
    local count=0
    local asset=""
    if [[ -f "$FAKE_UPLOAD_COUNT" ]]; then
      read -r count < "$FAKE_UPLOAD_COUNT" || return 94
    fi
    count=$((count + 1))
    printf '%s\n' "$count" > "$FAKE_UPLOAD_COUNT" || return 94
    case "$count" in
      1) asset="$WINDOWS_ASSET" ;;
      2) asset="$LINUX_ASSET" ;;
      3) asset="SHA256SUMS" ;;
      *) return 95 ;;
    esac
    [[ "${headers[4]}" == "Content-Type: application/octet-stream" ]] || return 94
    [[ "$url" == "https://uploads.github.com/repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID/assets?name=$asset" ]] || return 94
    [[ "$output" == "$RUNNER_TEMP/upload-responses/$asset.json" ]] || return 94
    [[ "$data_binary" == "@bundle/$asset" ]] || return 94
    [[ -f "${data_binary#@}" ]] || return 94
    if [[ -f "$FAKE_UPLOAD_TRANSPORT_AT" ]]; then
      local transport_at
      read -r transport_at < "$FAKE_UPLOAD_TRANSPORT_AT" || return 94
      if [[ "$count" -eq "$transport_at" ]]; then
        return 56
      fi
    fi
    [[ -f "$FAKE_UPLOAD_DIR/$count.json" ]] || return 94
    [[ -f "$FAKE_UPLOAD_DIR/$count.status" ]] || return 94
    cp "$FAKE_UPLOAD_DIR/$count.json" "$output" || return 94
    cat "$FAKE_UPLOAD_DIR/$count.status" || return 94
    return 0
  fi

  return 97
}

gh() {
  printf '%s\n' "$*" >> "$FAKE_GH_LOG" || return 94
  if [[ " $* " == *" --paginate "* || "$*" == *"releases?"* ]]; then
    return 91
  fi
  if [[ "${1:-}" != "api" ]]; then
    return 92
  fi
  shift
  if [[ "$#" -eq 1 &&
        "$1" == "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID" ]]; then
    [[ -f "$FAKE_DRAFT_RESPONSE" ]] || return 94
    cat "$FAKE_DRAFT_RESPONSE" || return 94
    return 0
  fi
  return 93
}
"#
}
