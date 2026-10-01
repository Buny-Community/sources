#!/usr/bin/env python3
"""Post health check results to a Discord webhook.

Reads the per-source JSON reports and posts one embed summarising them. Silent
when DISCORD_WEBHOOK is unset, so forks without the secret still run clean.

A webhook URL already encodes its channel, so no channel ID is needed: create it
in Discord under Server Settings -> Integrations -> Webhooks, pick the channel,
and store the URL as the DISCORD_WEBHOOK repository secret. It is a credential
(anyone holding it can post to that channel), so it belongs in Secrets rather
than Variables.
"""

import json
import os
import pathlib
import sys
import urllib.error
import urllib.request

# Discord's documented ceilings, minus headroom for the surrounding text.
MAX_FIELDS = 25
MAX_FIELD_VALUE = 1000
MAX_DESCRIPTION = 3800

COLOURS = {"fail": 0xD92D20, "blocked": 0xF79009, "warn": 0xF79009, "pass": 0x12B76A}
RANK = {"fail": 3, "blocked": 2, "warn": 1, "pass": 0}


def verdict(report):
    statuses = {c["status"] for c in report["checks"]}
    for name in ("fail", "blocked", "warn"):
        if name in statuses:
            return name
    return "pass"


def main():
    webhook = os.environ.get("DISCORD_WEBHOOK", "").strip()
    if not webhook:
        print("DISCORD_WEBHOOK not set; skipping notification")
        return 0

    reports_dir = pathlib.Path(os.environ.get("REPORTS_DIR", "reports"))
    reports = []
    for path in sorted(reports_dir.glob("*.json")):
        try:
            reports.append(json.loads(path.read_text()))
        except (OSError, ValueError) as e:
            print(f"skipping {path}: {e}", file=sys.stderr)
    if not reports:
        print("no reports found; skipping notification")
        return 0

    worst = max((verdict(r) for r in reports), key=lambda v: RANK[v])
    failing = [r for r in reports if verdict(r) == "fail"]

    # On a schedule, stay quiet unless something is actually wrong: a daily
    # "all good" ping is noise that trains people to ignore the channel.
    if os.environ.get("GITHUB_EVENT_NAME") == "schedule" and not failing:
        print(f"nothing failing (worst: {worst}); staying quiet on schedule")
        return 0

    counts = {}
    for report in reports:
        counts[verdict(report)] = counts.get(verdict(report), 0) + 1
    summary = ", ".join(f"{n} {name}" for name, n in sorted(counts.items()))

    fields = []
    for report in sorted(reports, key=lambda r: (-RANK[verdict(r)], r["source"])):
        state = verdict(report)
        if state == "pass":
            continue
        notes = [
            f"`{c['status'].upper()}` {c['name']}: {c['detail']}"
            for c in report["checks"]
            if c["status"] in ("fail", "blocked", "warn")
        ]
        value = "\n".join(notes) or "—"
        if len(value) > MAX_FIELD_VALUE:
            value = value[: MAX_FIELD_VALUE - 1] + "…"
        fields.append({"name": report["source"], "value": value, "inline": False})
    fields = fields[:MAX_FIELDS]

    run_url = (
        f"{os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/"
        f"{os.environ.get('GITHUB_REPOSITORY', '')}/actions/runs/"
        f"{os.environ.get('GITHUB_RUN_ID', '')}"
    )
    description = f"{len(reports)} sources checked — {summary}\n[View run]({run_url})"

    payload = {
        "embeds": [
            {
                "title": "Source health",
                "description": description[:MAX_DESCRIPTION],
                "color": COLOURS[worst],
                "fields": fields,
            }
        ]
    }

    request = urllib.request.Request(
        webhook,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "buny-healthcheck"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            print(f"posted to Discord ({response.status})")
    except urllib.error.HTTPError as e:
        # A broken webhook must not fail an otherwise good run.
        print(f"::warning::Discord returned {e.code}: {e.read()[:200]!r}")
    except urllib.error.URLError as e:
        print(f"::warning::could not reach Discord: {e.reason}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
