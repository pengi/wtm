# wtm — worktree manager

wtm keeps all your git checkouts in one pool, instead of clones scattered
over your disk:

- **Repositories** are registered once, by name. Each is kept as a bare
  repository in the *repos directory*. It has no files checked out; it holds
  the history, the branches and the remotes.
- **Worktrees** are checkouts of a registered repository, in the flat
  *checkouts directory*. A repository can have any number of them, and they
  all share its history and branches. Nothing is downloaded twice.

```sh
wtm register git@github.com:someone/project.git project
wtm new project              # ~/wt/project, on the default branch
wtm new project fix-thing    # ~/wt/fix-thing, on branch fix-thing
wcd fix-thing                # cd to it (see shell integration)
wtm list                     # what's where, and what's not pushed
wtm drop fix-thing           # remove it, if nothing would be lost
```

Inside a worktree, you use git as usual. wtm adds as little as possible on
top of git: everything it knows, it reads from the repositories themselves.

- [Installation](install.md)
- [Getting started](getting-started.md)
- [Configuration](configuration.md)
- [Accounts](accounts.md): GitHub and GitLab
- [Repositories](repositories.md) and [worktrees](worktrees.md): what wtm
  stores
- [Command reference](reference/commands.md)
