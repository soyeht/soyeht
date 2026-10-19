#!/usr/bin/env python3
"""Exercise workspace-task flows with fake agents under disposable launchd jobs."""
import subprocess

from e2e import PRODUCT_NAMES, argument_parser, require, require_binaries, run_suite
from e2e_agents import AgentServices, events, harness_registry, open_agent, request, transcript, turn, until


FAKE_WORKTRUNK = r'''#!/usr/bin/python3
import json
import shutil
import subprocess
import sys
from pathlib import Path


def run(repository, *args, check=True):
    return subprocess.run(["/usr/bin/git", "-C", str(repository), *args],
                          check=check, capture_output=True, text=True)


def state_path(repository):
    common = Path(run(repository, "rev-parse", "--git-common-dir").stdout.strip())
    if not common.is_absolute():
        common = repository / common
    return common.resolve() / "soyeht-e2e-worktrees.json"


def load(repository):
    path = state_path(repository)
    return json.loads(path.read_text()) if path.exists() else {}


def save(repository, value):
    state_path(repository).write_text(json.dumps(value))


def changes(path):
    lines = run(path, "status", "--porcelain=v1").stdout.splitlines()
    return {
        "staged": any(line[:1] not in (" ", "?") for line in lines),
        "modified": any(len(line) > 1 and line[1] != " " for line in lines),
        "untracked": any(line.startswith("??") for line in lines),
        "renamed": any("R" in line[:2] for line in lines),
        "deleted": any("D" in line[:2] for line in lines),
    }


def listing(repository, state):
    items = []
    for branch, raw_path in sorted(state.items()):
        path = Path(raw_path)
        if not path.exists():
            continue
        items.append({
            "branch": branch,
            "head": {"sha": run(path, "rev-parse", "HEAD").stdout.strip()},
            "worktree": {"path": str(path), "changes": changes(path)},
            "default_branch": {"integration": {"reason": "ancestor"}},
        })
    print(json.dumps({"schema": 2, "items": items}))


def switch(repository, state, branch):
    path = repository.parent / "task-worktrees" / branch.replace("/", "-")
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        run(repository, "worktree", "add", "-b", branch, str(path))
    state[branch] = str(path.resolve())
    save(repository, state)
    print(json.dumps({"action": "created", "branch": branch, "path": state[branch]}))


def remove(repository, state, branch):
    path = Path(state[branch])
    run(repository, "worktree", "remove", str(path))
    state.pop(branch)
    save(repository, state)
    shutil.rmtree(path, ignore_errors=True)
    print(json.dumps({"kind": "worktree", "branch": branch, "path": str(path),
                      "branch_outcome": "not_attempted"}))


def main():
    if len(sys.argv) < 5 or sys.argv[1] != "-C":
        raise SystemExit("invalid invocation")
    repository = Path(sys.argv[2]).resolve()
    arguments = sys.argv[3:]
    state = load(repository)
    with state_path(repository).with_suffix(".calls").open("a") as log:
        log.write(" ".join(arguments) + "\n")
    if arguments[0] == "list":
        listing(repository, state)
    elif arguments[0] == "switch":
        switch(repository, state, arguments[-1])
    elif arguments[0] == "remove" and arguments[-1] in state:
        remove(repository, state, arguments[-1])
    else:
        raise SystemExit("unsupported invocation")


if __name__ == "__main__":
    main()
'''


class TaskServices(AgentServices):
    def __init__(self, binaries):
        super().__init__(binaries)
        executable = self.binaries / "wt"
        executable.write_text(FAKE_WORKTRUNK)
        executable.chmod(0o700)
        self.prepend_path("engine", self.binaries)


def repository(services):
    path = services.data / "repository"
    path.mkdir()
    subprocess.run(["git", "init", "-b", "main"], cwd=path, check=True, capture_output=True)
    subprocess.run(["git", "config", "user.name", "Soyeht Test"], cwd=path, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=path, check=True)
    (path / "src").mkdir()
    (path / "src/lib.rs").write_text("pub fn run(value: u32) -> u32 { value + 1 }\n")
    subprocess.run(["git", "add", "."], cwd=path, check=True)
    subprocess.run(["git", "-c", "commit.gpgsign=false", "commit", "-m", "baseline"],
                   cwd=path, check=True, capture_output=True)
    return path


def create_task(client, workspace, key, title="Run the workflow"):
    return client.mutate(
        "task.create", workspaceId=workspace, title=title,
        description="Implement the requested change", howToVerify="Run the task checks",
        mustNotChange="Keep the protocol stable", priority="high", createWorktree=True,
        executor={"workspaceDefault": False, "harness": "test", "permission": "ask"},
        dependencies=[], idempotencyKey=key,
    )["task"]


def show(client, workspace, task):
    return client.call("task.show", {"workspaceId": workspace, "taskId": task})


def wait_task(client, workspace, task, state, seconds=25):
    return until(
        lambda: (value := show(client, workspace, task))["task"]["state"] == state and value,
        f"task did not reach {state}", seconds=seconds,
    )


def contains_text(client, pane, fragment):
    return any(fragment in item.get("text", "") for item in transcript(client, pane))


def acceptance_create(services, client, workspace, results):
    invalid = request(client, "task.create", {
        "workspaceId": workspace, "title": "Invalid", "mustNotChange": "",
        "idempotencyKey": "invalid-task",
    })
    require(invalid["error"]["data"]["kind"] == "invalid_params",
            "empty preservation boundary was accepted")
    task = create_task(client, workspace, "create-task")
    require(task["taskId"] == "WS-1" and task["createdBy"] == "you",
            "valid task lost its id or author")
    require(any(item["task"]["taskId"] == task["taskId"]
                for item in events(services, "task.created")),
            "task creation event is missing")
    results.append({"acceptance": 1, "name": "create_rejects_empty_boundary_and_records_author",
                    "passed": True})
    return task


def wait_dispatched(client, workspace, task, pane):
    return until(
        lambda: (value := show(client, workspace, task))["task"].get("worktree")
        and client.pane(pane).get("agent", {}).get("sessionId") and value,
        "dispatch did not prepare the worktree and agent pane",
    )


def assert_first_prompt(client, pane, worktree):
    for required in [
        "Run the workflow", "Implement the requested change", "Keep the protocol stable",
        "Run the task checks", worktree,
    ]:
        require(contains_text(client, pane, required), f"first prompt omitted {required}")


def acceptance_dispatch_workflow(services, client, workspace, task, results):
    params = {
        "workspaceId": workspace, "taskId": task["taskId"],
        "preset": {"workspaceDefault": False, "harness": "test", "permission": "ask"},
        "idempotencyKey": "dispatch-task",
    }
    dispatched = client.call("task.dispatch", params)
    pane = dispatched["paneId"]
    current = wait_dispatched(client, workspace, task["taskId"], pane)
    assert_first_prompt(client, pane, current["task"]["worktree"]["path"])
    duplicate = request(client, "task.dispatch", {**params, "idempotencyKey": "dispatch-again"})
    require(duplicate["error"]["data"]["kind"] == "task_has_owner",
            "owned task accepted another dispatch")
    until(lambda: client.pane(pane)["state"] == "idle", "first task prompt did not finish")
    turn(services, client, pane, "task-execute")
    completed = wait_task(client, workspace, task["taskId"], "completed")
    require(len(completed["subitems"]) == 2
            and all(item["state"] == "done" for item in completed["subitems"]),
            "fake agent did not finish its declared subitems")
    require(any(item["text"] == "Planning milestone completed" for item in completed["comments"]),
            "fake agent milestone was not persisted")
    require(any(item["author"] == "reviewer" and "No public functions" in item["text"]
                for item in completed["comments"]),
            "reviewer result was not added to task comments")
    require(len(completed["attempts"]) == 1
            and completed["attempts"][0]["result"] == "completed",
            "completed workflow did not close exactly one attempt")
    results.append({"acceptance": 2, "name": "dispatch_runs_agent_and_reviewer_workflow",
                    "passed": True})


def prompt_claim(client, pane, key):
    return client.mutate("pane.prompt", paneId=pane, text="task-claim", idempotencyKey=key)


def race_result(client, first, second):
    values = [contains_text(client, pane, '"willRestartSession":true') for pane in (first, second)]
    empties = [contains_text(client, pane, '"empty":true') for pane in (first, second)]
    return sum(values) == 1 and sum(empties) == 1


def acceptance_task_next_race(client, workspace, results):
    task = create_task(client, workspace, "race-task", "Claim exactly once")
    first = open_agent(client, workspace, "test")
    second = open_agent(client, workspace, "test")
    before = {pane: client.pane(pane)["generation"] for pane in (first, second)}
    prompt_claim(client, first, "race-first")
    prompt_claim(client, second, "race-second")
    until(lambda: race_result(client, first, second), "task.next race did not settle", seconds=25)
    claimed = until(
        lambda: claimed_task(client, workspace, task["taskId"]),
        "task.next winner did not finish dispatch", seconds=25,
    )
    owner = claimed["task"]["ownerPaneId"]
    require(owner in (first, second) and len(claimed["attempts"]) == 1,
            "task.next race created an invalid owner or duplicate attempt")
    restarted = until(
        lambda: restarted_pane(client, owner, before[owner], claimed["task"]["worktree"]["path"]),
        "task.next winner did not restart in its worktree", seconds=25,
    )
    require(restarted["agent"].get("sessionId"),
            "task.next did not restart the winning pane in its worktree")
    results.append({"acceptance": 3, "name": "task_next_race_has_one_restarted_winner",
                    "passed": True})
    return second if owner == first else first


def claimed_task(client, workspace, task):
    value = show(client, workspace, task)
    if value["task"].get("ownerPaneId") and value["task"].get("worktree"):
        return value
    return None


def restarted_pane(client, pane, generation, path):
    value = client.pane(pane)
    if (value["generation"] > generation and value["cwd"] == path
            and value["agent"].get("sessionId")):
        return value
    return None


def loop_config(client, pane):
    return next((item for item in client.snapshot()[1].get("loops", [])
                 if item["paneId"] == pane), None)


def acceptance_empty_loop(services, client, pane, results):
    configured = request(client, "loop.set", {
        "paneId": pane, "prompt": "task-claim", "interval": 1, "maxRounds": 10,
        "stopWhen": "three_empty", "enabled": True, "idempotencyKey": "empty-loop",
    })
    require("error" not in configured, f"loop configuration failed: {configured}")
    stopped = until(
        lambda: (value := loop_config(client, pane))
        and not value["config"]["enabled"] and value,
        "empty loop did not stop", seconds=25,
    )
    ticks = [item for item in events(services, "loop.tick") if item["paneId"] == pane]
    config = stopped["config"]
    require(config["rounds"] == 3 and config["emptyResults"] == 3 and len(ticks) == 3,
            "empty loop did not stop after exactly three dispatched rounds")
    results.append({"acceptance": 4, "name": "loop_stops_after_three_empty_task_results",
                    "passed": True})


def run_acceptances(services, results):
    harness_registry(services)
    client = services.client("task-acceptance")
    workspace = client.mutate(
        "workspace.create", name="Task acceptances", path=str(repository(services)),
        idempotencyKey="task-workspace",
    )["workspace"]["workspaceId"]
    task = acceptance_create(services, client, workspace, results)
    acceptance_dispatch_workflow(services, client, workspace, task, results)
    idle = acceptance_task_next_race(client, workspace, results)
    acceptance_empty_loop(services, client, idle, results)


def main():
    args = argument_parser(__doc__).parse_args()
    binaries = args.bin_dir.resolve()
    require_binaries(binaries, (*PRODUCT_NAMES, "soyeht-fake-acp"),
                     "build workspace binaries before running task acceptances")
    services = TaskServices(binaries)
    run_suite(services, lambda: services.install_fake_acp(binaries),
              lambda results: run_acceptances(services, results), 4, args.report)


if __name__ == "__main__":
    main()
