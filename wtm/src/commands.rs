//! Running commands.

use anyhow::Result;
use wtm_core::config::Overrides;
use wtm_core::git::{Checkout, DefaultBackend, GitOutput, Head};
use wtm_core::model::{DropOptions, NewWorktree, Worktree, WorktreeState};
use wtm_core::{Config, Pool};

use crate::args::{Cli, Command, RemoteCommand};
use crate::{generate, shell, table};

pub fn run(cli: Cli) -> Result<()> {
    let overrides = Overrides {
        config_file: cli.global.config,
        repos_dir: cli.global.repos_dir,
        checkouts_dir: cli.global.checkouts_dir,
    };
    // Commands that need neither configuration nor git don't load them, so
    // they work even when those are broken.
    let config = || Config::load(&overrides);
    let pool = || -> Result<Pool> {
        let git = DefaultBackend::new(GitOutput::Inherit)?;
        Ok(Pool::new(config()?, git))
    };

    match cli.command {
        Command::Register {
            source,
            name,
            remote,
        } => register(&pool()?, &source, name.as_deref(), &remote),
        Command::Adopt {
            path,
            name,
            keep_path,
        } => {
            let original = path.canonicalize().unwrap_or_else(|_| path.clone());
            let adopted = pool()?.adopt(&path, name.as_deref(), keep_path)?;
            eprintln!(
                "Adopted '{}' as {} with worktree {}",
                adopted.repo.name,
                adopted.repo.git_dir.display(),
                adopted.path.display()
            );
            if let Ok(cwd) = std::env::current_dir()
                && cwd.starts_with(&original)
                && adopted.path != original
            {
                eprintln!("note: the current directory was moved; cd to the new path");
            }
            println!("{}", adopted.path.display());
            Ok(())
        }
        Command::Unregister { repo, force } => {
            let repo = pool()?.unregister(&repo, force)?;
            eprintln!(
                "Unregistered '{}' and deleted {}",
                repo.name,
                repo.git_dir.display()
            );
            Ok(())
        }
        Command::Repos => repos(&pool()?),
        Command::Remote(command) => remote(&pool()?, command),
        Command::Fetch { repos } => fetch(&pool()?, &repos),
        Command::New {
            repo,
            name,
            branch,
            from,
            remote,
            no_fetch,
        } => new(
            &pool()?,
            NewWorktree {
                repo,
                name,
                branch,
                from,
                remote,
                fetch: !no_fetch,
            },
        ),
        Command::List { repo, paths, names } => list(&pool()?, repo.as_deref(), paths, names),
        Command::Drop {
            name,
            force,
            keep_branch,
            fetch,
        } => drop(
            &pool()?,
            &name,
            DropOptions {
                keep_branch,
                force,
                fetch,
            },
        ),
        Command::Locate { name, repo } => {
            let path = match (name, repo) {
                (None, false) => config()?.checkouts_dir.value,
                (None, true) => config()?.repos_dir.value,
                (Some(name), false) => pool()?.worktree(&name)?.path,
                (Some(name), true) => pool()?.repo(&name)?.git_dir,
            };
            println!("{}", path.display());
            Ok(())
        }
        Command::Repair => repair(&pool()?),
        Command::Config => show_config(&config()?),
        Command::ShellInit { shell, cd_command } => shell::init(shell, &cd_command),
        Command::Generate(what) => generate::run(what),
    }
}

fn register(pool: &Pool, source: &str, name: Option<&str>, remote: &str) -> Result<()> {
    let registered = pool.register(source, name, remote)?;
    for other in &registered.same_url {
        eprintln!(
            "warning: {} is also registered as '{other}'",
            registered.url
        );
    }
    eprintln!(
        "Registered '{}' from {}",
        registered.repo.name, registered.url
    );
    match &registered.default_branch {
        Some(branch) => eprintln!("Default branch: {branch}"),
        None => eprintln!("warning: the upstream has no default branch; is it empty?"),
    }
    Ok(())
}

fn repos(pool: &Pool) -> Result<()> {
    let mut rows = vec![vec![
        "NAME".to_string(),
        "BRANCH".to_string(),
        "REMOTE".to_string(),
        "URL".to_string(),
    ]];
    for repo in pool.repos()? {
        let details = pool.repo_details(&repo)?;
        let branch = details.default_branch.clone().unwrap_or("-".into());
        if details.remotes.is_empty() {
            rows.push(vec![repo.name, branch, "-".into(), "-".into()]);
            continue;
        }
        for (i, remote) in details.remotes.iter().enumerate() {
            let (name, branch) = if i == 0 {
                (repo.name.clone(), branch.clone())
            } else {
                (String::new(), String::new())
            };
            rows.push(vec![
                name,
                branch,
                remote_label(&remote.name, details.default_remote.as_deref()),
                remote.url.clone().unwrap_or_default(),
            ]);
        }
    }
    print!("{}", table::format(&rows));
    Ok(())
}

fn remote_label(name: &str, default: Option<&str>) -> String {
    if Some(name) == default {
        format!("{name}*")
    } else {
        name.to_string()
    }
}

fn remote(pool: &Pool, command: RemoteCommand) -> Result<()> {
    match command {
        RemoteCommand::List { repo } => {
            let details = pool.repo_details(&pool.repo(&repo)?)?;
            let mut rows = vec![vec!["NAME".to_string(), "URL".to_string()]];
            for remote in &details.remotes {
                let mut url = remote.url.clone().unwrap_or_default();
                if let Some(push) = remote.push_url.as_ref().filter(|p| **p != url) {
                    url = format!("{url} (push: {push})");
                }
                rows.push(vec![
                    remote_label(&remote.name, details.default_remote.as_deref()),
                    url,
                ]);
            }
            print!("{}", table::format(&rows));
        }
        RemoteCommand::Add {
            repo,
            name,
            url,
            no_fetch,
            default,
        } => {
            let repo = pool.repo(&repo)?;
            pool.add_remote(&repo, &name, &url, !no_fetch)?;
            eprintln!("Added remote '{name}' to '{}'", repo.name);
            if default {
                pool.set_default_remote(&repo, &name)?;
                eprintln!("Default remote of '{}' is now '{name}'", repo.name);
            }
        }
        RemoteCommand::Remove { repo, name } => {
            let repo = pool.repo(&repo)?;
            pool.remove_remote(&repo, &name)?;
            eprintln!("Removed remote '{name}' from '{}'", repo.name);
        }
        RemoteCommand::SetDefault { repo, name } => {
            let repo = pool.repo(&repo)?;
            pool.set_default_remote(&repo, &name)?;
            eprintln!("Default remote of '{}' is now '{name}'", repo.name);
        }
    }
    Ok(())
}

fn fetch(pool: &Pool, names: &[String]) -> Result<()> {
    let repos = if names.is_empty() {
        pool.repos()?
    } else {
        names
            .iter()
            .map(|n| pool.repo(n))
            .collect::<wtm_core::Result<_>>()?
    };
    let mut failed = Vec::new();
    for repo in repos {
        eprintln!("Fetching {}", repo.name);
        if let Err(e) = pool.fetch(&repo) {
            eprintln!("wtm: {e}");
            failed.push(repo.name);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("fetching failed for: {}", failed.join(", "))
    }
}

fn new(pool: &Pool, opts: NewWorktree) -> Result<()> {
    let created = pool.new_worktree(&opts)?;
    if let Some(e) = &created.fetch_error {
        eprintln!("warning: fetch failed, using what was fetched before: {e}");
    }
    match &created.checkout {
        Checkout::Existing { branch } => eprintln!("Checked out branch '{branch}'"),
        Checkout::Track { branch, upstream } => {
            eprintln!("Created branch '{branch}' tracking '{upstream}'")
        }
        Checkout::New { branch, start } => {
            eprintln!("Created new branch '{branch}' from '{start}'")
        }
    }
    println!("{}", created.path.display());
    Ok(())
}

fn list(pool: &Pool, repo: Option<&str>, paths: bool, names: bool) -> Result<()> {
    if let Some(repo) = repo {
        pool.repo(repo)?;
    }
    let worktrees: Vec<Worktree> = pool
        .worktrees()?
        .into_iter()
        .filter(|w| repo.is_none_or(|r| w.repo == r))
        .collect();

    if names {
        for worktree in worktrees {
            println!("{}", worktree.name);
        }
        return Ok(());
    }

    let mut header = vec!["NAME", "REPO", "BRANCH", "STATE"];
    if paths {
        header.push("PATH");
    }
    let mut rows = vec![header.into_iter().map(String::from).collect()];
    for worktree in &worktrees {
        let state = match pool.worktree_state(worktree) {
            Ok(state) => describe_state(worktree, &state),
            Err(e) => format!("error: {e}"),
        };
        let mut row = vec![
            worktree.name.clone(),
            worktree.repo.clone(),
            describe_head(worktree.head.as_ref()),
            state,
        ];
        if paths {
            row.push(worktree.path.display().to_string());
        }
        rows.push(row);
    }
    print!("{}", table::format(&rows));
    Ok(())
}

fn describe_head(head: Option<&Head>) -> String {
    match head {
        Some(Head::Branch(branch)) => branch.clone(),
        Some(Head::Unborn(branch)) => format!("{branch} (no commits)"),
        Some(Head::Detached(commit)) => format!("(detached at {})", commit.short()),
        None => "-".to_string(),
    }
}

fn describe_state(worktree: &Worktree, state: &WorktreeState) -> String {
    if worktree.missing {
        return "missing".to_string();
    }
    let mut parts = Vec::new();
    if worktree.locked {
        parts.push("locked".to_string());
    }
    if let Some(status) = &state.status {
        for (n, what) in [
            (status.conflicted, "conflicted"),
            (status.staged, "staged"),
            (status.unstaged, "unstaged"),
            (status.untracked, "untracked"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {what}"));
            }
        }
        if let Some(op) = status.operation {
            parts.push(format!("{op} in progress"));
        }
    }
    match (&state.upstream, state.ahead_behind) {
        (Some(_), Some((ahead, behind))) => {
            if ahead > 0 {
                parts.push(format!("ahead {ahead}"));
            }
            if behind > 0 {
                parts.push(format!("behind {behind}"));
            }
        }
        (Some(_), None) => parts.push("upstream gone".to_string()),
        (None, _) => {}
    }
    if let Some(n) = state.not_on_remotes.filter(|n| *n > 0)
        && state.ahead_behind.is_none()
    {
        parts.push(format!("{n} unpushed"));
    }
    if parts.is_empty() {
        "clean".to_string()
    } else {
        parts.join(", ")
    }
}

fn drop(pool: &Pool, name: &str, opts: DropOptions) -> Result<()> {
    let dropped = pool.drop_worktree(name, &opts)?;
    eprintln!(
        "Dropped worktree '{}' ({})",
        dropped.worktree.name,
        dropped.worktree.path.display()
    );
    if let Some(branch) = &dropped.deleted_branch {
        eprintln!("Deleted branch '{branch}'");
    }
    if let Ok(cwd) = std::env::current_dir()
        && cwd.starts_with(&dropped.worktree.path)
    {
        eprintln!("note: the current directory was inside the dropped worktree");
    }
    Ok(())
}

fn repair(pool: &Pool) -> Result<()> {
    for repaired in pool.repair()? {
        if !repaired.worktrees.is_empty() {
            eprintln!(
                "{}: checked {} worktree(s)",
                repaired.repo,
                repaired.worktrees.len()
            );
        }
    }
    Ok(())
}

fn show_config(config: &Config) -> Result<()> {
    let mut file = config.file.value.display().to_string();
    if !config.file_exists {
        file.push_str(" (not present)");
    }
    let rows = vec![
        vec![
            "SETTING".to_string(),
            "VALUE".to_string(),
            "FROM".to_string(),
        ],
        vec![
            "config file".to_string(),
            file,
            config.file.source.to_string(),
        ],
        vec![
            "repos_dir".to_string(),
            config.repos_dir.value.display().to_string(),
            config.repos_dir.source.to_string(),
        ],
        vec![
            "checkouts_dir".to_string(),
            config.checkouts_dir.value.display().to_string(),
            config.checkouts_dir.source.to_string(),
        ],
    ];
    print!("{}", table::format(&rows));
    Ok(())
}
