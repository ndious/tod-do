use std::fs;
use std::io::{IsTerminal, Write};

use anyhow::{bail, Context, Result};

use dot::git;
use dot::plan::Plan;
use dot::spec;
use dot::store::Store;
use dot::task::Task;

use crate::hooks as hooks_mod;

pub fn init(skill: bool, no_skill: bool, agent: Option<String>) -> Result<()> {
    let store = Store::discover()?;
    store.init()?;
    println!(
        "Initialized tod: branch '{}' checked out in {}",
        dot::store::TOD_BRANCH,
        store.tod_dir().display()
    );
    let remote = store.remote()?;
    println!(
        "Your todos are versioned. Share them with: git push {remote} {}",
        dot::store::TOD_BRANCH
    );
    scaffold_config(&store)?;

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

/// Write the commented `.tod.toml` at the repository root when there
/// is none: the per-project settings (sync remote, validation
/// command) must stay one edit away.
fn scaffold_config(store: &Store) -> Result<()> {
    let path = store.root().join(dot::config::CONFIG_FILE);
    if !path.exists() {
        std::fs::write(&path, dot::config::SCAFFOLD)
            .with_context(|| format!("cannot write {}", path.display()))?;
        println!(
            "Created {} — the sync remote and the validation command, ready to edit",
            path.display()
        );
    }
    Ok(())
}

/// Run the validation tests of the project: the `[verify] command`
/// of `.tod.toml`, executed at the repository root through the
/// shell. The exit code of the command is tod's exit code, so an
/// agent, a hook or a CI can branch on the result.
pub fn verify() -> Result<()> {
    let store = Store::discover()?;
    verify_in(&store)
}

fn verify_in(store: &Store) -> Result<()> {
    let command = store.config()?.verify.command.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "no validation command configured — set [verify] command in {} at the root",
            dot::config::CONFIG_FILE
        )
    })?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(store.root())
        .status()
        .with_context(|| format!("cannot run '{command}'"))?;
    if status.success() {
        println!("Verified: '{command}' passed");
        Ok(())
    } else {
        println!("Failed: '{command}'");
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Install (or remove) the agent hooks — the deterministic layer
/// besides the skill: the context of the branch injected at session
/// start, the reminder to check the plan steps at the stop. With a
/// `command` ("context" | "reminder"), run the hook the agent's
/// config invokes.
pub fn hooks(command: Option<&str>, agent: Option<&str>, remove: bool) -> Result<()> {
    match command {
        None => {
            let store = Store::discover()?;
            let agent = match agent {
                Some(flag) => parse_agent(flag)?,
                None => choose_agent_interactive()?,
            };
            if remove {
                if hooks_mod::remove(&store, agent)? {
                    println!("Removed the {} hooks", agent.label());
                } else {
                    println!("No tod hooks installed for {}", agent.label());
                }
            } else {
                let path = hooks_mod::install(&store, agent)?;
                println!("Installed the {} hooks: {}", agent.label(), path.display());
                println!("Commit them so every session in this repository runs them.");
            }
        }
        Some("context") => hooks_mod::context_command()?,
        Some("reminder") => hooks_mod::reminder_command()?,
        Some(other) => bail!("unknown hook command '{other}' (context | reminder)"),
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

    // the plan command scaffolds the spec: the detailed prompt the
    // human or the agent fills in
    let (_, dir) = store.locate(&task.uid)?;
    spec::write_scaffold(&dir, &task)
        .with_context(|| format!("cannot write {}", dir.join(spec::SPEC_FILE).display()))?;
    store.commit(&format!("tod: spec {}", &task.uid[..7]))?;

    println!(
        "Planned #{} {} — fill the prompt with: tod spec {} -e",
        &task.uid[..5],
        task.description,
        &task.uid[..5]
    );
    Ok(())
}

pub fn sub(identifier: &str, step: &str) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (_, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;

    let mut plan = Plan::load(&dir)?;
    let number = plan.add_step(step);
    Plan::save(&dir, &plan)?;
    store.commit("tod: update plan")?;

    println!("Added step {number} to #{}: {step}", &task.uid[..5]);
    Ok(())
}

pub fn check(identifier: &str, number: usize) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (_, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;

    let mut plan = Plan::load(&dir)?;
    let step = plan.check_step(number)?;
    Plan::save(&dir, &plan)?;
    store.commit("tod: update plan")?;

    let mark = if step.done { 'x' } else { ' ' };
    println!(
        "[{mark}] step {number} of #{}: {}",
        &task.uid[..5],
        step.text
    );
    Ok(())
}

/// Print the handoff journal of a task, or append a dated entry:
/// every command of the tod workflow reads the journal through
/// the brief — this is where the traversal state lives.
pub fn log(identifier: &str, note: Option<&str>) -> Result<()> {
    let store = Store::discover()?;
    log_in(&store, identifier, note)
}

fn log_in(store: &Store, identifier: &str, note: Option<&str>) -> Result<()> {
    store.ensure_init()?;
    let (_, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;

    match note {
        Some(text) => {
            let author = git::user_name();
            let entry = dot::log::append(&dir, &author, text)?;
            store.commit(&format!("tod: log {}", &task.uid[..7]))?;
            println!(
                "Logged to #{}: {} - {}",
                &task.uid[..5],
                entry.timestamp,
                entry.author
            );
        }
        None => {
            let journal = dot::log::Journal::read(&dir)?;
            if journal.is_empty() {
                println!(
                    "No journal entry for #{} — add one: tod log {} \"<note>\"",
                    &task.uid[..5],
                    &task.uid[..7]
                );
            } else {
                for entry in journal.recent() {
                    println!("{entry}\n");
                }
            }
        }
    }
    Ok(())
}

/// Print the spec of a task — scaffolding it
/// when missing. The spec is authored (edited with `tod spec -e`),
/// never generated: this is what the agent reads first.
pub fn brief(identifier: &str) -> Result<()> {
    let store = Store::discover()?;
    let text = brief_in(&store, identifier)?;
    println!("{text}");
    Ok(())
}

/// Render the brief of a task: the title, the journal of the work
/// in progress (most recent first, capped), then the authored spec —
/// the resume of a session sees where the work stands before the
/// prompt of the past.
fn brief_in(store: &Store, identifier: &str) -> Result<String> {
    store.ensure_init()?;
    let (_, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;
    let path = dir.join(spec::SPEC_FILE);

    if !path.is_file() {
        spec::write_scaffold(&dir, &task)
            .with_context(|| format!("cannot write {}", path.display()))?;
        store.commit(&format!("tod: spec {}", &task.uid[..7]))?;
    }

    let mut out = format!("# {}\n", task.description);

    // the journal: the 10 latest entries, most recent first
    let journal = dot::log::Journal::read(&dir)?;
    if !journal.is_empty() {
        let total = journal.entries.len();
        out.push_str(&format!("\n## Journal ({total} entries)\n\n"));
        for entry in journal.recent().into_iter().take(10) {
            out.push_str(&format!("{entry}\n\n"));
        }
    }

    out.push('\n');
    // the spec: its scaffolded title line is replaced by the live
    // description above, since description evolves in task.tod —
    // any other first line is kept
    let spec = fs::read_to_string(&path)?;
    let mut lines = spec.lines();
    match lines.next() {
        Some(first) if first.starts_with("# ") => {}
        Some(first) => out.push_str(first),
        None => {}
    }
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
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
            println!(
                "Created {} — fill the scenarios, or edit them with --edit",
                path.display()
            );
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

/// Print the spec.md of a task — its detailed prompt, scaffolded
/// when missing — or open it in the editor with --edit.
pub fn spec(identifier: &str, edit: bool) -> Result<()> {
    let store = Store::discover()?;
    store.ensure_init()?;
    let (_, task) = store.find_task(identifier)?;
    let dir = store.locate(&task.uid)?.1;
    let path = dir.join(spec::SPEC_FILE);

    if !path.is_file() {
        spec::write_scaffold(&dir, &task)
            .with_context(|| format!("cannot write {}", path.display()))?;
        store.commit(&format!("tod: spec {}", &task.uid[..7]))?;
        if !edit {
            println!(
                "Created {} — fill the prompt, or edit it with --edit",
                path.display()
            );
        }
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
        println!(
            "Cleared the tags of #{} {}",
            &task.uid[..5],
            task.description
        );
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
    println!("Moved #{} {} to {target}", &task.uid[..5], task.description);
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
    println!(
        "Moved #{} {} to {destination}",
        &task.uid[..5],
        task.description
    );
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
        &task.uid[..5],
        task.description
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

    println!("Finished '{branch}': merged into '{base}', {count} task(s) done, branch deleted");
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
        bail!(
            "task #{} has no working branch (start it first)",
            &task.uid[..5]
        );
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

    // push both, blocking: the testers may be elsewhere and the
    // console reads the sync remote
    if store.has_remote() {
        let remote = store.remote()?;
        for to_push in [git::TEST_BRANCH, branch.as_str()] {
            if let Err(e) = git::push(root, &remote, to_push) {
                println!(
                    "warning: {e} — push '{to_push}' manually for the testers and the console"
                );
            }
        }
    }

    println!(
        "Task #{} moved to test: '{}' merged into '{}'{}",
        &task.uid[..5],
        branch,
        git::TEST_BRANCH,
        if store.has_remote() { " (pushed)" } else { "" }
    );
    Ok(())
}

/// Mark a task as ready for review and push its working branch, so
/// the review can happen in the web console.
pub fn rfr(identifier: &str) -> Result<()> {
    let store = Store::discover()?;
    let task = store.ready_for_review(identifier)?;

    // push the branch so the console can show the diff of the
    // work: blocking, because a silent failure would break the
    // review (rfr runs once per task, the latency is worth it)
    let branch = git::current_branch_in(store.root())?;
    if branch != "main" && store.has_remote() {
        let remote = store.remote()?;
        match git::push(store.root(), &remote, &branch) {
            Ok(()) => println!("Pushed '{branch}' to {remote}"),
            Err(e) => println!(
                "warning: {e} — the console cannot show the diff until '{branch}' is pushed"
            ),
        }
    }

    println!("Ready for review #{} {}", &task.uid[..5], task.description);
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
        let ahead = git(
            dir.path(),
            &[
                "rev-list",
                "--count",
                &format!("main..{}", git::TEST_BRANCH),
            ],
        );
        assert_eq!(ahead, "0");
    }

    #[test]
    fn start_fails_when_the_branch_already_exists() {
        let (dir, store) = git_repo();
        let task = store.add_in(true, "Fix the login bug", "tester").unwrap();
        start_in(&store, &task.uid[..7], None, "main").unwrap();
        let taken = format!("t/{}-fix-the-login-bug", &task.uid[..7]);

        let task2 = store
            .add_in(true, "Fix the login bug again", "tester")
            .unwrap();
        // the generated name is free, but an existing name must fail
        assert!(start_in(&store, &task2.uid[..7], Some(taken.clone()), "main").is_err());
        // nothing moved: the second task is still in the backlog
        assert_eq!(store.load_backlog().unwrap().len(), 1);
        assert_eq!(git::current_branch_in(dir.path()).unwrap(), taken);
    }

    #[test]
    fn verify_runs_the_command_of_the_project_config() {
        let (dir, _store) = git_repo();
        std::fs::write(
            dir.path().join(dot::config::CONFIG_FILE),
            "[verify]\ncommand = \"touch verified.txt\"\n",
        )
        .unwrap();
        verify_in(&Store::new(dir.path())).unwrap();
        assert!(dir.path().join("verified.txt").is_file());
    }

    #[test]
    fn verify_without_a_configured_command_explains_itself() {
        let (dir, store) = git_repo();
        let error = verify_in(&store).unwrap_err().to_string();
        assert!(error.contains("[verify] command"), "{error}");
        assert!(error.contains(dot::config::CONFIG_FILE), "{error}");
        assert!(!dir.path().join(dot::config::CONFIG_FILE).exists());
    }

    #[test]
    fn verify_on_a_broken_config_fails_loudly() {
        let (dir, _store) = git_repo();
        std::fs::write(
            dir.path().join(dot::config::CONFIG_FILE),
            "not [ valid toml",
        )
        .unwrap();
        let error = verify_in(&Store::new(dir.path())).unwrap_err().to_string();
        assert!(error.contains(dot::config::CONFIG_FILE), "{error}");
        assert!(error.contains("cannot parse"), "{error}");
    }

    #[test]
    fn the_config_is_scaffolded_once_and_never_overwritten() {
        let (dir, store) = git_repo();
        scaffold_config(&store).unwrap();
        let path = dir.path().join(dot::config::CONFIG_FILE);
        let scaffold = std::fs::read_to_string(&path).unwrap();
        assert!(scaffold.contains("[sync]"));
        assert!(scaffold.contains("[verify]"));

        // an edited config is never overwritten
        std::fs::write(&path, "[verify]\ncommand = \"x\"\n").unwrap();
        scaffold_config(&store).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[verify]\ncommand = \"x\"\n"
        );
    }

    #[test]
    fn log_appends_dated_entries_and_reads_the_newest_first() {
        let (_dir, store) = git_repo();
        let task = store.add_in(true, "Fix the login bug", "tester").unwrap();

        // reading an empty journal explains itself instead of pretending
        log_in(&store, &task.uid[..7], None).unwrap();

        log_in(&store, &task.uid[..7], Some("first entry")).unwrap();
        log_in(
            &store,
            &task.uid[..7],
            Some("blocked: the cache does not invalidate"),
        )
        .unwrap();
        let dir = store.locate(&task.uid).unwrap().1;
        let journal = dot::log::Journal::read(&dir).unwrap();
        assert_eq!(journal.entries.len(), 2);
        // most recent first when read
        assert_eq!(
            journal.recent()[0].text,
            "blocked: the cache does not invalidate"
        );

        // an empty entry is refused, and a bad identifier too
        assert!(log_in(&store, &task.uid[..7], Some("  ")).is_err());
        assert!(log_in(&store, "zzzzz", Some("note")).is_err());
        assert!(store.find_task("zzzzz").is_err());
        // the refusals left the journal untouched
        assert_eq!(journal.entries.len(), 2);
    }

    #[test]
    fn brief_shows_the_journal_before_the_authored_spec() {
        let (_dir, store) = git_repo();
        let task = store.add_in(true, "Fix the login bug", "tester").unwrap();
        log_in(
            &store,
            &task.uid[..7],
            Some("finished the module, tests green"),
        )
        .unwrap();

        let text = brief_in(&store, &task.uid[..7]).unwrap();
        // the title comes from the task data, once
        assert!(text.starts_with("# Fix the login bug\n"), "{text}");
        assert_eq!(text.matches("# Fix the login bug").count(), 1, "{text}");
        // the journal before the spec sections
        let journal_at = text.find("## Journal (1 entries)").unwrap();
        let entry_at = text.find("finished the module, tests green").unwrap();
        let spec_at = text.find("## Analyse de l'existant").unwrap();
        assert!(journal_at < entry_at && entry_at < spec_at, "{text}");
    }

    #[test]
    fn brief_without_a_journal_is_only_the_task_and_the_spec() {
        let (_dir, store) = git_repo();
        let task = store.add_in(true, "Fix the login bug", "tester").unwrap();
        let text = brief_in(&store, &task.uid[..7]).unwrap();
        assert!(text.starts_with("# Fix the login bug\n"), "{text}");
        assert!(!text.contains("## Journal"), "{text}");
        assert!(text.contains("## Analyse de l'existant"), "{text}");
    }
}

/// Print the shell completion script. The installation one-liner:
/// `eval "$(tod completions <shell>)"`.
pub fn completions(shell: &str) -> Result<()> {
    let shell = shell.to_lowercase();
    if !matches!(
        shell.as_str(),
        "bash" | "zsh" | "fish" | "powershell" | "elvish"
    ) {
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
