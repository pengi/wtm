# Configuration

wtm needs to know two directories:

| Setting | Holds | Default |
|---|---|---|
| `repos_dir` | the bare repositories | `~/.local/share/wtm/repos` |
| `checkouts_dir` | the worktrees | `~/wt` |

Both are created when first needed. No configuration is required.

## Config file

`~/.config/wtm/config.toml` (or `$XDG_CONFIG_HOME/wtm/config.toml`):

```toml
repos_dir = "~/.local/share/wtm/repos"
checkouts_dir = "~/src"
```

Paths must be absolute or start with `~/`. Unknown keys are an error, so
typos don't go unnoticed.

## Overriding

Each setting can also be given as an environment variable or a command line
option. The first one found is used:

1. command line option
2. environment variable
3. config file
4. default

| Setting | Option | Environment variable |
|---|---|---|
| config file | `--config FILE` | `WTM_CONFIG` |
| `repos_dir` | `--repos-dir DIR` | `WTM_REPOS_DIR` |
| `checkouts_dir` | `--checkouts-dir DIR` | `WTM_CHECKOUTS_DIR` |

A config file given explicitly must exist. Relative paths in options and
environment variables are relative to the current directory.

`wtm config` shows the settings in effect and where each comes from.

!!! note
    Shell completion only sees environment variables and the config file,
    not options on the command line being completed.

## Moving the directories

Git records where worktrees and repositories are. After moving either
directory, update the configuration and run `wtm repair` to re-link them.
