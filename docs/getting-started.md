# Getting started

This walks through a typical session. It assumes the default directories:
repositories in `~/.local/share/wtm/repos`, worktrees in `~/wt`. See
[configuration](configuration.md) to change them.

## Register a repository

```sh
wtm register git@github.com:someone/project.git project
```

This creates the bare repository `project.git` in the repos directory and
fetches everything from the upstream. No files are checked out yet. The
name is optional; without it, the name is taken from the URL.

## Create a worktree

```sh
wtm new project
```

This creates `~/wt/project` with the upstream's default branch checked out,
and prints its path. Work in it with git as usual: commit, push, pull.

## Work on something else at the same time

```sh
wtm new project fix-thing
```

This creates a second worktree, `~/wt/fix-thing`, on branch `fix-thing`:

- if the upstream has a branch `fix-thing`, you get a local branch tracking
  it;
- otherwise a new branch is created from the upstream's default branch. It
  has no upstream yet; push it with `git push -u origin fix-thing`.

To choose the branch or start point yourself:

```sh
wtm new project review -b someones-branch   # worktree "review", that branch
wtm new project hotfix --from v1.2.0        # new branch from a tag
```

Before creating a worktree, `new` fetches the repository, so you start from
what's current. Without network, it warns and uses what was fetched before.

!!! note
    A branch can only be checked out in one worktree at a time. This is a
    git rule.

## Move between worktrees

```sh
wcd fix-thing
```

`wcd` comes with the [shell integration](install.md#shell-integration).
Without it, use `cd "$(wtm locate fix-thing)"`.

## See what you have

```sh
wtm list
```

```
NAME       REPO     BRANCH     STATE
fix-thing  project  fix-thing  1 unstaged, 2 unpushed
project    project  main       behind 3
```

`wtm fetch` updates all repositories, so `behind` counts are current.

## Clean up

```sh
wtm drop fix-thing
```

This deletes the worktree directory and the branch `fix-thing`, but only if
nothing would be lost: no uncommitted changes, no untracked files (ignored
files, such as build output, don't count), and no commits that aren't on a
remote. Otherwise it tells you what's in the way:

```
wtm: worktree 'fix-thing' is not clean
  - branch 'fix-thing' has 2 commits on no remote
Push the branch, keep it with --keep-branch, or drop anyway with --force.
```

With `--keep-branch`, the branch stays in the repository, and
`wtm new project fix-thing` brings it back later.
