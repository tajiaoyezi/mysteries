use mysteries::cli::{run_cli, CliError, CliPaths};
use std::env;
use std::io;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), CliError> {
    let prompt = read_prompt()?;
    let paths = default_paths()?;

    run_cli(paths, &prompt).await
}

fn read_prompt() -> io::Result<String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if !args.is_empty() {
        return Ok(args.join(" "));
    }

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    while input.ends_with('\n') || input.ends_with('\r') {
        input.pop();
    }

    Ok(input)
}

fn default_paths() -> io::Result<CliPaths> {
    let cwd = env::current_dir()?;
    let config_dir = home_dir()
        .map(|home| home.join(".config").join("mysteries"))
        .unwrap_or_else(|| cwd.join(".mysteries-missing-home"));

    Ok(CliPaths {
        user_config: config_dir.join("config.toml"),
        project_config: cwd.join("mysteries.toml"),
        credentials: config_dir.join("credentials"),
        cwd,
    })
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}
