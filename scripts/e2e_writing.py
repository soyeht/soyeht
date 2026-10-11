#!/usr/bin/env python3
"""Exercise writing documents under disposable launchd jobs."""
import argparse
import hashlib
import json
import shutil
import struct
import time
import uuid
from pathlib import Path

from e2e import Client, ROOT, receive, require, send
from e2e_agents import AgentServices, harness_registry, open_agent, request, until
from e2e_platform import connect_endpoint, database
from generated_profile import DEV_ENGINE_SOCKET


class WritingServices(AgentServices):
    def binding_client(self, pane_id):
        with database(self.data / "soyeht.db") as db:
            raw = db.execute(
                "SELECT payload FROM agent_runtime WHERE pane_id=?", (pane_id,)
            ).fetchone()[0]
        binding = json.loads(raw)["binding"]
        client = object.__new__(Client)
        client.stream = connect_endpoint(self.data / DEV_ENGINE_SOCKET)
        require(receive(client.stream).get("method") == "auth.challenge", "missing challenge")
        client.next = 0
        client.pending = []
        client.welcome = client.call(
            "hello",
            {
                "client": "writing-bound-agent",
                "clientVersion": "1",
                "protocolVersion": 1,
                "binding": binding,
            },
        )
        self.clients.append(client)
        return client


def repository(services):
    root = services.data / "writing-repository"
    for section in ("Texts", "Presentations", "Screenplays"):
        (root / "Writing" / section).mkdir(parents=True)
    (root / "Writing/Texts/field-notes.md").write_text(
        "# Field notes\n\nDraft safely.\n"
    )
    (root / "Writing/Presentations/review.md").write_text(
        "# Opening\nSpeaker note\n\nImage here\n---\n# Closing\nClosing note\n"
    )
    (root / "Writing/Screenplays/night-train.fountain").write_text(
        "INT. STATION - NIGHT\n\nANA\nWhat time is it?\n\n"
        "EXT. PLATFORM - DAWN\n\nThe train arrives.\n"
    )
    return root


def open_document(client, workspace, path, kind, key):
    return client.mutate(
        "writing.document.open",
        workspaceId=workspace,
        path=path,
        kind=kind,
        idempotencyKey=key,
    )


def acceptance_sections(client, workspace, results):
    created = client.mutate(
        "writing.document.create",
        workspaceId=workspace,
        section="texts",
        title="New chapter",
        relativeFolder="Drafts",
        idempotencyKey="create-text",
    )
    repeated = client.mutate(
        "writing.document.create",
        workspaceId=workspace,
        section="texts",
        title="New chapter",
        relativeFolder="Drafts",
        idempotencyKey="create-text",
    )
    require(created == repeated, "writing creation was not idempotent")
    require(
        created["document"]["file"]["path"] == "Writing/Texts/Drafts/new-chapter.md",
        "new text was created outside its section",
    )
    sections = {
        section: client.call(
            "writing.section.list", {"workspaceId": workspace, "section": section, "limit": 50}
        )["entries"]
        for section in ("texts", "presentations", "screenplays")
    }
    require(all(sections.values()), "a writing section was empty")
    require(
        all(item["kind"] == "text" for item in sections["texts"])
        and all(item["kind"] == "presentation" for item in sections["presentations"])
        and all(item["kind"] == "screenplay" for item in sections["screenplays"]),
        "writing sections mixed document kinds",
    )
    escaped = request(
        client,
        "writing.document.open",
        {
            "workspaceId": workspace,
            "path": "Writing/Texts/../Presentations/review.md",
            "kind": "text",
            "idempotencyKey": "escaped-section",
        },
    )
    require(
        escaped["error"]["data"]["kind"] == "writing_section_refused",
        "cross-section path was accepted",
    )
    results.append({"acceptance": 1, "name": "sections_create_list_and_never_mix", "passed": True})


def acceptance_text(services, client, workspace, root, results):
    opened = open_document(
        client, workspace, "Writing/Texts/field-notes.md", "text", "open-text"
    )
    file_id = opened["document"]["file"]["fileId"]
    preferred = client.mutate(
        "writing.preferences.set",
        fileId=file_id,
        baseRevision="1",
        font="sans",
        focusMode=True,
        idempotencyKey="text-preferences",
    )
    require(preferred["document"]["focusMode"], "focus mode was not persisted")
    formatted = client.mutate(
        "writing.format",
        fileId=file_id,
        baseRevision="1",
        range={"start": {"line": 2, "column": 0}, "end": {"line": 2, "column": 5}},
        kind="bold",
        idempotencyKey="text-bold",
    )
    require(formatted["file"]["revision"] == "2", "format did not advance one revision")
    path = root / "Writing/Texts/field-notes.md"
    until(lambda: path.read_text() == "# Field notes\n\n**Draft** safely.\n", "text did not autosave")
    client = services.restart_engine()
    restored = next(
        item for item in client.snapshot()[1]["writing"] if item["file"]["fileId"] == file_id
    )
    require(
        restored["file"]["revision"] == "2"
        and restored["font"] == "sans"
        and restored["focusMode"],
        "text revision or preferences did not survive restart",
    )
    results.append({"acceptance": 2, "name": "text_format_focus_and_restart", "passed": True})
    return client


def send_attachment(client, attachment_id, payload):
    body = uuid.UUID(attachment_id).bytes + payload
    client.stream.sendall(struct.pack(">BI", 3, len(body)) + body)


def acceptance_presentation(client, workspace, root, results):
    opened = open_document(
        client,
        workspace,
        "Writing/Presentations/review.md",
        "presentation",
        "open-presentation",
    )
    file_id = opened["document"]["file"]["fileId"]
    structure = client.call("writing.structure", {"fileId": file_id, "revision": "1"})
    require(
        len(structure["slides"]) == 2
        and structure["slides"][0]["heading"] == "Opening"
        and "Speaker note" in structure["slides"][0]["notes"],
        "presentation segmentation lost heading or notes",
    )
    attachment_id = "30415263-7485-496a-bbcc-ddeeff001122"
    image = b"\x89PNG\r\n\x1a\nwriting-e2e-image"
    send_attachment(client, attachment_id, image)
    formatted = client.mutate(
        "writing.format",
        fileId=file_id,
        baseRevision="1",
        range={"start": {"line": 3, "column": 0}, "end": {"line": 3, "column": 10}},
        kind="image",
        image={
            "attachmentId": attachment_id,
            "mimeType": "image/png",
            "name": "slide.png",
            "bytes": str(len(image)),
        },
        idempotencyKey="presentation-image",
    )
    digest = hashlib.sha256(image).hexdigest()
    asset = root / f"Writing/Presentations/assets/{digest}.png"
    require(formatted["file"]["revision"] == "2", "image paste did not advance revision")
    require(asset.read_bytes() == image, "content-addressed presentation asset was not published")
    read = client.call("file.read", {"fileId": file_id, "fromLine": 0, "lineCount": 20})
    require(
        any(f"assets/{digest}.png" in line["text"] for line in read["lines"]),
        "presentation Markdown did not reference the durable image",
    )
    results.append({"acceptance": 3, "name": "presentation_segments_and_persists_image", "passed": True})


def acceptance_screenplay(services, client, workspace, results):
    opened = open_document(
        client,
        workspace,
        "Writing/Screenplays/night-train.fountain",
        "screenplay",
        "open-screenplay",
    )
    file_id = opened["document"]["file"]["fileId"]
    structure = client.call("writing.structure", {"fileId": file_id, "revision": "1"})
    require(len(structure["scenes"]) == 2, "screenplay scene navigator was incomplete")
    dialogue = next(item for item in structure["elements"] if item["kind"] == "dialogue")
    pane = open_agent(client, workspace, "test")
    requested = client.mutate(
        "writing.variant.request",
        fileId=file_id,
        lineId=dialogue["elementId"],
        baseRevision="1",
        agentPaneId=pane,
        instruction="Write a quieter alternate.",
        idempotencyKey="request-variant",
    )
    require(requested["delivered"], "variant request did not reach the agent")
    agent = services.binding_client(pane)
    added = agent.mutate(
        "writing.variant.add",
        fileId=file_id,
        lineId=dialogue["elementId"],
        baseRevision="1",
        text="Is it late already?",
        idempotencyKey="add-variant",
    )
    alternate = added["variants"]["alternates"][1]
    denied = request(
        agent,
        "writing.variant.select",
        {
            "fileId": file_id,
            "lineId": dialogue["elementId"],
            "variantId": alternate["variantId"],
            "baseRevision": "1",
            "idempotencyKey": "agent-select",
        },
    )
    require(denied["error"]["data"]["kind"] == "forbidden_role", "agent selected a variant")
    selected = client.mutate(
        "writing.variant.select",
        fileId=file_id,
        lineId=dialogue["elementId"],
        variantId=alternate["variantId"],
        baseRevision="1",
        idempotencyKey="writer-select",
    )
    require(
        selected["variants"]["selectedIndex"] == 1
        and selected["document"]["file"]["revision"] == "2",
        "writer selection did not become canonical",
    )
    client = services.restart_engine()
    restored = client.call("writing.structure", {"fileId": file_id, "revision": "2"})
    require(
        restored["variants"][0]["selectedIndex"] == 1
        and restored["variants"][0]["alternates"][1]["author"]["actor"]["paneId"] == pane,
        "agent-authored variant or writer selection did not survive restart",
    )
    results.append(
        {"acceptance": 4, "name": "screenplay_variants_are_agent_authored_writer_selected", "passed": True}
    )
    return client, file_id, restored["scenes"]


def acceptance_beat_board(services, client, file_id, scenes, results):
    first = client.mutate(
        "writing.beat.put",
        fileId=file_id,
        baseRevision="2",
        title="Opening image",
        body="A clock fills the station.",
        category="setting",
        order=0,
        sceneId=scenes[0]["sceneId"],
        idempotencyKey="beat-first",
    )["beat"]
    second = client.mutate(
        "writing.beat.put",
        fileId=file_id,
        baseRevision="2",
        title="The question",
        body="Time becomes the theme.",
        category="story",
        order=1,
        sceneId=scenes[1]["sceneId"],
        idempotencyKey="beat-second",
    )["beat"]
    client.mutate(
        "writing.beat.put",
        fileId=file_id,
        baseRevision="2",
        beatId=first["beatId"],
        title=first["title"],
        body=first["body"],
        category=first["category"],
        order=1,
        sceneId=first["sceneId"],
        idempotencyKey="beat-reorder",
    )
    listed = client.call("writing.beat.list", {"fileId": file_id, "revision": "2"})
    require(
        [beat["beatId"] for beat in listed["beats"]] == [second["beatId"], first["beatId"]],
        "beat reordering was not canonical",
    )
    linked = client.call("writing.structure", {"fileId": file_id, "revision": "2"})
    require(
        first["beatId"] in linked["scenes"][0]["linkedBeatIds"]
        and second["beatId"] in linked["scenes"][1]["linkedBeatIds"],
        "beat links were not visible from scenes",
    )
    client = services.restart_engine()
    require(
        client.call("writing.beat.list", {"fileId": file_id, "revision": "2"}) == listed,
        "beat board changed across restart",
    )
    results.append({"acceptance": 5, "name": "beat_board_reorders_links_and_restarts", "passed": True})


def run_acceptances(services, results):
    root = repository(services)
    client = services.client("writing-e2e")
    workspace = client.mutate(
        "workspace.create",
        name="Writing acceptance",
        path=str(root),
        idempotencyKey="writing-workspace",
    )["workspace"]["workspaceId"]
    acceptance_sections(client, workspace, results)
    client = acceptance_text(services, client, workspace, root, results)
    acceptance_presentation(client, workspace, root, results)
    client, file_id, scenes = acceptance_screenplay(services, client, workspace, results)
    acceptance_beat_board(services, client, file_id, scenes, results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "engine/target/debug")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    binaries = args.bin_dir.resolve()
    names = ("soyeht", "soyeht-engine", "soyeht-mcp", "soyeht-ptyd", "soyeht-fake-acp")
    require(all((binaries / name).is_file() for name in names), "build binaries before writing e2e")
    services = WritingServices(binaries)
    started, results, failure = time.monotonic(), [], None
    try:
        shutil.copy2(binaries / "soyeht-fake-acp", services.binaries / "soyeht-fake-acp")
        harness_registry(services)
        services.start("ptyd")
        services.start("engine")
        run_acceptances(services, results)
    except Exception as error:
        failure = f"{type(error).__name__}: {error}"
    finally:
        try:
            services.close()
        except Exception as error:
            failure = f"cleanup failed: {error}"
    report = {
        "passed": failure is None and len(results) == 5,
        "seconds": round(time.monotonic() - started, 2),
        "results": results,
    }
    if failure:
        report["error"] = failure
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if not report["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
