use mysteries::cli::{
    help_text, run_auth_list, run_auth_login_interactive, run_auth_logout_interactive, run_cli,
    version_text, wants_help, wants_version, AuthPaths, CliError, CliPaths,
};
use mysteries::tui::{run_tui, startup_mode};
use std::env;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> Result<(), CliError> {
    let mut resume = false;
    let mut continue_ = false;
    let args = env::args()
        .skip(1)
        .filter_map(|arg| {
            if arg == "--resume" {
                resume = true;
                None
            } else if arg == "--continue" {
                continue_ = true;
                None
            } else {
                Some(arg)
            }
        })
        .collect::<Vec<_>>();

    if wants_help(&args) {
        print!("{}", help_text());
        return Ok(());
    }
    if wants_version(&args) {
        println!("{}", version_text());
        return Ok(());
    }

    let paths = default_paths()?;

    if args.first().map(String::as_str) == Some("auth") {
        let auth_paths = AuthPaths {
            user_config: paths.user_config,
            credentials: paths.credentials,
        };
        return match args.get(1).map(String::as_str) {
            Some("list") => run_auth_list(&auth_paths),
            Some("login") => run_auth_login_interactive(&auth_paths),
            Some("logout") => run_auth_logout_interactive(&auth_paths),
            Some(_) | None => {
                print_auth_help();
                Ok(())
            }
        }
        .map_err(Into::into);
    }

    if args.iter().any(|arg| arg == "--headless") {
        let prompt = read_prompt(&args)?;
        run_cli(paths, &prompt).await
    } else {
        run_tui(paths, startup_mode(resume, continue_)).await
    }
}

fn read_prompt(args: &[String]) -> io::Result<String> {
    let prompt_args = args
        .iter()
        .filter(|arg| arg.as_str() != "--headless")
        .cloned()
        .collect::<Vec<_>>();
    if !prompt_args.is_empty() {
        return Ok(prompt_args.join(" "));
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
        config_dir,
        cwd,
    })
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

fn print_auth_help() {
    eprintln!("mysteries auth — manage providers and credentials");
    eprintln!("Commands:");
    eprintln!("  auth list      list configured providers and credentials");
    eprintln!("  auth login     log in to a provider");
    eprintln!("  auth logout    log out from a configured provider");
}
