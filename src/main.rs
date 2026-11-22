use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    sync::OnceLock,
};

use clap::{Arg, ArgMatches, Command};

use reqwest::Client;

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq)]
enum Lang {
    Zh,
    En,
}

impl Lang {
    fn parse(s: &str) -> Option<Lang> {
        match s.to_ascii_lowercase().as_str() {
            "zh" => Some(Lang::Zh),
            "en" => Some(Lang::En),
            _ => None,
        }
    }

    /// Scan raw args for a `--lang`/`-L` override so help is localized too.
    fn from_args() -> Option<Lang> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if let Some(value) = arg.strip_prefix("--lang=") {
                return Lang::parse(value);
            }
            if let Some(value) = arg.strip_prefix("-L=") {
                return Lang::parse(value);
            }
            if arg == "--lang" || arg == "-L" {
                if let Some(value) = iter.next() {
                    return Lang::parse(value);
                }
            }
        }
        None
    }

    fn from_env() -> Lang {
        for key in ["GIT_TOOL_LANG", "LANGUAGE", "LC_ALL", "LANG"] {
            if let Ok(value) = std::env::var(key) {
                if let Some(lang) = Lang::parse(&value) {
                    return lang;
                }
                if value.to_ascii_lowercase().contains("zh") {
                    return Lang::Zh;
                }
            }
        }
        Lang::En
    }
}

static LANG: OnceLock<Lang> = OnceLock::new();

fn current_lang() -> Lang {
    *LANG.get().unwrap_or(&Lang::En)
}

fn translate(key: &str) -> &'static str {
    let (zh, en) = match key {
        "about" => ("简易git工具", "Simple git tool"),
        "lang_help" => (
            "语言: zh(中文) 或 en(英文), 可选, 默认自动检测",
            "Language: zh (Chinese) or en (English), optional, default: auto-detect",
        ),
        "pull_about" => ("批量更新指定分支代码", "Batch update specified branch code"),
        "checkout_about" => ("批量切换分支", "Batch switch branches"),
        "init_about" => (
            "登录gitlab,拉取指定目录下的所有项目的指定分支",
            "Log in to GitLab and clone specified branches of all projects in a group",
        ),
        "directory_help" => (
            "指定项目文件夹的父文件夹(可选,默认当前文件夹)",
            "Parent folder of project folders (optional, default: current folder)",
        ),
        "branches_pull_help" => (
            "指定要更新的分支名称,多个分支用英文逗号分割(可选,默认master)",
            "Branch names to update, comma-separated (optional, default: master)",
        ),
        "branches_init_help" => (
            "克隆后要切换的分支名称,多个分支用英文逗号分割(可选,默认master)",
            "Branches to check out after cloning, comma-separated (optional, default: master)",
        ),
        "gitlab_base_help" => (
            "gitlab地址(未指定时读取环境变量或配置文件中的GITLAB_BASE_URL)",
            "GitLab base URL (falls back to GITLAB_BASE_URL env var or config file)",
        ),
        "gitlab_repo_group_help" => (
            "gitlab组ID(每次init时手动指定,不写入配置文件)",
            "GitLab group ID (specify manually for each init; not read from config)",
        ),
        "gitlab_access_token_help" => (
            "gitlab个人访问令牌(可选,未指定时读取环境变量或配置文件)",
            "GitLab personal access token (optional, falls back to env var or config file)",
        ),
        "env_file_help" => (
            "配置文件路径(默认 ~/.git-tool-env)",
            "Config file path (default: ~/.git-tool-env)",
        ),
        "profile_help" => (
            "配置分组名称(对应配置文件中的[分组名],多个分组时必填)",
            "Config profile name (matches [section] in the config file; required when multiple exist)",
        ),
        "init_gitlab" => ("初始化 GitLab: {}", "Initializing GitLab: {}"),
        "init_gitlab_group" => ("初始化 GitLab 组: {}", "Initializing GitLab group: {}"),
        "init_gitlab_token" => (
            "初始化 GitLab 访问令牌: {}",
            "Initializing GitLab access token: {}",
        ),
        "init_profile" => ("使用配置分组: {}", "Using config profile: {}"),
        "setting_git_identity" => (
            "设置仓库提交作者: {} <{}>",
            "Setting repo commit identity: {} <{}>",
        ),
        "request_url" => ("请求 URL: {}", "Request URL: {}"),
        "http_status" => ("HTTP 状态: {}", "HTTP status: {}"),
        "response_body" => ("响应内容: {}", "Response body: {}"),
        "cloning_repo" => ("拉取仓库: {}", "Cloning repo: {}"),
        "init_done" => ("所有仓库拉取完成！", "All repositories cloned!"),
        "updating_branches" => ("更新分支: {}", "Updating branches: {}"),
        "updating_directory" => ("更新目录: {}", "Updating directory: {}"),
        "checking_repo" => ("检查仓库: {}", "Checking repo: {}"),
        "not_git_repo" => ("仓库 {} 不是git仓库", "{} is not a git repository"),
        "branch_not_found" => (
            "仓库 {} 没有找到 {} 分支",
            "Repo {} does not have branch {}",
        ),
        "checking_branch" => ("检查分支: {}", "Checking branch: {}"),
        "running_git" => (
            "运行 git 命令: git {} in {}",
            "Running git command: git {} in {}",
        ),
        "git_done" => (
            "git {} 执行完成, 退出状态码: {}",
            "git {} finished, exit code: {}",
        ),
        "err_setting_missing" => (
            "未找到配置 {}: 命令行/环境变量未提供, 配置文件 {} 中也没有。可用分组: {} (多个分组时用 -p 指定)",
            "Missing {}: not provided via CLI/env vars or config file {}. Available profiles: {} (use -p when multiple exist)",
        ),
        "err_profile_not_found" => (
            "配置文件中不存在分组 [{}]: {}。可用分组: {}",
            "Profile [{}] not found in config file {}. Available profiles: {}",
        ),
        "no_profiles" => ("无", "none"),
        "err_env_file_read" => (
            "读取配置文件 {} 失败: {}",
            "Failed to read config file {}: {}",
        ),
        "err_empty_profile" => (
            "配置文件第 {} 行的分组名为空",
            "Empty profile name at config file line {}",
        ),
        "err_no_command" => (
            "请指定要执行的子命令: init、pull 或 checkout",
            "Please specify a subcommand: init, pull, or checkout",
        ),
        "err_directory" => (
            "请输入项目存储目录",
            "Please provide the project storage directory",
        ),
        "err_client" => (
            "初始化 HTTP 客户端失败: {}",
            "Failed to initialize the HTTP client: {}",
        ),
        "err_request" => (
            "请求 GitLab 失败: {}。请检查网络连接、GitLab 地址和代理设置。",
            "Failed to request GitLab: {}. Check your network, GitLab URL, and proxy settings.",
        ),
        "err_read_body" => (
            "读取 GitLab 响应失败: {}",
            "Failed to read the GitLab response: {}",
        ),
        "err_response_not_json" => (
            "GitLab 返回的内容不是有效 JSON (HTTP {}): {}。请确认地址和令牌是否正确。",
            "GitLab returned invalid JSON (HTTP {}): {}. Check the URL and access token.",
        ),
        "err_parse_projects" => (
            "GitLab 返回的项目列表格式不正确: {}",
            "The project list returned by GitLab has an invalid format: {}",
        ),
        "err_gitlab_api" => (
            "GitLab 接口返回错误 (HTTP {}): {}。请检查访问令牌、组 ID 和 GitLab 地址。",
            "GitLab API error (HTTP {}): {}. Check the access token, group ID, and GitLab URL.",
        ),
        "err_read_dir" => ("读取目录 {} 失败: {}", "Failed to read directory {}: {}"),
        "err_read_entry" => ("读取目录项失败: {}", "Failed to read a directory entry: {}"),
        "err_git_branch" => (
            "无法执行 git branch -a: {}。请确认已安装 git 并在 PATH 中。",
            "Failed to run git branch -a: {}. Make sure git is installed and on PATH.",
        ),
        "err_git_branch_status" => (
            "git branch -a 在 {} 执行失败: {}",
            "git branch -a failed in {}: {}",
        ),
        "err_run_git" => (
            "无法启动 git 命令: {}。请确认已安装 git 并在 PATH 中。",
            "Failed to start git: {}. Make sure git is installed and on PATH.",
        ),
        "err_wait_git" => (
            "等待 git 命令结束失败: {}",
            "Failed to wait for the git command: {}",
        ),
        _ => ("", ""),
    };
    match current_lang() {
        Lang::Zh => zh,
        Lang::En => en,
    }
}

fn tr_fmt(template: &str, args: &[String]) -> String {
    let mut out = template.to_string();
    for arg in args {
        out = out.replacen("{}", arg, 1);
    }
    out
}

/// 输出到 stdout；写入失败（例如管道已关闭）时忽略，不中断命令。
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

/// 输出到 stderr；写入失败时忽略，不中断命令。
macro_rules! errln {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

/// 输出用户可读的错误信息并以非零状态码退出。
///
/// 错误统一写到 stderr，便于脚本区分正常输出与失败。
fn fail(message: impl AsRef<str>) -> ! {
    errln!("❌ {}", message.as_ref());
    std::process::exit(1);
}

macro_rules! tr {
    ($key:literal) => {
        translate($key).to_string()
    };
    ($key:literal, $($arg:expr),+ $(,)?) => {
        tr_fmt(translate($key), &[$($arg.to_string()),+])
    };
}

#[tokio::main]
async fn main() {
    let lang = Lang::from_args().unwrap_or_else(Lang::from_env);
    let _ = LANG.set(lang);

    let matches = build_cli().get_matches();

    match matches.subcommand() {
        Some(("pull", sub_matches)) => {
            update_repo(sub_matches).await;
        }
        Some(("init", sub_matches)) => {
            init_repo(sub_matches).await;
        }
        Some(("checkout", sub_matches)) => {
            update_repo(sub_matches).await;
        }
        _ => fail(tr!("err_no_command")),
    }
}

fn build_cli() -> Command {
    Command::new("git-tool")
        .version("1.0")
        .author("xgeorge")
        .about(tr!("about"))
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("lang")
                .short('L')
                .long("lang")
                .global(true)
                .value_parser(["zh", "en"])
                .help(tr!("lang_help")),
        )
        .subcommand(
            Command::new("pull")
                .about(tr!("pull_about"))
                .arg(
                    Arg::new("directory")
                        .short('d')
                        .long("directory")
                        .default_value(".")
                        .help(tr!("directory_help")),
                )
                .arg(
                    Arg::new("branches")
                        .short('b')
                        .long("branches")
                        .help(tr!("branches_pull_help"))
                        .value_delimiter(',')
                        .default_value("master"),
                ),
        )
        .subcommand(
            Command::new("checkout")
                .about(tr!("checkout_about"))
                .arg(
                    Arg::new("directory")
                        .short('d')
                        .long("directory")
                        .default_value(".")
                        .help(tr!("directory_help")),
                )
                .arg(
                    Arg::new("branches")
                        .short('b')
                        .long("branches")
                        .help(tr!("branches_pull_help"))
                        .value_delimiter(',')
                        .default_value("master"),
                ),
        )
        .subcommand(
            Command::new("init")
                .about(tr!("init_about"))
                .arg(
                    Arg::new("profile")
                        .short('p')
                        .long("profile")
                        .help(tr!("profile_help"))
                        // 支持通过环境变量选择分组，便于在脚本/CI 中切换身份。
                        .env("GIT_TOOL_PROFILE"),
                )
                .arg(
                    Arg::new("gitlab_base")
                        .short('l')
                        .long("gitlab_base")
                        .help(tr!("gitlab_base_help"))
                        // 兼容旧用法：命令行显式指定时优先级最高；
                        // 未指定时从进程环境变量或环境文件读取。
                        .env("GITLAB_BASE_URL"),
                )
                .arg(
                    Arg::new("gitlab_repo_group")
                        .short('g')
                        .long("gitlab_repo_group")
                        .help(tr!("gitlab_repo_group_help"))
                        // 组 ID 每次 init 的目标都可能不同，必须由用户实时指定，
                        // 因此不提供环境变量或配置文件回退，避免误用上次的组。
                        .required(true),
                )
                .arg(
                    Arg::new("gitlab_access_token")
                        .short('k')
                        .long("gitlab_access_token")
                        .help(tr!("gitlab_access_token_help"))
                        // 兼容旧用法：命令行显式指定时优先级最高；
                        // 未指定时从进程环境变量或环境文件读取。
                        .env("GITLAB_ACCESS_TOKEN"),
                )
                .arg(
                    Arg::new("env_file")
                        .short('e')
                        .long("env-file")
                        .help(tr!("env_file_help")),
                )
                .arg(
                    Arg::new("directory")
                        .short('d')
                        .long("directory")
                        .help(tr!("directory_help"))
                        .default_value("."),
                )
                .arg(
                    Arg::new("branches")
                        .short('b')
                        .long("branches")
                        .help(tr!("branches_init_help"))
                        .value_delimiter(',')
                        .default_value("master"),
                ),
        )
}

#[derive(Deserialize)]
struct Project {
    name: String,
    description: Option<String>,
    path_with_namespace: String,
    ssh_url_to_repo: String,
    http_url_to_repo: String,
}

/// 默认的令牌配置文件路径：用户主目录下的 `.git-tool-env`。
/// Unix 使用 HOME，Windows 使用 USERPROFILE；两者都缺失时回退到当前目录，
/// 以便后续报出明确的“文件不存在”错误。
fn default_env_file() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".git-tool-env")
}

/// 配置文件内容：分组前的键为顶层共享配置，`[分组名]` 下的键归入对应分组。
#[derive(Default)]
struct ConfigFile {
    shared: HashMap<String, String>,
    profiles: HashMap<String, HashMap<String, String>>,
}

impl ConfigFile {
    /// 生成错误提示用的分组列表；无分组时返回本地化的“无”。
    /// 排序是为了让同一份配置每次提示的顺序稳定，便于用户对照。
    fn profile_names(&self) -> String {
        if self.profiles.is_empty() {
            return tr!("no_profiles");
        }
        let mut names: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
        names.sort_unstable();
        names.join(", ")
    }
}

/// 读取并选择分组后的配置。
struct AppConfig {
    path: PathBuf,
    file: ConfigFile,
    profile: Option<String>,
}

impl AppConfig {
    /// 读取配置文件并确定当前分组。
    ///
    /// 分组选择规则：
    /// - `-p/--profile` 或环境变量 `GIT_TOOL_PROFILE` 显式指定时优先；
    /// - 未指定且文件中只有一个分组时自动使用该分组；
    /// - 存在多个分组但未指定时保持“未选择”，只有真正需要文件中的值时才
    ///   在错误信息里提示用户用 `-p` 指定，避免影响纯命令行用法。
    fn load(matches: &ArgMatches) -> AppConfig {
        let path = env_file_path(matches);
        // 文件不存在时按空配置处理，让错误信息聚焦到“缺少哪个配置”；
        // 其他读取错误（如权限不足、格式错误）直接报错，避免掩盖真实问题。
        let file = match read_config_file(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ConfigFile::default(),
            Err(e) => fail(tr!("err_env_file_read", path.display(), e)),
        };

        let requested = matches
            .get_one::<String>("profile")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());

        let profile = match requested {
            Some(name) => {
                if !file.profiles.contains_key(&name) {
                    // 指定了不存在的分组属于明确错误，直接终止并列出可用分组。
                    let message = tr_fmt(
                        translate("err_profile_not_found"),
                        &[name, path.display().to_string(), file.profile_names()],
                    );
                    fail(message);
                }
                Some(name)
            }
            None if file.profiles.len() == 1 => file.profiles.keys().next().cloned(),
            None => None,
        };

        AppConfig {
            path,
            file,
            profile,
        }
    }

    /// 按“当前分组 > 顶层共享配置”的顺序读取配置项。
    /// 分组内没有的键会回退到顶层，方便把提交身份等公共项只写一次。
    fn get(&self, key: &str) -> Option<&str> {
        self.profile
            .as_ref()
            .and_then(|name| self.file.profiles.get(name))
            .and_then(|values| values.get(key))
            .or_else(|| self.file.shared.get(key))
            .map(String::as_str)
    }
}

/// 读取 INI 风格的分组配置文件。
///
/// 语法示例：
/// ```ini
/// GITLAB_COMMIT_USER_NAME=公共作者
///
/// [work]
/// GITLAB_BASE_URL=https://gitlab.example.com
/// GITLAB_ACCESS_TOKEN=xxx
/// ```
///
/// 规则：
/// - `[分组名]` 开始一个分组，分组前的键作为顶层共享配置；
/// - 忽略空行和整行 `#` 注释，兼容 `export KEY=VALUE`；
/// - 去掉值两端的单/双引号，不做变量展开；
/// - 同名键后出现的覆盖先出现的，同名分组会合并。
fn read_config_file(path: &Path) -> std::io::Result<ConfigFile> {
    let content = fs::read_to_string(path)?;
    let mut file = ConfigFile::default();
    let mut current: Option<String> = None;

    for (index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // 分组头必须独占一行，形如 `[work]`。
        if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = inner.trim();
            if name.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    tr!("err_empty_profile", index + 1),
                ));
            }
            file.profiles.entry(name.to_string()).or_default();
            current = Some(name.to_string());
            continue;
        }

        // 兼容 shell 中常见的 `export KEY=VALUE` 写法，方便用户直接 source 该文件。
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }

        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value)
            .to_string();

        match &current {
            Some(name) => {
                // 正常流程中分组头已插入；用 entry 兼容重复分组头等边界情况。
                file.profiles
                    .entry(name.clone())
                    .or_default()
                    .insert(key.to_string(), value);
            }
            None => {
                file.shared.insert(key.to_string(), value);
            }
        }
    }

    Ok(file)
}

/// 令牌展示脱敏：只保留前 4 位和后 3 位，避免日志/终端记录完整凭证。
/// 长度不足 8 位时无法安全保留片段，整体以 `****` 代替。
fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() < 8 {
        return "****".to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 3..].iter().collect();
    format!("{}***{}", head, tail)
}

/// 配置文件路径：`-e/--env-file` 优先，否则使用用户目录的默认路径。
fn env_file_path(matches: &ArgMatches) -> PathBuf {
    matches
        .get_one::<String>("env_file")
        .map(PathBuf::from)
        .unwrap_or_else(default_env_file)
}

/// 按优先级解析必填配置：
/// 1. 命令行参数
/// 2. 进程环境变量（由 clap 的 `.env(...)` 自动注入为参数值）
/// 3. 当前分组，最后回退到顶层共享配置
///
/// 都缺失时直接终止程序，避免带着空值继续请求导致难以理解的错误。
fn resolve_required(
    matches: &ArgMatches,
    config: &AppConfig,
    arg_id: &str,
    env_key: &str,
) -> String {
    if let Some(value) = matches
        .get_one::<String>(arg_id)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        return value.to_string();
    }

    if let Some(value) = config.get(env_key).map(str::trim).filter(|v| !v.is_empty()) {
        return value.to_string();
    }

    let message = tr_fmt(
        translate("err_setting_missing"),
        &[
            env_key.to_string(),
            config.path.display().to_string(),
            config.file.profile_names(),
        ],
    );
    fail(message)
}

/// 解析可选配置：进程环境变量优先，其次是当前分组/顶层共享配置。
fn resolve_optional(config: &AppConfig, env_key: &str) -> Option<String> {
    std::env::var(env_key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            config
                .get(env_key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
}

/// 解析 GitLab 实例地址，来源优先级见 [`resolve_required`]。
fn resolve_gitlab_base(matches: &ArgMatches, config: &AppConfig) -> String {
    resolve_required(matches, config, "gitlab_base", "GITLAB_BASE_URL")
}

/// 解析 GitLab 访问令牌，来源优先级见 [`resolve_required`]。
fn resolve_access_token(matches: &ArgMatches, config: &AppConfig) -> String {
    resolve_required(
        matches,
        config,
        "gitlab_access_token",
        "GITLAB_ACCESS_TOKEN",
    )
}

async fn init_repo(sub_matches: &ArgMatches) {
    // 配置只读取一次，后续所有键都从同一份快照解析，避免文件在运行中被修改
    // 导致不同配置项来自不同分组。
    let config = AppConfig::load(sub_matches);
    let gitlab_base = resolve_gitlab_base(sub_matches, &config);
    // 组 ID 只能由命令行实时传入，clap 的 required 已保证此处一定有值。
    let gitlab_repo_group = sub_matches
        .get_one::<String>("gitlab_repo_group")
        .unwrap_or_else(|| fail(tr!("err_gitlab_group")));
    let gitlab_access_token = resolve_access_token(sub_matches, &config);
    let commit_user_name = resolve_optional(&config, "GITLAB_COMMIT_USER_NAME");
    let commit_user_email = resolve_optional(&config, "GITLAB_COMMIT_USER_EMAIL");
    let directory = sub_matches
        .get_one::<String>("directory")
        .unwrap_or_else(|| fail(tr!("err_directory")));
    // 克隆成功后逐个切换分支；-b 支持逗号分隔多个分支。
    let branches: Vec<String> = sub_matches
        .get_many::<String>("branches")
        .unwrap_or_default()
        .map(|branch| branch.to_string())
        .collect();
    if let Some(profile) = &config.profile {
        outln!("{}", tr!("init_profile", profile));
    }
    outln!("{}", tr!("init_gitlab", gitlab_base));
    outln!("{}", tr!("init_gitlab_group", gitlab_repo_group));
    // 日志中只显示脱敏后的令牌，防止完整凭证进入终端记录或 CI 日志。
    outln!(
        "{}",
        tr!("init_gitlab_token", mask_token(&gitlab_access_token))
    );

    let api_url = format!(
        "{}/api/v4/groups/{}/projects",
        gitlab_base, gitlab_repo_group
    );

    // 使用可返回错误的客户端构建方式；初始化失败时给出明确提示。
    let client = Client::builder()
        .build()
        .unwrap_or_else(|e| fail(tr!("err_client", e)));
    outln!("{}", tr!("request_url", api_url));
    let response = client
        .get(&api_url)
        .header("PRIVATE-TOKEN", gitlab_access_token.as_str())
        .send()
        .await
        .unwrap_or_else(|e| fail(tr!("err_request", e)));

    let status = response.status();
    outln!("{}", tr!("http_status", status));
    let body = response
        .text()
        .await
        .unwrap_or_else(|e| fail(tr!("err_read_body", e)));
    outln!("{}", tr!("response_body", body));

    // 非 JSON 响应（例如代理返回 HTML 错误页）时提示 HTTP 状态和解析错误，
    // 便于定位地址、令牌或代理配置问题。
    let res: Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| fail(tr!("err_response_not_json", status.as_u16(), e)));

    // GitLab 的常见错误响应形如 {"message": "401 Unauthorized"}，
    // 直接提取 message/error，避免只显示一句笼统的失败提示。
    if res.is_object() {
        if let Some(error) = res.get("message").or_else(|| res.get("error")) {
            let detail = error
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string());
            fail(tr!("err_gitlab_api", status.as_u16(), detail));
        }
    }

    let projects: Vec<Project> =
        serde_json::from_value(res).unwrap_or_else(|e| fail(tr!("err_parse_projects", e)));

    for project in projects {
        let repo_path = format!(
            "{}/{}",
            project.description.unwrap_or_default(),
            project.path_with_namespace
        );
        outln!("=========================================================");
        outln!("{}", tr!("cloning_repo", repo_path));
        let cloned = run_git(
            &std::path::PathBuf::from(directory.clone()),
            &["clone", &project.ssh_url_to_repo],
        );

        // 只有克隆成功后才设置本地提交身份；仓库已存在导致 clone 失败时不覆盖
        // 用户已有配置。身份只写入该仓库的 .git/config，不修改全局配置。
        if cloned {
            if let Some(repo_name) = repo_dir_name(&project.ssh_url_to_repo) {
                let repo_path = std::path::PathBuf::from(directory.as_str()).join(repo_name);
                apply_commit_identity(
                    &repo_path,
                    commit_user_name.as_deref(),
                    commit_user_email.as_deref(),
                );

                for branch in &branches {
                    run_git(&repo_path, &["checkout", branch.as_str()]);
                }
            }
        }
    }

    outln!("{}", tr!("init_done"));
}

async fn update_repo(sub_matches: &ArgMatches) {
    let directory = sub_matches
        .get_one::<String>("directory")
        .unwrap_or_else(|| fail(tr!("err_directory")));
    let branches = sub_matches
        .get_many::<String>("branches")
        .unwrap_or_default()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    outln!("{}", tr!("updating_branches", format!("{:?}", branches)));
    outln!("{}", tr!("updating_directory", directory));

    let repo_path = std::path::PathBuf::from(directory);

    let entries = fs::read_dir(&repo_path)
        .unwrap_or_else(|e| fail(tr!("err_read_dir", repo_path.display(), e)));

    for entry in entries {
        // 单个目录项读取失败不应中断整个批处理，提示后继续处理其他仓库。
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                errln!("⚠️  {}", tr!("err_read_entry", e));
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() && path.join(".git").exists() {
            outln!("=========================================================");
            outln!("{}", tr!("checking_repo", path.display()));
            process_repo(&path, &branches, &None);
        } else {
            outln!("{}", tr!("not_git_repo", path.display()));
        }
    }
}

fn process_repo(
    repo_path: &std::path::Path,
    target_branches: &Vec<String>,
    remote: &Option<String>,
) {
    let branches = get_branches(repo_path);

    for branch in target_branches {
        let short = branch.strip_prefix("origin/").unwrap_or(branch);
        if !branches.iter().any(|b| b.as_str() == short) {
            outln!("{}", tr!("branch_not_found", repo_path.display(), branch));
            continue;
        }
        outln!("{}", tr!("checking_branch", branch));

        run_git(repo_path, &["checkout", branch]);
        outln!("--------------------------------------------------------------");
        run_git(repo_path, &["pull", "origin", branch]);
        outln!("========================================================\r\n");
    }
}

fn get_branches(repo_path: &std::path::Path) -> Vec<String> {
    let output = ProcessCommand::new("git")
        .args(&["branch", "-a"])
        .current_dir(repo_path)
        .output()
        .unwrap_or_else(|e| fail(tr!("err_git_branch", e)));

    // 命令能启动但返回非零通常说明目录不是有效仓库或 git 权限异常；
    // 把 stderr 一起带上，避免用户只看到“失败”却不知道原因。
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        fail(tr!(
            "err_git_branch_status",
            repo_path.display(),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    stdout
        .lines()
        .filter_map(|line| {
            let name = line.trim().trim_start_matches("* ").trim();
            // Skip the remote HEAD symbolic ref ("remotes/origin/HEAD -> origin/master").
            // Normalize "remotes/origin/dev" to "dev" so it matches a plain -b dev request.
            if name.is_empty() || name.contains(" -> ") {
                return None;
            }
            Some(
                name.strip_prefix("remotes/origin/")
                    .unwrap_or(name)
                    .to_string(),
            )
        })
        .collect()
}

/// 从仓库 URL 推断 `git clone` 生成的目录名。
/// 同时兼容 `git@host:group/repo.git` 和 `https://host/group/repo.git`。
fn repo_dir_name(url: &str) -> Option<&str> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .map(|name| name.strip_suffix(".git").unwrap_or(name))
        .filter(|name| !name.is_empty())
}

/// 把分组中的提交身份写入指定仓库的本地配置。
///
/// 只修改该仓库的 `.git/config`，避免污染用户的全局 git 配置；
/// 名称和邮箱都是可选项，配置了哪个就设置哪个。
fn apply_commit_identity(repo_path: &Path, name: Option<&str>, email: Option<&str>) {
    if name.is_none() && email.is_none() {
        return;
    }

    outln!(
        "{}",
        tr!(
            "setting_git_identity",
            name.unwrap_or("-"),
            email.unwrap_or("-")
        )
    );

    if let Some(name) = name {
        run_git(repo_path, &["config", "user.name", name]);
    }
    if let Some(email) = email {
        run_git(repo_path, &["config", "user.email", email]);
    }
}

/// 运行 git 命令并返回是否成功，便于调用方只在成功时执行后续操作。
fn run_git(repo_path: &std::path::Path, args: &[&str]) -> bool {
    outln!(
        "{}",
        tr!("running_git", args.join(" "), repo_path.display())
    );

    // 启动失败（例如未安装 git）时提示并返回 false，
    // 让批处理按“该仓库失败”继续处理其他仓库。
    let mut child = match ProcessCommand::new("git")
        .args(args)
        .current_dir(repo_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            errln!("❌ {}", tr!("err_run_git", e));
            return false;
        }
    };

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(line) = line {
                outln!("📥 {}", line);
            }
        }
    }

    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(line) = line {
                errln!("⚠️  {}", line);
            }
        }
    }

    let status = match child.wait() {
        Ok(status) => status,
        Err(e) => {
            errln!("❌ {}", tr!("err_wait_git", e));
            return false;
        }
    };
    outln!(
        "✅ {}",
        tr!("git_done", args.join(" "), status.code().unwrap_or(-1))
    );
    status.success()
}
