use std::fs;
use std::io::{IsTerminal, Write};

use anyhow::{bail, Context, Result};

use dot::brief;
use dot::git;
use dot::plan::Plan;
use dot::store::Store;
use dot::task::Task;

pub fn init(skill: bool, no_skill: bool, agent: Option<String>) -> Result<()> {
    let store = Store::discover()?;
    store.init()?;
    println!(
        "Initialized tod: branch '{}' checked out in {}",
        dot::store::TOD_BRANCH,
        store.tod_dir().display()
    );
    println!("Your todos are versioned. Share them with: git push origin {}", dot::store::TOD_BRANCH);

    // --agent <name>: install directly. --skill: install, asking
    // which agent when interactive, generic otherwise. --no-skill:
    // skip. No flag: ask (default yes) when interactive, skip in
    // scripts.
    let install_for = if let Some(flag) = agent {
        Some(parse_agent(&flag)?)
    } else if skill {
        Some(choose_agent_interactive()?)
    } else if no_skill {
        None
    } else if std::io::stdin().is_terminal() && ask_install_skill()? {
        Some(choose_agent_interactive()?)
    } else {
        None
    };
    if let Some(agent) = install_for {
        install_skill(&store, agent)?;
    }
    Ok(())
}

fn parse_agent(flag: &str) -> Result<crate::skill::Agent> {
    crate::skill::Agent::from_flag(flag)
        .ok_or_else(|| anyhow::anyhow!("unknown agent '{flag}' (claude | cursor | generic)"))
}

fn ask_install_skill() -> Result<bool> {
    print!("\nInstall the tod skill for your AI agent? [Y/n] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    let answer = answer.trim().to_lowercase();
    Ok(answer.is_empty() || answer == "y" || answer == "yes")
}

/// Which agent do you use? The skill folder differs per agent.
fn choose_agent_interactive() -> Result<crate::skill::Agent> {
    println!("\nWhich agent do you use?");
    println!("  1) Claude Code    (.claude/skills/tod/)");
    println!("  2) Cursor         (.cursor/skills/tod/)");
    println!("  3) Mistral Vibe   (.vibe/skills/tod/)");
    println!("  4) other / standard (.agents/skills/tod/)");
    print!("Choose [1-4, default 4]: ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(match answer.trim() {
        "1" => crate::skill::Agent::Claude,
        "2" => crate::skill::Agent::Cursor,
        "3" => crate::skill::Agent::Vibe,
        _ => crate::skill::Agent::Generic,
    })
}

pub fn skill(remove: bool, agent: Option<String>) -> Result<()> {
    let store = Store::discover()?;
    if remove {
        if let Some(flag) = agent {
            let agent = parse_agent(&flag)?;
            if crate::skill::remove(&store, agent)? {
                println!("Removed the {} skill", agent.label());
            } else {
                println!("No skill installed for {}", agent.label());
            }
        } else {
            let removed = crate::skill::remove_all(&store)?;
            println!("Removed {removed} skill(s)");
        }
    } else {
        let agent = match agent {
            Some(flag) => parse_agent(&flag)?,
            None => choose_agent_interactive()?,
        };
        install_skill(&store, agent)?;
    }
    Ok(())
}

fn install_skill(store: &Store, agent: crate::skill::Agent) -> Result<()> {
    let path = crate::skill::install(store, agent)?;
    println!("Installed the {} skill: {}", agent.label(), path.display());
    println!("Commit it so your agent picks it up in every checkout.");
    Ok(())
}

pub fn add(description: &str, backlog: bool) -> Result<()> {
    let store = Store::discover()?;
    let author = git::user_name();
    let task = store.add(description, &author, backlog)?;
    println!("Added #{} {}", &task.uid[..5], task.description);
    Ok(())
}

pub fn plan(description: &str, backlog: bool) -> Result<()> {
    let store = Store::discover()?;
    let author = git::user_name();
    let task = store.add(description, &author, backlog)?;

    // the plan command creates the spec.md in advance
    let (state, dir) = store.locate(&task.uid)?;
    let plan = Plan::load(&dir)?;
    brief::write_brief(&dir, &task, &state, &plan)?;
    store.commit(&format!("tod: brief {}", &task.uid[..7]))?;

    println!(
        "Planned #{} {} — {}",
        &task.uid[..5],
        task.description,
        dir.join(dot::brief::SPEC_FILE).display()
    );
    Ok(())
}

pub fn sub(identifier: &str, step: &str) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (state, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;

    let mut plan = Plan::load(&dir)?;
    let number = plan.add_step(step);
    Plan::save(&dir, &plan)?;
    store.commit("tod: update plan")?;
    refresh_brief(&store, &task, &state)?;

    println!("Added step {number} to #{}: {step}", &task.uid[..5]);
    Ok(())
}

pub fn check(identifier: &str, number: usize) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (state, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;

    let mut plan = Plan::load(&dir)?;
    let step = plan.check_step(number)?;
    Plan::save(&dir, &plan)?;
    store.commit("tod: update plan")?;
    refresh_brief(&store, &task, &state)?;

    let mark = if step.done { 'x' } else { ' ' };
    println!("[{mark}] step {number} of #{}: {}", &task.uid[..5], step.text);
    Ok(())
}

pub fn brief(identifier: &str) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (state, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;
    let plan = Plan::load(&dir)?;

    brief::write_brief(&dir, &task, &state, &plan)?;
    store.commit(&format!("tod: brief {}", &task.uid[..7]))?;
    println!("{}", brief::markdown(&task, &state, &plan));
    Ok(())
}

/// Print the Gherkin scenarios of a task — scaffolded in French
/// Gherkin when missing — or open them in the editor with --edit:
/// the acceptance criteria are meant to be written by hand.
pub fn scenario(identifier: &str, edit: bool) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (_state, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;
    let path = dir.join(dot::scenario::SCENARIO_FILE);

    if !path.is_file() {
        fs::write(&path, dot::scenario::scaffold(&task))
            .with_context(|| format!("cannot write {}", path.display()))?;
        store.commit(&format!("tod: scenario {}", &task.uid[..7]))?;
        if !edit {
            println!("Created {} — fill the scenarios, or edit them with --edit", path.display());
        }
    }

    if edit {
        edit_in_editor(&path)?;
        store.commit(&format!("tod: scenario {}", &task.uid[..7]))?;
        println!("Edited {}", path.display());
    } else {
        println!("{}", fs::read_to_string(&path)?);
    }
    Ok(())
}

pub fn status() -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;

    for (state, tasks) in store.states()? {
        println!("{}:", state.to_uppercase());
        if tasks.is_empty() {
            println!("  (none)");
            continue;
        }
        for task in &tasks {
            let plan = Plan::load(&store.locate(&task.uid)?.1)?;
            let progress = if !plan.is_empty() {
                format!(" [{} {}]", plan.done_count(), plan.steps.len())
            } else {
                String::new()
            };
            let branch = task
                .branch
                .as_deref()
                .map(|b| format!(" @{b}"))
                .unwrap_or_default();
            println!(
                "  #{} {}{}{}{progress}",
                &task.uid[..5],
                task.description,
                tags_suffix(task),
                branch
            );
        }
    }
    Ok(())
}

/// The tags of a task, as a printable suffix (" #a #b").
fn tags_suffix(task: &Task) -> String {
    task.tags.iter().map(|t| format!(" #{t}")).collect()
}

/// Open a file in the user's editor — `$TOD_EDITOR`, then `$EDITOR`,
/// then `vi`. The command waits for the editor to exit, so the
/// changes are there when it returns.
fn edit_in_editor(path: &std::path::Path) -> Result<()> {
    let editor = std::env::var("TOD_EDITOR")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    // sh -c: the editor variable may carry flags ("code -w")
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} \"$1\"", editor))
        .arg("tod")
        .arg(path)
        .status()
        .with_context(|| format!("cannot start the editor '{editor}'"))?;
    if !status.success() {
        bail!("the editor '{editor}' exited with an error");
    }
    Ok(())
}

/// Print the spec.md of a task — generated on the fly when missing —
/// or open it in the editor with --edit: the fine-grained work
/// description is meant to be contributed, not only generated.
pub fn spec(identifier: &str, edit: bool) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (state, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;
    let path = dir.join(dot::brief::SPEC_FILE);

    if !path.is_file() {
        let plan = Plan::load(&dir)?;
        brief::write_brief(&dir, &task, &state, &plan)?;
        store.commit(&format!("tod: brief {}", &task.uid[..7]))?;
    }

    if edit {
        edit_in_editor(&path)?;
        store.commit(&format!("tod: spec {}", &task.uid[..7]))?;
        println!("Edited {}", path.display());
    } else {
        println!("{}", fs::read_to_string(&path)?);
    }
    Ok(())
}

/// List the todos. The tasks left to do (backlog, todo, review,
/// test) are shown, unless 'all' also includes done and canceled.
/// Default is the current branch; 'backlog' is the backlog;
/// 'all_contexts' is every state. 'tag' keeps only the tasks
/// carrying a given tag.
pub fn ls(backlog: bool, all: bool, all_contexts: bool, tag: Option<String>) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;

    let mut source: Vec<(String, Vec<Task>)> = if all_contexts {
        store.states()?
    } else if backlog {
        vec![("backlog".to_string(), store.load_backlog()?)]
    } else {
        let branch = store.branch()?;
        vec![(branch.clone(), store.load_context(&branch)?)]
    };

    // 'all' includes the finished states; otherwise only the work
    // left to do. The tag filter applies on top of it.
    for (state, tasks) in &mut source {
        let keep_state = all || !matches!(state.as_str(), "done" | "canceled");
        if keep_state {
            tasks.retain(|t| {
                tag.as_ref()
                    .map_or(true, |want| t.tags.iter().any(|x| x == want))
            });
        } else {
            tasks.clear();
        }
    }

    for (state, tasks) in &source {
        if tasks.is_empty() {
            continue;
        }
        if all_contexts {
            println!("{state}:");
        }
        for task in tasks {
            let branch = task
                .branch
                .as_deref()
                .map(|b| format!(" @{b}"))
                .unwrap_or_default();
            println!(
                "[{state}] #{}  {}{}{}",
                &task.uid[..5],
                task.description,
                tags_suffix(task),
                branch
            );
        }
    }
    Ok(())
}

/// Replace the tags of a task, wherever it lives. An empty list
/// clears them.
pub fn tag(identifier: &str, tags: Vec<String>) -> Result<()> {
    let store = Store::discover()?;
    let task = store.set_tags(identifier, tags)?;
    if task.tags.is_empty() {
        println!("Cleared the tags of #{} {}", &task.uid[..5], task.description);
    } else {
        println!(
            "Tagged #{} {}: {}",
            &task.uid[..5],
            task.description,
            task.tags.join(", ")
        );
    }
    Ok(())
}

pub fn done(identifier: &str) -> Result<()> {
    let store = Store::discover()?;
    let task = store.complete(identifier)?;
    println!("Completed #{} {}", &task.uid[..5], task.description);
    Ok(())
}

/// Move a task to an arbitrary state.
pub fn state(identifier: &str, target: &str) -> Result<()> {
    let store = Store::discover()?;
    let task = store.set_state(identifier, target)?;
    println!(
        "Moved #{} {} to {target}",
        &task.uid[..5], task.description
    );
    Ok(())
}

/// Cancel a task: it moves to the canceled state, its directory
/// (task.tod, plan.md, spec.md) is kept for the record.
pub fn rm(identifier: &str) -> Result<()> {
    let store = Store::discover()?;
    let task = store.remove(identifier)?;
    println!("Canceled #{} {}", &task.uid[..5], task.description);
    Ok(())
}

pub fn mv(identifier: &str, to_backlog: bool) -> Result<()> {
    let store = Store::discover()?;
    let task = store.move_task(identifier, to_backlog)?;
    let destination = if to_backlog {
        "backlog".to_string()
    } else {
        format!("context '{}'", store.branch()?)
    };
    println!("Moved #{} {} to {destination}", &task.uid[..5], task.description);
    Ok(())
}

/// Start a task: create a working branch named after it, switch to
/// it, and make the task belong to that branch. The branch starts
/// from 'main' — each task is independent — unless --base says
/// otherwise.
pub fn start(identifier: &str, branch: Option<String>, base: Option<&str>) -> Result<()> {
    let store = Store::discover()?;
    start_in(&store, identifier, branch, base.unwrap_or("main"))
}

fn start_in(store: &Store, identifier: &str, branch: Option<String>, base: &str) -> Result<()> {
    store.ensure_init()?;
    let (_context, task) = store.find_task(identifier)?;

    let branch = match branch {
        Some(name) => name,
        None => {
            // t/<uid7>-<slug>: namespaced ('tod/' would collide with
            // the tod storage branch itself), linked to the task by
            // its identifier, with a short ASCII hint of its content
            let uid7 = &task.uid[..7];
            let slug = git::branch_slug(&task.description);
            if slug.is_empty() {
                format!("t/{uid7}")
            } else {
                format!("t/{uid7}-{slug}")
            }
        }
    };
    git::create_branch_from(store.root(), &branch, base)?;
    git::checkout(store.root(), &branch)?;
    store.start_task(identifier, &branch)?;

    println!(
        "Started #{} {} on branch '{branch}'",
        &task.uid[..5], task.description
    );
    Ok(())
}

/// Finish the current task: rebase its branch onto the base, merge
/// it (fast-forward only), delete the branch and complete its tasks
/// — the whole end-of-task cycle in one command. The test branch is
/// rebased on the base too: the commits main just received drop out
/// of it, it keeps only the tasks still waiting for a test.
pub fn finish(base: &str) -> Result<()> {
    let store = Store::discover()?;
    finish_in(&store, base)
}

fn finish_in(store: &Store, base: &str) -> Result<()> {
    let root = store.root();
    let branch = git::current_branch_in(root)?;
    if branch == base {
        bail!("nothing to finish: you are on '{base}'");
    }

    // the work, rebased on the latest base
    git::rebase(root, base)?;
    git::checkout(root, base)?;
    git::merge_ff_only(root, &branch)?;

    // the branch is merged: it can die, and its tasks are done
    let count = store.complete_context(&branch)?;
    git::delete_branch(root, &branch)?;

    // the test branch drops the merged work
    if git::branch_exists(root, git::TEST_BRANCH)? {
        git::checkout(root, git::TEST_BRANCH)?;
        git::rebase(root, base)?;
        git::checkout(root, base)?;
    }

    println!(
        "Finished '{branch}': merged into '{base}', {count} task(s) done, branch deleted"
    );
    Ok(())
}

/// Move a task to the test state: its work is merged into the test
/// branch — parallel to main, accumulating the tasks to test — so
/// the tester checks out one branch and tries everything.
pub fn test(identifier: &str) -> Result<()> {
    let store = Store::discover()?;
    test_in(&store, identifier)
}

fn test_in(store: &Store, identifier: &str) -> Result<()> {
    store.ensure_init()?;
    let (_, task) = store.find_task(identifier)?;
    let Some(branch) = task.branch.clone() else {
        bail!("task #{} has no working branch (start it first)", &task.uid[..5]);
    };
    let root = store.root();
    let back_to = git::current_branch_in(root)?;

    // keep the work clean on top of main before merging it
    if back_to == branch {
        git::rebase(root, "main")?;
    }

    // the test branch, from main when it does not exist yet
    if !git::branch_exists(root, git::TEST_BRANCH)? {
        git::create_branch_from(root, git::TEST_BRANCH, "main")?;
    }
    git::checkout(root, git::TEST_BRANCH)?;
    git::merge(root, &branch)?;
    git::checkout(root, &back_to)?;

    store.set_state(identifier, "test")?;

    // push both: the testers may be elsewhere
    if store.has_origin() {
        let _ = git::spawn_push(root, "origin", git::TEST_BRANCH);
        let _ = git::spawn_push(root, "origin", &branch);
    }

    println!(
        "Task #{} moved to test: '{}' merged into '{}'{}",
        &task.uid[..5],
        branch,
        git::TEST_BRANCH,
        if store.has_origin() { " (pushed)" } else { "" }
    );
    Ok(())
}

/// Mark a task as ready for review and push its working branch, so
/// the review can happen in the web console.
pub fn rfr(identifier: &str) -> Result<()> {
    let store = Store::discover()?;
    let task = store.ready_for_review(identifier)?;

    // push the branch so the console can show the diff of the work
    let branch = git::current_branch_in(store.root())?;
    if branch != "main" && store.has_origin() {
        println!("Pushing '{branch}' to origin in the background…");
        git::spawn_push(store.root(), "origin", &branch)?;
    }

    println!(
        "Ready for review #{} {}",
        &task.uid[..5], task.description
    );
    Ok(())
}

pub fn ctx() -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let contexts = store.contexts()?;
    if contexts.is_empty() {
        println!("No contexts with todos");
        return Ok(());
    }
    for (name, tasks) in &contexts {
        println!("{}: {} todo(s)", name, tasks.len());
    }
    Ok(())
}

pub fn resolve(context: &str) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let count = store.resolve(context)?;
    println!("Resolved context '{context}': {count} task(s) canceled");
    Ok(())
}

pub fn export(output: &str) -> Result<()> {
    let store = Store::discover()?;
    let (tasks, states) = store.export(output)?;
    println!("Exported {tasks} task(s) across {states} states to {output}");
    Ok(())
}

/// Regenerate the spec markdown, but only if it already exists:
/// 'tod brief' is the explicit way to (re)generate a spec.
fn refresh_brief(store: &Store, task: &Task, state: &str) -> Result<()> {
    let dir = store.locate(&task.uid)?.1;
    if dir.join(dot::brief::SPEC_FILE).is_file() {
        let plan = Plan::load(&dir)?;
        brief::write_brief(&dir, task, state, &plan)?;
        store.commit(&format!("tod: brief {}", &task.uid[..7]))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Run git in 'root' and return its trimmed stdout.
    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {} failed", args.join(" "));
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// A temp directory with a real git repository and a store on it.
    fn git_repo() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git init failed");
        // main must exist: every working branch starts from it
        std::fs::write(dir.path().join("README.md"), "test\n").unwrap();
        git::commit_all(dir.path(), "init").unwrap();
        let store = Store::new(dir.path());
        store.init().unwrap();
        (dir, store)
    }

    #[test]
    fn edit_in_editor_runs_the_configured_editor() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "before").unwrap();
        // 'touch' is the cheapest editor that exists: it opens the
        // file and exits 0
        std::env::set_var("TOD_EDITOR", "touch");
        edit_in_editor(&file).unwrap();
        assert!(file.is_file());
    }

    #[test]
    fn start_creates_a_branch_and_moves_the_task_into_it() {
        let (dir, store) = git_repo();
        let task = store.add_in(true, "Fix the login bug", "tester").unwrap();

        // the branch name is namespaced and linked to the task by its uid
        start_in(&store, &task.uid[..7], None, "main").unwrap();
        assert_eq!(
            git::current_branch_in(dir.path()).unwrap(),
            format!("t/{}-fix-the-login-bug", &task.uid[..7])
        );
        assert!(store.load_backlog().unwrap().is_empty());
        let taken = format!("t/{}-fix-the-login-bug", &task.uid[..7]);
        assert_eq!(store.load_context(&taken).unwrap().len(), 1);

        // an explicit branch name overrides the slug
        let task2 = store.add_in(true, "Another one", "tester").unwrap();
        start_in(&store, &task2.uid[..7], Some("custom".into()), "main").unwrap();
        assert_eq!(git::current_branch_in(dir.path()).unwrap(), "custom");
        assert_eq!(store.load_context("custom").unwrap().len(), 1);

        // a useless description falls back to the bare uid prefix
        let task3 = store.add_in(true, "###", "tester").unwrap();
        start_in(&store, &task3.uid[..7], None, "main").unwrap();
        assert_eq!(
            git::current_branch_in(dir.path()).unwrap(),
            format!("t/{}", &task3.uid[..7])
        );
    }

    #[test]
    fn test_branch_accumulates_work_and_finish_cleans_it() {
        let (dir, store) = git_repo();
        let task = store.add_in(true, "feature a", "tester").unwrap();
        start_in(&store, &task.uid[..7], None, "main").unwrap();
        let work_branch = format!("t/{}-feature-a", &task.uid[..7]);

        // some work on the branch
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        git::commit_all(dir.path(), "feat a").unwrap();

        // tod test: the work lands in the test branch, the task
        // moves to the test state, and we stay on the work branch
        test_in(&store, &task.uid[..7]).unwrap();
        assert_eq!(store.find_task(&task.uid).unwrap().0, "test");
        assert!(git::branch_exists(dir.path(), git::TEST_BRANCH).unwrap());
        assert_eq!(git::current_branch_in(dir.path()).unwrap(), work_branch);
        let log = git(dir.path(), &["log", "--format=%s", git::TEST_BRANCH]);
        assert!(log.contains("feat a"));

        // tod finish from the work branch: main receives the work,
        // the test branch is rebased clean of it
        finish_in(&store, "main").unwrap();
        assert_eq!(store.find_task(&task.uid).unwrap().0, "done");
        assert!(!git::branch_exists(dir.path(), &work_branch).unwrap());
        let ahead = git(dir.path(), &["rev-list", "--count", &format!("main..{}", git::TEST_BRANCH)]);
        assert_eq!(ahead, "0");
    }

    #[test]
    fn start_fails_when_the_branch_already_exists() {
        let (dir, store) = git_repo();
        let task = store.add_in(true, "Fix the login bug", "tester").unwrap();
        start_in(&store, &task.uid[..7], None, "main").unwrap();
        let taken = format!("t/{}-fix-the-login-bug", &task.uid[..7]);

        let task2 = store.add_in(true, "Fix the login bug again", "tester").unwrap();
        // the generated name is free, but an existing name must fail
        assert!(start_in(&store, &task2.uid[..7], Some(taken.clone()), "main").is_err());
        // nothing moved: the second task is still in the backlog
        assert_eq!(store.load_backlog().unwrap().len(), 1);
        assert_eq!(git::current_branch_in(dir.path()).unwrap(), taken);
    }
}

/// Print the shell completion script. The installation one-liner:
/// `eval "$(tod completions <shell>)"`.
pub fn completions(shell: &str) -> Result<()> {
    let shell = shell.to_lowercase();
    if !matches!(shell.as_str(), "bash" | "zsh" | "fish" | "powershell" | "elvish") {
        bail!("unknown shell '{shell}' (bash | zsh | fish | powershell | elvish)");
    }
    // the registration script is printed by the binary itself when
    // invoked with COMPLETE=<shell> and no arguments
    let exe = std::env::current_exe()?;
    let out = std::process::Command::new(exe)
        .env("COMPLETE", &shell)
        .output()
        .map_err(|e| anyhow::anyhow!("cannot run the completion generator: {e}"))?;
    print!("{}", String::from_utf8_lossy(&out.stdout));
    Ok(())
}
