# Installation

wtm needs git 2.30 or newer at run time, and `curl` for
[accounts](accounts.md).

## Packages

Download the package for your system from the
[releases page](https://github.com/pengi/wtm/releases):

| System | Package |
|---|---|
| Debian 12+, Ubuntu 22.04+ | `wtm_<version>_amd64.deb` |
| RHEL 9, AlmaLinux 9, Rocky Linux 9 and newer | `wtm-<version>-1.x86_64.rpm` |

```sh
sudo apt install ./wtm_*_amd64.deb      # Debian, Ubuntu
sudo dnf install ./wtm-*.x86_64.rpm     # RHEL, Alma, Rocky
```

The packages install the `wtm` command, man pages (`man wtm`) and shell
completion for bash and zsh.

## From source

With a [Rust toolchain](https://rustup.rs):

```sh
cargo install --locked --git https://github.com/pengi/wtm wtm
```

This installs only the command. For completion, use the shell integration
below.

## Shell integration

Add this line to your shell's startup file. It sets up completion of wtm's
commands, repository names and worktree names, and defines `wcd`, which
changes directory to a worktree:

For zsh, in `~/.zshrc`, after `compinit`:

```sh
eval "$(wtm shell-init zsh)"
```

For bash, in `~/.bashrc`:

```sh
eval "$(wtm shell-init bash)"
```

Then, in a new shell:

```sh
wcd myrepo     # cd to the worktree myrepo
wcd            # cd to the checkouts directory
```

To use another name than `wcd`, pass `--cd-command NAME` to `shell-init`.
