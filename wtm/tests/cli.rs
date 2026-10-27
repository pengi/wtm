//! End-to-end tests driving the `wtm` binary against local upstreams.

use std::path::{Path, PathBuf};
use std::process::Command as Process;

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;
use tempfile::TempDir;

/// An isolated home directory with its own git config.
struct Env {
    tmp: TempDir,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let env = Env { tmp };
        std::fs::create_dir_all(env.home()).unwrap();
        // A default branch other than "main" checks that wtm asks the
        // upstream instead of assuming.
        std::fs::write(
            env.home().join(".gitconfig"),
            "[user]\n\tname = Test\n\temail = test@example.invalid\n\
             [init]\n\tdefaultBranch = trunk\n",
        )
        .unwrap();
        env
    }

    fn root(&self) -> &Path {
        self.tmp.path()
    }

    fn home(&self) -> PathBuf {
        self.root().join("home")
    }

    fn checkouts(&self) -> PathBuf {
        self.home().join("wt")
    }

    fn repos(&self) -> PathBuf {
        self.home().join(".local/share/wtm/repos")
    }

    fn apply(&self, cmd: &mut Process) {
        cmd.env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join(".config"))
            .env("XDG_DATA_HOME", self.home().join(".local/share"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_CONFIG_GLOBAL")
            .env_remove("WTM_CONFIG")
            .env_remove("WTM_REPOS_DIR")
            .env_remove("WTM_CHECKOUTS_DIR")
            .current_dir(self.root());
    }

    fn wtm(&self) -> Command {
        let mut process = Process::new(assert_cmd::cargo::cargo_bin!("wtm"));
        self.apply(&mut process);
        Command::from_std(process)
    }

    /// Run git, failing the test if it fails; returns standard output.
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Process::new("git");
        self.apply(&mut cmd);
        let out = cmd.current_dir(dir).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn commit(&self, dir: &Path, file: &str, content: &str) {
        std::fs::write(dir.join(file), content).unwrap();
        self.git(dir, &["add", file]);
        self.git(dir, &["commit", "-q", "-m", file]);
    }

    /// An upstream with branches `trunk` (default) and `feature`, which
    /// ignores `*.o` files.
    fn upstream(&self, name: &str) -> PathBuf {
        let dir = self.root().join("upstreams").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        self.git(&dir, &["init", "-q"]);
        self.commit(&dir, ".gitignore", "*.o\n");
        self.git(&dir, &["checkout", "-q", "-b", "feature"]);
        self.commit(&dir, "feature.txt", "feature\n");
        self.git(&dir, &["checkout", "-q", "trunk"]);
        dir
    }

    /// Register an upstream as `myrepo`.
    fn registered(&self) -> PathBuf {
        let upstream = self.upstream("up");
        self.wtm()
            .args(["register", upstream.to_str().unwrap(), "myrepo"])
            .assert()
            .success();
        upstream
    }

    fn new_worktree(&self, args: &[&str]) -> PathBuf {
        let out = self.wtm().arg("new").args(args).assert().success();
        PathBuf::from(
            String::from_utf8(out.get_output().stdout.clone())
                .unwrap()
                .trim(),
        )
    }

    fn branch(&self, worktree: &Path) -> String {
        self.git(worktree, &["branch", "--show-current"])
            .trim()
            .to_string()
    }
}

#[test]
fn register_creates_bare_repo_with_tracking_refs() {
    let env = Env::new();
    let upstream = env.upstream("up");
    env.wtm()
        .args(["register", upstream.to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("Registered 'up'").and(contains("Default branch: trunk")));

    let git_dir = env.repos().join("up.git");
    assert_eq!(
        env.git(&git_dir, &["rev-parse", "--is-bare-repository"])
            .trim(),
        "true"
    );
    let refs = env.git(&git_dir, &["for-each-ref", "--format=%(refname)"]);
    assert!(refs.contains("refs/remotes/origin/trunk"), "{refs}");
    assert!(refs.contains("refs/remotes/origin/feature"), "{refs}");
    assert!(!refs.contains("refs/heads/"), "no local branches: {refs}");
    assert_eq!(
        env.git(&git_dir, &["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/trunk"
    );

    env.wtm().arg("repos").assert().success().stdout(
        contains("up")
            .and(contains("trunk"))
            .and(contains("origin*")),
    );
}

#[test]
fn register_refuses_duplicates_and_cleans_up_failures() {
    let env = Env::new();
    env.registered();
    let other = env.upstream("other");
    env.wtm()
        .args(["register", other.to_str().unwrap(), "myrepo"])
        .assert()
        .failure()
        .stderr(contains("already registered"));

    env.wtm()
        .args(["register", "/nonexistent/repo.git", "broken"])
        .assert()
        .failure();
    assert!(!env.repos().join("broken.git").exists());

    env.wtm()
        .args(["register", "/x/y.git", "bad/name"])
        .assert()
        .failure()
        .stderr(contains("invalid name"));
}

#[test]
fn new_follows_branch_rules() {
    let env = Env::new();
    env.registered();

    // No name: the default branch, tracking the remote.
    let main = env.new_worktree(&["myrepo"]);
    assert_eq!(main, env.checkouts().join("myrepo"));
    assert_eq!(env.branch(&main), "trunk");
    assert!(main.join(".gitignore").exists());

    // A name matching a remote branch tracks it.
    let feature = env.new_worktree(&["myrepo", "feature"]);
    assert_eq!(env.branch(&feature), "feature");
    assert_eq!(
        env.git(&feature, &["rev-parse", "--abbrev-ref", "@{upstream}"])
            .trim(),
        "origin/feature"
    );

    // A new name creates a new branch without upstream.
    let side = env.new_worktree(&["myrepo", "side"]);
    assert_eq!(env.branch(&side), "side");
    assert!(
        Process::new("git")
            .current_dir(&side)
            .args(["rev-parse", "@{upstream}"])
            .output()
            .unwrap()
            .status
            .code()
            != Some(0)
    );

    // --from sets the start point.
    let from = env.new_worktree(&["myrepo", "from", "--from", "origin/feature"]);
    assert!(from.join("feature.txt").exists());

    // -b picks a branch other than the name.
    let named = env.new_worktree(&["myrepo", "elsewhere", "-b", "topic"]);
    assert_eq!(env.branch(&named), "topic");
}

#[test]
fn new_refuses_conflicts() {
    let env = Env::new();
    env.registered();
    env.new_worktree(&["myrepo"]);

    env.wtm()
        .args(["new", "myrepo"])
        .assert()
        .failure()
        .stderr(contains("already exists"));
    env.wtm()
        .args(["new", "myrepo", "second", "-b", "trunk"])
        .assert()
        .failure()
        .stderr(contains("already checked out"));
    env.wtm()
        .args([
            "new",
            "myrepo",
            "x",
            "-b",
            "trunk",
            "--from",
            "origin/feature",
        ])
        .assert()
        .failure()
        .stderr(contains("already exists"));
    env.wtm()
        .args(["new", "myrepo", "y", "--from", "nope"])
        .assert()
        .failure()
        .stderr(contains("not a commit"));
    env.wtm()
        .args(["new", "unknown"])
        .assert()
        .failure()
        .stderr(contains("not registered"));
    assert!(!env.checkouts().join("y").exists());
}

#[test]
fn new_works_offline() {
    let env = Env::new();
    let upstream = env.registered();
    std::fs::rename(&upstream, env.root().join("gone")).unwrap();
    let path = env.new_worktree(&["myrepo"]);
    assert!(path.join(".gitignore").exists());
    env.wtm()
        .args(["new", "myrepo", "two"])
        .assert()
        .success()
        .stderr(contains("warning: fetch failed"));
}

#[test]
fn locate_prints_paths_only() {
    let env = Env::new();
    env.registered();
    let path = env.new_worktree(&["myrepo", "feature"]);

    let out = env.wtm().args(["locate", "feature"]).assert().success();
    let located = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(
        Path::new(located.trim()).canonicalize().unwrap(),
        path.canonicalize().unwrap()
    );
    env.wtm()
        .arg("locate")
        .assert()
        .success()
        .stdout(format!("{}\n", env.checkouts().display()));
    env.wtm()
        .args(["locate", "--repo", "myrepo"])
        .assert()
        .success()
        .stdout(format!("{}\n", env.repos().join("myrepo.git").display()));
    env.wtm()
        .args(["locate", "nope"])
        .assert()
        .failure()
        .stdout("");
}

#[test]
fn list_shows_state() {
    let env = Env::new();
    let upstream = env.registered();
    let main = env.new_worktree(&["myrepo"]);
    let feature = env.new_worktree(&["myrepo", "feature"]);
    let side = env.new_worktree(&["myrepo", "side"]);

    std::fs::write(main.join("untracked.txt"), "x").unwrap();
    std::fs::write(main.join("ignored.o"), "x").unwrap();
    env.commit(&feature, "local.txt", "x");
    env.commit(&side, "side.txt", "x");
    env.commit(&upstream, "upstream.txt", "x");
    env.wtm().arg("fetch").assert().success();

    let out = env.wtm().arg("list").assert().success();
    let out = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let line = |name: &str| {
        out.lines()
            .find(|l| l.starts_with(&format!("{name} ")))
            .unwrap_or_else(|| panic!("no {name} in {out}"))
            .to_string()
    };
    assert!(line("myrepo").contains("1 untracked"), "{out}");
    assert!(line("myrepo").contains("behind 1"), "{out}");
    assert!(line("feature").contains("ahead 1"), "{out}");
    assert!(line("side").contains("1 unpushed"), "{out}");

    env.wtm()
        .args(["list", "--names"])
        .assert()
        .success()
        .stdout("feature\nmyrepo\nside\n");
}

#[test]
fn drop_refuses_when_something_would_be_lost() {
    let env = Env::new();
    env.registered();
    let path = env.new_worktree(&["myrepo", "work"]);

    // Ignored files don't count.
    std::fs::write(path.join("build.o"), "x").unwrap();

    std::fs::write(path.join("notes.txt"), "x").unwrap();
    env.wtm()
        .args(["drop", "work"])
        .assert()
        .failure()
        .stderr(contains("1 untracked file not ignored"));

    env.git(&path, &["add", "notes.txt"]);
    env.wtm()
        .args(["drop", "work"])
        .assert()
        .failure()
        .stderr(contains("uncommitted changes (1 staged)"));

    env.git(&path, &["commit", "-q", "-m", "notes"]);
    env.wtm()
        .args(["drop", "work"])
        .assert()
        .failure()
        .stderr(contains("branch 'work' has 1 commit on no remote").and(contains("--keep-branch")));

    // Keeping the branch keeps the commit, so nothing is lost.
    env.wtm()
        .args(["drop", "work", "--keep-branch"])
        .assert()
        .success();
    assert!(!path.exists());
    let git_dir = env.repos().join("myrepo.git");
    env.git(&git_dir, &["rev-parse", "--verify", "refs/heads/work"]);

    // The kept branch can be checked out again.
    let again = env.new_worktree(&["myrepo", "work"]);
    assert!(again.join("notes.txt").exists());
    env.wtm()
        .args(["drop", "work", "--force"])
        .assert()
        .success();
    assert!(
        Process::new("git")
            .args(["--git-dir", git_dir.to_str().unwrap()])
            .args(["rev-parse", "--verify", "-q", "refs/heads/work"])
            .output()
            .unwrap()
            .stdout
            .is_empty()
    );
}

#[test]
fn drop_removes_clean_worktree_and_branch() {
    let env = Env::new();
    env.registered();
    let path = env.new_worktree(&["myrepo", "feature"]);
    std::fs::write(path.join("build.o"), "x").unwrap();
    env.wtm()
        .args(["drop", "feature"])
        .assert()
        .success()
        .stderr(contains("Deleted branch 'feature'"));
    assert!(!path.exists());
    env.wtm().args(["list", "--names"]).assert().stdout("");
}

#[test]
fn drop_checks_detached_head_and_locks() {
    let env = Env::new();
    env.registered();
    let path = env.new_worktree(&["myrepo", "det"]);
    env.git(&path, &["checkout", "-q", "--detach"]);
    env.commit(&path, "lost.txt", "x");
    env.wtm()
        .args(["drop", "det"])
        .assert()
        .failure()
        .stderr(contains("detached HEAD has 1 commit on no branch"));
    env.git(&path, &["reset", "-q", "--hard", "HEAD~1"]);

    let git_dir = env.repos().join("myrepo.git");
    env.git(&git_dir, &["worktree", "lock", path.to_str().unwrap()]);
    env.wtm()
        .args(["drop", "det"])
        .assert()
        .failure()
        .stderr(contains("locked"));
    env.wtm().args(["drop", "det", "-f"]).assert().success();
    assert!(!path.exists());
}

#[test]
fn drop_forgets_deleted_worktree() {
    let env = Env::new();
    env.registered();
    let path = env.new_worktree(&["myrepo", "feature"]);
    std::fs::remove_dir_all(&path).unwrap();
    env.wtm()
        .arg("list")
        .assert()
        .success()
        .stdout(contains("missing"));
    env.wtm().args(["drop", "feature"]).assert().success();
    env.wtm().args(["list", "--names"]).assert().stdout("");
}

#[test]
fn unregister_requires_no_worktrees_and_pushed_branches() {
    let env = Env::new();
    env.registered();
    let path = env.new_worktree(&["myrepo", "work"]);
    env.wtm()
        .args(["unregister", "myrepo"])
        .assert()
        .failure()
        .stderr(contains("still has worktrees"));

    env.commit(&path, "x.txt", "x");
    env.wtm()
        .args(["drop", "work", "--keep-branch"])
        .assert()
        .success();
    env.wtm()
        .args(["unregister", "myrepo"])
        .assert()
        .failure()
        .stderr(contains("work"));
    env.wtm()
        .args(["unregister", "myrepo", "--force"])
        .assert()
        .success();
    assert!(!env.repos().join("myrepo.git").exists());
}

#[test]
fn remotes_can_be_added_and_made_default() {
    let env = Env::new();
    env.registered();
    let fork = env.upstream("fork");
    env.git(&fork, &["checkout", "-q", "-b", "forked"]);
    env.commit(&fork, "fork.txt", "x");

    env.wtm()
        .args(["remote", "add", "myrepo", "fork", fork.to_str().unwrap()])
        .assert()
        .success();
    env.wtm()
        .args(["remote", "list", "myrepo"])
        .assert()
        .success()
        .stdout(contains("origin*").and(contains("fork ")));

    // Without --remote, only the default remote is looked at.
    let path = env.new_worktree(&["myrepo", "a", "-b", "forked"]);
    assert!(!path.join("fork.txt").exists());
    env.wtm().args(["drop", "a"]).assert().success();

    let path = env.new_worktree(&["myrepo", "b", "-b", "forked", "--remote", "fork"]);
    assert!(path.join("fork.txt").exists());
    assert_eq!(
        env.git(&path, &["rev-parse", "--abbrev-ref", "@{upstream}"])
            .trim(),
        "fork/forked"
    );
    env.wtm().args(["drop", "b"]).assert().success();

    env.wtm()
        .args(["remote", "set-default", "myrepo", "fork"])
        .assert()
        .success();
    let path = env.new_worktree(&["myrepo", "forked"]);
    assert!(path.join("fork.txt").exists());

    env.wtm()
        .args(["remote", "remove", "myrepo", "fork"])
        .assert()
        .success();
    env.wtm()
        .args(["remote", "list", "myrepo"])
        .assert()
        .success()
        .stdout(contains("origin*").and(contains("fork").not()));
    env.wtm()
        .args(["remote", "remove", "myrepo", "fork"])
        .assert()
        .failure()
        .stderr(contains("no remote 'fork'"));
}

#[test]
fn repair_relinks_moved_directories() {
    let env = Env::new();
    env.registered();
    env.new_worktree(&["myrepo"]);
    env.new_worktree(&["myrepo", "feature"]);

    let new_checkouts = env.root().join("moved-wt");
    let new_repos = env.root().join("moved-repos");
    std::fs::rename(env.checkouts(), &new_checkouts).unwrap();
    std::fs::rename(env.repos(), &new_repos).unwrap();
    let dirs = [
        "--checkouts-dir",
        new_checkouts.to_str().unwrap(),
        "--repos-dir",
        new_repos.to_str().unwrap(),
    ];

    env.wtm().args(dirs).arg("repair").assert().success();
    env.wtm()
        .args(dirs)
        .arg("list")
        .assert()
        .success()
        .stdout(contains("missing").not().and(contains("feature")));
    assert_eq!(
        env.git(&new_checkouts.join("feature"), &["status", "--short"]),
        ""
    );
    env.wtm()
        .args(dirs)
        .args(["drop", "feature"])
        .assert()
        .success();
}

#[test]
fn config_precedence() {
    let env = Env::new();
    let config = env.home().join(".config/wtm/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "checkouts_dir = \"~/from-file\"\n").unwrap();

    env.wtm()
        .arg("config")
        .assert()
        .success()
        .stdout(contains("from-file").and(contains("config file")));
    env.wtm()
        .arg("locate")
        .env("WTM_CHECKOUTS_DIR", "/from/env")
        .assert()
        .stdout("/from/env\n");
    env.wtm()
        .args(["locate", "--checkouts-dir", "/from/arg"])
        .env("WTM_CHECKOUTS_DIR", "/from/env")
        .assert()
        .stdout("/from/arg\n");

    std::fs::write(&config, "unknown = 1\n").unwrap();
    env.wtm()
        .arg("config")
        .assert()
        .failure()
        .stderr(contains("unknown"));
}

#[test]
fn completion_offers_worktrees() {
    let env = Env::new();
    env.registered();
    env.new_worktree(&["myrepo", "feature"]);
    env.wtm()
        .args(["--", "wtm", "drop", ""])
        .env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "2")
        .env("_CLAP_IFS", "\n")
        .assert()
        .success()
        .stdout(contains("feature"));
    env.wtm()
        .args(["--", "wtm", "new", "my"])
        .env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "2")
        .env("_CLAP_IFS", "\n")
        .assert()
        .success()
        .stdout("myrepo");
}

#[test]
fn shell_init_defines_cd_function() {
    let env = Env::new();
    env.wtm()
        .args(["shell-init", "zsh"])
        .assert()
        .success()
        .stdout(contains("wcd()").and(contains("compdef _clap_dynamic_completer_wtm wtm")));
    env.wtm()
        .args(["shell-init", "bash", "--cd-command", "go"])
        .assert()
        .success()
        .stdout(contains("go()").and(contains("complete -F _wtm_go go")));
    env.wtm()
        .args(["shell-init", "bash", "--cd-command", "a;b"])
        .assert()
        .failure();
}

impl Env {
    /// A normal clone of a fresh upstream, at `<root>/src/<name>`.
    fn clone_of_upstream(&self, name: &str) -> PathBuf {
        let upstream = self.upstream(&format!("{name}-up"));
        let clone = self.root().join("src").join(name);
        std::fs::create_dir_all(clone.parent().unwrap()).unwrap();
        self.git(
            self.root(),
            &[
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        clone
    }
}

#[test]
fn adopt_keeps_everything() {
    let env = Env::new();
    let clone = env.clone_of_upstream("proj");

    // Local state that must survive: an unpushed branch, a stash, staged
    // and unstaged changes, untracked and ignored files.
    env.git(&clone, &["checkout", "-q", "-b", "local"]);
    env.commit(&clone, "local.txt", "local");
    std::fs::write(clone.join("stashed.txt"), "s").unwrap();
    env.git(&clone, &["stash", "push", "-q", "-u"]);
    std::fs::write(clone.join("staged.txt"), "s").unwrap();
    env.git(&clone, &["add", "staged.txt"]);
    std::fs::write(clone.join("local.txt"), "changed").unwrap();
    std::fs::write(clone.join("untracked.txt"), "u").unwrap();
    std::fs::write(clone.join("build.o"), "o").unwrap();
    let status = env.git(&clone, &["status", "--porcelain", "--ignored"]);

    let out = env
        .wtm()
        .args(["adopt", clone.to_str().unwrap()])
        .assert()
        .success();
    let path = PathBuf::from(
        String::from_utf8(out.get_output().stdout.clone())
            .unwrap()
            .trim(),
    );
    assert_eq!(path, env.checkouts().join("proj"));
    assert!(!clone.exists());

    assert!(path.join(".git").is_file());
    assert_eq!(
        env.git(&path, &["status", "--porcelain", "--ignored"]),
        status
    );
    assert_eq!(env.branch(&path), "local");
    assert!(env.git(&path, &["stash", "list"]).contains("stash@{0}"));

    let git_dir = env.repos().join("proj.git");
    assert_eq!(
        env.git(&git_dir, &["rev-parse", "--is-bare-repository"])
            .trim(),
        "true"
    );
    assert_eq!(
        env.git(&git_dir, &["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/trunk"
    );

    env.wtm().arg("list").assert().success().stdout(
        contains("proj")
            .and(contains("local"))
            .and(contains("1 staged")),
    );
    // The adopted repository works like a registered one.
    let other = env.new_worktree(&["proj", "feature"]);
    assert!(other.join("feature.txt").exists());
    env.wtm()
        .args(["drop", "proj"])
        .assert()
        .failure()
        .stderr(contains("uncommitted changes"));
}

#[test]
fn adopt_keep_path_and_detached_head() {
    let env = Env::new();
    let clone = env.clone_of_upstream("proj");
    env.git(&clone, &["checkout", "-q", "--detach", "HEAD"]);

    env.wtm()
        .args(["adopt", "--keep-path", clone.to_str().unwrap(), "renamed"])
        .assert()
        .success();
    assert!(clone.join(".git").is_file());
    assert!(env.repos().join("renamed.git").is_dir());
    assert_eq!(env.git(&clone, &["status", "--porcelain"]), "");
    assert_eq!(env.branch(&clone), "");

    let out = env.wtm().args(["locate", "proj"]).assert().success();
    let located = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(
        Path::new(located.trim()).canonicalize().unwrap(),
        clone.canonicalize().unwrap()
    );
}

#[test]
fn adopt_refuses_what_it_cannot_convert() {
    let env = Env::new();
    let clone = env.clone_of_upstream("proj");

    env.wtm()
        .args(["adopt", clone.join("subdir").to_str().unwrap()])
        .assert()
        .failure();
    std::fs::create_dir(clone.join("sub")).unwrap();
    env.wtm()
        .args(["adopt", clone.join("sub").to_str().unwrap()])
        .assert()
        .failure()
        .stderr(contains("not the top directory"));

    let linked = env.root().join("linked");
    env.git(&clone, &["worktree", "add", "-q", linked.to_str().unwrap()]);
    env.wtm()
        .args(["adopt", clone.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(contains("linked worktrees"));
    env.wtm()
        .args(["adopt", linked.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(contains("not the top directory"));
    env.git(&clone, &["worktree", "remove", linked.to_str().unwrap()]);

    env.registered();
    env.wtm()
        .args(["adopt", clone.to_str().unwrap(), "myrepo"])
        .assert()
        .failure()
        .stderr(contains("already registered"));

    // Nothing was changed by the refusals.
    assert!(clone.join(".git").is_dir());
    assert_eq!(env.git(&clone, &["status", "--porcelain"]), "");
}
