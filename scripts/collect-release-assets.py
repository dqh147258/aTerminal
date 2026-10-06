#!/usr/bin/env python3
"""Collect verified, same-commit CI client packages without publishing anything.

Requires Python 3.10+, the official GitHub CLI and GH_TOKEN with actions:read.
All API failures are fatal (including 401/403/404); absence is only a successful
empty API result. Downloads use `gh run download`, never curl/token redirects.
Run scripts/test-collect-release-assets.py for network-free synthetic tests.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VERIFY_PATH = ".github/workflows/verify.yml"
PACKAGES_PATH = ".github/workflows/test-packages.yml"
TARGETS = {
    "x86_64-pc-windows-msvc": ("aTerminal.exe", ".zip", "Windows", "X64"),
    "aarch64-apple-darwin": ("aTerminal", ".tar.gz", "macOS", "ARM64"),
    "x86_64-apple-darwin": ("aTerminal", ".tar.gz", "macOS", "X64"),
    "x86_64-unknown-linux-gnu": ("aTerminal", ".tar.gz", "Linux", "X64"),
    "android": ("aTerminal-debug.apk", ".zip", "Linux", "X64"),
}
ANDROID_JOB = "Android debug APK (three ABIs)"
SIGNATURE_STEP = "Verify APK metadata and debug signature"
UPLOAD_STEP = "Run actions/upload-artifact@v4"
VERIFY_JOBS = {"terminal (ubuntu-latest)", "terminal (macos-latest)",
               "terminal (windows-latest)", "mobile-native", "webrtc-loopback"}
PACKAGE_JOBS = {"Desktop " + target for target in TARGETS if target != "android"} | {ANDROID_JOB}
WORKFLOWS = {VERIFY_PATH: ("Verify Terminal", VERIFY_JOBS),
             PACKAGES_PATH: ("Test Packages", PACKAGE_JOBS)}
MAX_FILE = 512 * 1024 * 1024
MAX_EXPANDED = 2 * 1024 * 1024 * 1024
WARNINGS = [
    "Prerelease test packages only; desktop CLI archives are not installers.",
    "Desktop has no distribution-identity signing or macOS notarization; linker ad-hoc signatures may exist.",
    "Android is a debuggable APK signed with a temporary CI debug key, not a production/store signing key.",
    "Android signature verification is evidenced by the source CI job step, not repeated by this collector.",
    "Changing Android debug keys may prevent an in-place update; uninstalling deletes app data.",
    "Hashes and successful CI are integrity/provenance evidence, not cryptographic build attestations or device acceptance tests.",
    "GitHub artifact-envelope digests are recorded from the API; gh unwraps that envelope. Inner package archive and every delivered file are independently SHA-256 verified.",
]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def strict_json(text):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, f"Duplicate JSON key: {key}")
            result[key] = value
        return result
    return json.loads(text, object_pairs_hook=pairs)


def read_json(path):
    require(path.stat().st_size <= 4 * 1024 * 1024, f"Oversized JSON: {path.name}")
    return strict_json(path.read_text(encoding="utf-8"))


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def timestamp(value):
    result = datetime.fromisoformat(value.replace("Z", "+00:00"))
    require(result.tzinfo is not None, "Missing timestamp timezone")
    return result


def positive_int(value, description):
    require(type(value) is int and value > 0, f"Invalid {description}")
    return value


class GitHub:
    """Read-only GitHub access through official gh, with a shared deadline."""
    def __init__(self, repo, deadline):
        self.repo, self.deadline = repo, deadline
        self.base = f"repos/{repo}"

    def command(self, arguments, timeout=120):
        remaining = self.deadline - time.monotonic()
        require(remaining > 0, "Collection deadline reached")
        env = os.environ.copy()
        env.update(GH_HOST="github.com", GH_PROMPT_DISABLED="1", GH_PAGER="cat")
        # gh debug mode can log headers. Do not inherit it into this task.
        env.pop("GH_DEBUG", None)
        result = subprocess.run(["gh", *arguments], capture_output=True, text=True,
                                timeout=min(timeout, remaining), env=env, check=False)
        if result.returncode:
            # gh sanitizes its errors; still avoid reflecting any accidental token.
            error = result.stderr.strip()
            for key in ("GH_TOKEN", "GITHUB_TOKEN"):
                if env.get(key):
                    error = error.replace(env[key], "[redacted]")
            raise RuntimeError(f"gh {arguments[0]} failed ({result.returncode}): {error[:2000]}")
        return result.stdout

    def api(self, endpoint):
        return strict_json(self.command(["api", "--hostname", "github.com", "--method", "GET", endpoint]))

    def pages(self, endpoint, field):
        pages = strict_json(self.command(["api", "--hostname", "github.com", "--method", "GET",
                                         "--paginate", "--slurp", endpoint]))
        require(isinstance(pages, list), "Expected paginated GitHub response")
        items = []
        for page in pages:
            require(isinstance(page, dict) and isinstance(page.get(field), list), f"Missing API {field}")
            items.extend(page[field])
        ids = [item.get("id") for item in items]
        require(len(ids) == len(set(ids)), f"Duplicate API {field} IDs")
        return items

    def runs(self, sha):
        return self.pages(f"{self.base}/actions/runs?head_sha={sha}&event=push&branch=main&per_page=100", "workflow_runs")

    def jobs(self, run_id):
        # filter=latest alone may omit jobs retained from earlier attempts.
        return self.pages(f"{self.base}/actions/runs/{run_id}/jobs?filter=all&per_page=100", "jobs")

    def artifacts(self, run_id):
        return self.pages(f"{self.base}/actions/runs/{run_id}/artifacts?per_page=100", "artifacts")

    def download(self, run_id, artifact, destination):
        self.command(["run", "download", str(run_id), "--repo", self.repo,
                      "--name", artifact["name"], "--dir", str(destination)], timeout=600)
        fresh = self.api(f"{self.base}/actions/artifacts/{artifact['id']}")
        for key in ("id", "name", "digest", "workflow_run", "expired"):
            require(fresh.get(key) == artifact.get(key), f"Artifact changed during download: {key}")


def run_matches(run, repo, sha, path):
    return (run.get("head_sha") == sha and run.get("head_branch") == "main"
            and run.get("event") == "push" and run.get("path") == path
            and run.get("name") == WORKFLOWS[path][0]
            and run.get("repository", {}).get("full_name") == repo
            and run.get("head_repository", {}).get("full_name") == repo
            and run.get("repository", {}).get("id") == run.get("head_repository", {}).get("id"))


def latest_run(runs, repo, sha, path):
    candidates = [run for run in runs if run_matches(run, repo, sha, path)]
    return max(candidates, key=lambda run: positive_int(run.get("id"), "run ID")) if candidates else None


def original_execution(latest, candidates):
    """Resolve GitHub's retained-success clone, never a newly executed retry.

    For rerun-failed-jobs GitHub may assign retained jobs new IDs/attempt numbers
    while preserving their original execution and steps. The clone's created_at
    is after its completed_at; an original successful record must independently
    prove the complete identical execution before an older artifact is eligible.
    """
    if not (latest.get("status") == "completed" and latest.get("conclusion") == "success"
            and latest.get("created_at") and latest.get("completed_at")
            and timestamp(latest["created_at"]) > timestamp(latest["completed_at"])):
        return None
    step_fields = {"name", "status", "conclusion", "number", "started_at", "completed_at"}
    require(latest.get("steps") and all(step_fields <= set(step) for step in latest["steps"]),
            "Retained-job clone is missing full step execution evidence")
    matches = []
    for original in candidates:
        if (original["run_attempt"] < latest["run_attempt"]
                and original.get("status") == "completed" and original.get("conclusion") == "success"
                and original.get("created_at") and original.get("started_at")
                and timestamp(original["created_at"]) <= timestamp(original["started_at"])
                and all(original.get(key) == latest.get(key) for key in
                        ("run_id", "head_sha", "name", "started_at", "completed_at", "steps", "runner_id", "runner_name", "labels"))):
            matches.append(original)
    require(len(matches) == 1, "Retained-job clone lacks one unambiguous original successful execution")
    return matches[0]


def effective_jobs(run, jobs, expected, completed=True):
    """Latest execution per named matrix job, retaining successful earlier jobs."""
    latest = {}
    current_attempt = positive_int(run.get("run_attempt"), "run attempt")
    for job in jobs:
        require("retained_source_job" not in job, "Raw job evidence contains reserved retained_source_job field")
        require(job.get("run_id") == run["id"], "Job belongs to another workflow run")
        require(job.get("head_sha") == run["head_sha"], "Job source SHA mismatch")
        attempt = positive_int(job.get("run_attempt"), "job attempt")
        require(attempt <= current_attempt, "Job attempt is newer than run snapshot")
        name = job.get("name")
        require(isinstance(name, str), "Missing job name")
        old = latest.get(name)
        if old and old["run_attempt"] == attempt:
            raise ValueError(f"Ambiguous duplicate job {name} in attempt {attempt}")
        if old is None or old["run_attempt"] < attempt:
            latest[name] = job
    require(set(latest) <= expected, f"Unexpected workflow job set: {sorted(set(latest) - expected)}")
    for name, job in latest.items():
        if job["run_attempt"] == current_attempt and job.get("status") == "completed":
            require(job.get("conclusion") == "success", f"Required job failed: {name} ({job.get('conclusion')})")
    if completed:
        require(set(latest) == expected, f"Missing required jobs: {sorted(expected - set(latest))}")
        for name, job in latest.items():
            require(job.get("status") == "completed" and job.get("conclusion") == "success",
                    f"Required job is not successful: {name}")
    for name, job in list(latest.items()):
        original = original_execution(job, [item for item in jobs if item.get("name") == name])
        if original is not None:
            latest[name] = dict(job, retained_source_job=original)
    return latest


def inspect_runs(client, repo, sha):
    runs, ready, pending = client.runs(sha), {}, []
    # Inspect both workflows even when one is missing/pending: a definitive failure
    # in the other should be reported immediately rather than hidden by polling.
    for path, (_, expected) in WORKFLOWS.items():
        run = latest_run(runs, repo, sha, path)
        if run is None:
            pending.append(f"{path}: awaiting same-commit main push run")
            continue
        positive_int(run.get("repository", {}).get("id"), "repository ID")
        complete = run.get("status") == "completed"
        if complete:
            require(run.get("conclusion") == "success",
                    f"{path} run {run['id']} definitively ended: {run.get('conclusion')}")
        jobs = effective_jobs(run, client.jobs(run["id"]), expected, completed=complete)
        if not complete:
            pending.append(f"{path}: run {run['id']} attempt {run['run_attempt']} {run.get('status')}")
        else:
            ready[path] = {"run": run, "jobs": jobs}
    return ready if not pending else None, pending


def wait_for_runs(client, repo, sha, poll_seconds):
    previous = None
    while True:
        ready, pending = inspect_runs(client, repo, sha)
        if ready is not None:
            return ready
        message = "; ".join(pending)
        if message != previous:
            print(message, file=sys.stderr, flush=True)
            previous = message
        remaining = client.deadline - time.monotonic()
        require(remaining > 0, f"Timed out waiting for required runs: {message}")
        time.sleep(min(poll_seconds, remaining))


def required_steps(job, target):
    names = {UPLOAD_STEP}
    if target == "android":
        names |= {SIGNATURE_STEP, "Build APK and lint", "Package and verify native libraries"}
    else:
        names |= {"Run workspace tests", "Build optimized test binary", "Check executable starts", "Package and verify contents"}
        if target != "x86_64-pc-windows-msvc":
            names.add("Exercise host terminal")
    for name in names:
        matches = [step for step in job.get("steps", []) if step.get("name") == name]
        require(len(matches) == 1 and matches[0].get("status") == "completed"
                and matches[0].get("conclusion") == "success", f"Missing successful step {name} in job {job['id']}")
    return sorted(names)


def select_artifacts(run, jobs, artifacts, now=None):
    now = now or datetime.now(timezone.utc)
    selections = []
    for target in TARGETS:
        effective_job = jobs[ANDROID_JOB if target == "android" else "Desktop " + target]
        job = effective_job.get("retained_source_job", effective_job)
        steps = required_steps(job, target)
        artifact_target = "android-debug" if target == "android" else target
        name = f"aTerminal-test-{artifact_target}-{run['head_sha']}-attempt-{job['run_attempt']}"
        candidates = [artifact for artifact in artifacts if artifact.get("name") == name]
        require(len(candidates) == 1, f"Expected one exact artifact {name}; found {len(candidates)}")
        artifact = candidates[0]
        positive_int(artifact.get("id"), "artifact ID")
        require(artifact.get("expired") is False and timestamp(artifact["expires_at"]) > now,
                f"Expired artifact: {name}")
        require(0 < artifact.get("size_in_bytes", 0) <= MAX_EXPANDED, f"Invalid artifact size: {name}")
        require(re.fullmatch(r"sha256:[0-9a-f]{64}", artifact.get("digest", "")) is not None,
                f"Missing artifact API SHA-256 digest: {name}")
        source = artifact.get("workflow_run", {})
        require(source.get("id") == run["id"] and source.get("head_sha") == run["head_sha"]
                and source.get("head_branch") == "main"
                and source.get("repository_id") == run["repository"]["id"]
                and source.get("head_repository_id") == run["head_repository"]["id"],
                f"Artifact source run/repository/SHA mismatch: {name}")
        # The upload must belong to that execution, not just share its name.
        require(timestamp(job["started_at"]) <= timestamp(artifact["created_at"])
                <= timestamp(job["completed_at"]), f"Artifact upload is outside job execution: {name}")
        selections.append({"target": target, "artifact": artifact, "job": job, "effective_job": effective_job, "verified_steps": steps})
    return selections


def safe_name(name, directory=False):
    require(isinstance(name, str) and name and "\\" not in name and "\x00" not in name
            and ":" not in name and not name.startswith("/")
            and not any(ord(char) < 32 or ord(char) == 127 for char in name), f"Unsafe archive path: {name!r}")
    plain = name[:-1] if directory and name.endswith("/") else name
    require(plain and all(part not in ("", ".", "..") for part in plain.split("/"))
            and str(PurePosixPath(plain)) == plain, f"Noncanonical archive path: {name!r}")
    return plain


def check_apk_paths(path):
    with zipfile.ZipFile(path) as archive:
        require(len(archive.infolist()) <= 20000, "APK has too many entries")
        seen, total = set(), 0
        for entry in archive.infolist():
            require(entry.orig_filename == entry.filename, "APK ZIP entry name was truncated or normalized")
            name = safe_name(entry.orig_filename, entry.is_dir())
            require(name not in seen, f"Duplicate APK entry: {name}")
            seen.add(name)
            mode = entry.external_attr >> 16
            kind = stat.S_IFMT(mode)
            require(kind in (0, stat.S_IFDIR if entry.is_dir() else stat.S_IFREG)
                    and not mode & 0o7000, f"Unsafe APK entry type/mode: {name}")
            require(not entry.flag_bits & 1, "Encrypted APK entry")
            require(entry.file_size <= MAX_FILE, "Oversized APK entry")
            total += entry.file_size
            require(total <= MAX_EXPANDED, "APK expansion exceeds size limit")


def extract_package(path, destination, stem, expected):
    """Whitelist and stream ordinary files; never use extract/extractall."""
    records = {}
    total = 0

    def accept(name, size, mode, regular, stream_factory):
        nonlocal total
        safe_name(name)
        require(name.startswith(stem + "/"), "Archive root mismatch")
        relative = name[len(stem) + 1:]
        require(relative in expected and relative not in records, f"Unexpected/duplicate archive member: {name}")
        require(regular and not mode & ~0o777 and 0 < size <= MAX_FILE, f"Unsafe archive member type/mode/size: {name}")
        total += size
        require(total <= MAX_EXPANDED, "Package expansion exceeds size limit")
        output = destination / relative
        digest, length = hashlib.sha256(), 0
        with stream_factory() as source, output.open("xb") as target:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                length += len(chunk)
                require(length <= size, "Archive member expanded past declared size")
                digest.update(chunk)
                target.write(chunk)
        require(length == size, "Truncated archive member")
        records[relative] = {"path": relative, "size": size, "mode": format(mode, "04o"), "sha256": digest.hexdigest()}

    destination.mkdir()
    if path.name.endswith(".tar.gz"):
        with tarfile.open(path, "r:gz") as archive:
            for member in archive:
                accept(member.name, member.size, member.mode, member.isfile() and not member.issparse(),
                       lambda member=member: archive.extractfile(member))
    else:
        with zipfile.ZipFile(path) as archive:
            require(len(archive.infolist()) == len(expected), "ZIP package inventory count mismatch")
            for member in archive.infolist():
                require(member.orig_filename == member.filename, "Package ZIP entry name was truncated or normalized")
                safe_name(member.orig_filename)
                mode = member.external_attr >> 16
                require(not member.flag_bits & 1 and member.create_system == 3, "Unexpected encrypted/non-Unix ZIP package")
                accept(member.filename, member.file_size, stat.S_IMODE(mode), stat.S_ISREG(mode),
                       lambda member=member: archive.open(member))
    require(set(records) == expected, f"Archive inventory mismatch: missing {sorted(expected - set(records))}")
    return records


def checksum_records(text):
    result = {}
    for line in text.splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([^\r\n]+)", line)
        require(match is not None, "Malformed SHA256SUMS record")
        digest, name = match.groups()
        safe_name(name)
        require(name not in result, f"Duplicate checksum path: {name}")
        result[name] = digest
    require(result, "Empty checksum file")
    return result


def indexed_records(records):
    require(isinstance(records, list), "Missing manifest file records")
    result = {}
    for record in records:
        require(isinstance(record, dict) and isinstance(record.get("path"), str), "Invalid file record")
        name = safe_name(record["path"])
        require(name not in result, f"Duplicate manifest path: {name}")
        require(type(record.get("size")) is int and record["size"] > 0, "Invalid manifest file size")
        result[name] = record
    return result


def packager_module():
    specification = importlib.util.spec_from_file_location("test_packager", ROOT / "scripts/package-test-artifact.py")
    require(specification is not None and specification.loader is not None, "Missing package validator")
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


def validate_package(download, unpacked, selection, run, repo, packager=None):
    target, artifact, job = selection["target"], selection["artifact"], selection["job"]
    payload, suffix, runner_os, runner_arch = TARGETS[target]
    stem = f"aTerminal-test-{target}-{run['head_sha'][:12]}"
    archive_name = stem + suffix
    external_names = {archive_name, archive_name + ".sha256", stem + ".build-info.json"}
    # gh handles the Actions transport ZIP; accept only the three expected normal
    # files it produced. No directories, symlinks, unexpected files or hardlinks.
    entries = list(download.iterdir())
    require({entry.name for entry in entries} == external_names, "Downloaded artifact inventory mismatch")
    for entry in entries:
        info = entry.lstat()
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and 0 < info.st_size <= MAX_FILE,
                f"Unsafe downloaded artifact file: {entry.name}")
    archive = download / archive_name
    external = read_json(download / (stem + ".build-info.json"))
    digest = sha256(archive)
    require(checksum_records((download / (archive_name + ".sha256")).read_text(encoding="utf-8"))
            == {archive_name: digest}, "External archive SHA-256 mismatch")
    require(external.get("archive") == archive_name and external.get("archive_sha256") == digest
            and external.get("archive_size") == archive.stat().st_size
            and external.get("archive_inventory_verified") is True, "External archive summary mismatch")
    data_names = {payload, "README.md", "THIRD_PARTY.md", "TEST-PACKAGES.md"}
    if target != "android":
        data_names.add("LICENSE-Noto-CJK.txt")
    expected = data_names | {"BUILD-INFO.json", "SHA256SUMS"}
    records = extract_package(archive, unpacked, stem, expected)
    manifest = read_json(unpacked / "BUILD-INFO.json")
    require(external.get("build_info") == manifest, "Inner/outer build metadata mismatch")
    require(manifest.get("schema_version") == 1 and manifest.get("purpose") == "test-only"
            and manifest.get("source_commit") == run["head_sha"] and manifest.get("target") == target
            and manifest.get("build_profile_expected") == ("debug" if target == "android" else "release"),
            "Package source SHA/target/profile mismatch")
    expected_ci = {"GITHUB_REPOSITORY": repo, "GITHUB_SHA": run["head_sha"], "GITHUB_RUN_ID": str(run["id"]),
                   "GITHUB_RUN_ATTEMPT": str(job["run_attempt"]), "GITHUB_WORKFLOW": "Test Packages",
                   "RUNNER_OS": runner_os, "RUNNER_ARCH": runner_arch}
    require(manifest.get("ci") == expected_ci, "Package CI provenance mismatch")
    require(indexed_records(external.get("archive_files")) == records, "Archive file checksums/sizes/modes mismatch")
    require(indexed_records(manifest.get("files")) == {name: records[name] for name in data_names},
            "Inner file checksums/sizes/modes mismatch")
    require(checksum_records((unpacked / "SHA256SUMS").read_text(encoding="utf-8"))
            == {name: item["sha256"] for name, item in records.items() if name != "SHA256SUMS"},
            "Inner SHA256SUMS mismatch")
    for name, item in records.items():
        mode = int(item["mode"], 8)
        if name != payload or target == "android":
            require(mode == 0o644, f"Unexpected document/APK permissions: {name}")
        else:
            require(mode & 0o111 and (target != "x86_64-pc-windows-msvc" or mode == 0o755), "Desktop payload is not executable")
    packager = packager or packager_module()
    if target == "android":
        check_apk_paths(unpacked / payload)
        inspection = packager.android_structure(unpacked / payload)
    else:
        inspection = packager.desktop_header(unpacked / payload, target)
    require(inspection == manifest.get("header_or_apk_inspection"), "Binary/APK inspection differs from metadata")
    require(isinstance(manifest.get("signing_notes"), str) and manifest["signing_notes"], "Missing signing limitation")
    return {"archive": archive, "payload": unpacked / payload, "external_names": sorted(external_names),
            "archive_sha256": digest, "inspection": inspection, "signing_notes": manifest["signing_notes"]}


def compact_run(value):
    run = value["run"]
    result = {key: run[key] for key in ("id", "run_attempt", "head_sha", "head_branch", "event", "path", "name", "status", "conclusion")}
    for key in ("repository", "head_repository"):
        result[key] = {field: run[key][field] for field in ("id", "full_name")}
    result["jobs"] = []
    for effective_job in value["jobs"].values():
        executions = [effective_job]
        if "retained_source_job" in effective_job:
            executions.append(effective_job["retained_source_job"])
        for job in executions:
            record = {key: job[key] for key in ("id", "run_id", "run_attempt", "head_sha", "name", "status", "conclusion", "started_at", "completed_at")}
            for key in ("created_at", "runner_id", "runner_name", "labels"):
                if key in job:
                    record[key] = job[key]
            record["steps"] = job.get("steps", [])
            result["jobs"].append(record)
    return result


def expected_output_names(target, sha, version):
    stem = f"aTerminal-test-{target}-{sha[:12]}"
    archive = stem + TARGETS[target][1]
    result = {archive: "archive", archive + ".sha256": "checksum", stem + ".build-info.json": "build-info"}
    if target == "android":
        apk = f"aTerminal-{version}-android-debug-{sha[:12]}.apk"
        result.update({apk: "apk", apk + ".sha256": "checksum"})
    return result


def regular_output(path):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and 0 < info.st_size <= MAX_FILE,
            f"Unsafe output file: {path.name}")
    return info


def verify_output(output, repo, sha, version):
    """Offline verification of bytes and recorded evidence, not a fresh CI query."""
    require(output.is_dir() and not output.is_symlink(), "Output is not a regular directory")
    manifest_path = output / "client-assets.json"
    regular_output(manifest_path)
    manifest = read_json(manifest_path)
    require(manifest.get("schema_version") == 1 and manifest.get("purpose") == "prerelease-test-assets"
            and manifest.get("source_commit") == sha and manifest.get("repository") == repo
            and manifest.get("version") == version, "Client manifest identity/version mismatch")
    require(manifest.get("platforms") == list(TARGETS), "Client manifest must contain all five required platforms")
    require(manifest.get("warnings") == WARNINGS, "Client manifest signing/verification warnings are missing or altered")
    evidence = manifest.get("workflow_evidence", [])
    require(len(evidence) == len(WORKFLOWS), "Missing workflow evidence")
    ready = {}
    for run in evidence:
        path = run.get("path")
        require(path in WORKFLOWS and path not in ready and run_matches(run, repo, sha, path)
                and run.get("status") == "completed" and run.get("conclusion") == "success", "Invalid recorded workflow evidence")
        jobs = effective_jobs(run, run["jobs"], WORKFLOWS[path][1])
        ready[path] = {"run": run, "jobs": jobs}
    run = ready[PACKAGES_PATH]["run"]
    require(manifest.get("test_packages_run_id") == run["id"], "Manifest run ID mismatch")
    sources = manifest.get("sources", [])
    require(len(sources) == len(TARGETS) and {source.get("target") for source in sources} == set(TARGETS), "Missing/duplicate artifact sources")
    selected = select_artifacts(run, ready[PACKAGES_PATH]["jobs"], [source["artifact"] for source in sources],
                                now=timestamp(manifest["collected_at_utc"]))
    assets = manifest.get("assets", [])
    expected = {name: (target, kind) for target in TARGETS for name, kind in expected_output_names(target, sha, version).items()}
    require(isinstance(assets, list) and len(assets) == len(expected)
            and {asset.get("name") for asset in assets} == set(expected), "Client output inventory mismatch")
    hashes = {}
    with tempfile.TemporaryDirectory(prefix=".verify-client-assets-") as temporary:
        work = Path(temporary)
        for selection in selected:
            target, artifact, job = selection["target"], selection["artifact"], selection["job"]
            source = next(item for item in sources if item["target"] == target)
            require(source.get("source_commit") == sha and source.get("artifact_id") == artifact["id"]
                    and source.get("artifact_name") == artifact["name"] and source.get("github_artifact_digest") == artifact["digest"]
                    and source.get("github_artifact_digest_independently_verified") is False
                    and source.get("job_id") == job["id"] and source.get("job_attempt") == job["run_attempt"]
                    and source.get("effective_job_id") == selection["effective_job"]["id"]
                    and source.get("effective_job_attempt") == selection["effective_job"]["run_attempt"]
                    and source.get("retained_execution") is (job["id"] != selection["effective_job"]["id"])
                    and source.get("verified_job_steps") == selection["verified_steps"], "Recorded artifact/job provenance mismatch")
            download = work / (target + "-download")
            download.mkdir()
            for asset in [item for item in assets if item.get("target") == target]:
                name = asset["name"]
                require(expected.get(name) == (target, asset.get("kind")) and asset.get("source_commit") == sha
                        and asset.get("artifact_id") == artifact["id"], "Asset binding mismatch")
                info = regular_output(output / name)
                hashes[name] = sha256(output / name)
                require(type(asset.get("size")) is int and asset["size"] == info.st_size
                        and asset.get("sha256") == hashes[name], f"Output checksum/size mismatch: {name}")
                if name.startswith(f"aTerminal-test-{target}-"):
                    shutil.copyfile(output / name, download / name)
            result = validate_package(download, work / (target + "-contents"), selection, run, repo)
            require(source.get("inspection") == result["inspection"] and source.get("signing_notes") == result["signing_notes"], "Recorded package inspection mismatch")
            if target == "android":
                name = next(name for name, kind in expected_output_names(target, sha, version).items() if kind == "apk")
                require(sha256(output / name) == sha256(result["payload"]), "Bare APK differs from verified package")
                require(checksum_records((output / (name + ".sha256")).read_text(encoding="utf-8")) == {name: hashes[name]}, "Bare APK checksum sidecar mismatch")
    require(set(hashes) == set(expected), "Missing bound client asset")
    hashes["client-assets.json"] = sha256(manifest_path)
    regular_output(output / "CLIENT-SHA256SUMS")
    require(checksum_records((output / "CLIENT-SHA256SUMS").read_text(encoding="utf-8")) == hashes,
            "CLIENT-SHA256SUMS mismatch")
    return {"verified": True, "verification_mode": "offline-recorded-evidence", "source_commit": sha, "version": version,
            "test_packages_run_id": run["id"], "manifest": str(manifest_path), "platforms": list(TARGETS), "assets": assets}


def collect(client, repo, sha, output, ready, version):
    require(not output.is_symlink() and (not output.exists() or output.is_dir()), "Output is not a regular directory")
    expected_names = {name for target in TARGETS for name in expected_output_names(target, sha, version)} | {"client-assets.json", "CLIENT-SHA256SUMS"}
    require(not any((output / name).exists() or (output / name).is_symlink() for name in expected_names), "Output client filename clash")
    output.parent.mkdir(parents=True, exist_ok=True)
    packages = ready[PACKAGES_PATH]
    run = packages["run"]
    selected = select_artifacts(run, packages["jobs"], client.artifacts(run["id"]))
    # Validate the complete set in private staging before touching the shared dist
    # directory. The final manifest is the completion marker and is installed last.
    with tempfile.TemporaryDirectory(prefix=".collect-release-", dir=output.parent) as temporary:
        work = Path(temporary)
        assets_dir = work / "assets"
        assets_dir.mkdir()
        assets, sources = [], []
        for selection in selected:
            target = selection["target"]
            download = work / (target + "-download")
            download.mkdir()
            client.download(run["id"], selection["artifact"], download)
            result = validate_package(download, work / (target + "-contents"), selection, run, repo)
            artifact, job = selection["artifact"], selection["job"]
            source = {"target": target, "source_commit": sha, "artifact_id": artifact["id"],
                      "artifact_name": artifact["name"], "github_artifact_digest": artifact["digest"],
                      "github_artifact_digest_independently_verified": False, "artifact": artifact,
                      "job_id": job["id"], "job_attempt": job["run_attempt"],
                      "effective_job_id": selection["effective_job"]["id"],
                      "effective_job_attempt": selection["effective_job"]["run_attempt"],
                      "retained_execution": job["id"] != selection["effective_job"]["id"],
                      "verified_job_steps": selection["verified_steps"], "inspection": result["inspection"],
                      "signing_notes": result["signing_notes"]}
            sources.append(source)
            for name in result["external_names"]:
                shutil.copyfile(download / name, assets_dir / name)
            if target == "android":
                name = next(name for name, kind in expected_output_names(target, sha, version).items() if kind == "apk")
                destination = assets_dir / name
                shutil.copyfile(result["payload"], destination)
                (assets_dir / (name + ".sha256")).write_text(f"{sha256(destination)}  {name}\n", encoding="utf-8")
            for name, kind in expected_output_names(target, sha, version).items():
                destination = assets_dir / name
                assets.append({"name": name, "kind": kind, "target": target, "source_commit": sha,
                               "artifact_id": artifact["id"], "sha256": sha256(destination), "size": destination.stat().st_size})
        for path, evidence in ready.items():
            fresh = client.api(f"{client.base}/actions/runs/{evidence['run']['id']}")
            require(run_matches(fresh, repo, sha, path) and fresh.get("status") == "completed"
                    and fresh.get("conclusion") == "success"
                    and fresh.get("run_attempt") == evidence["run"]["run_attempt"], "Source run changed during collection")
        manifest = {"schema_version": 1, "purpose": "prerelease-test-assets", "repository": repo, "version": version,
                    "source_commit": sha, "collected_at_utc": datetime.now(timezone.utc).isoformat(),
                    "test_packages_run_id": run["id"], "workflow_evidence": [compact_run(ready[p]) for p in WORKFLOWS],
                    "platforms": list(TARGETS), "sources": sources, "assets": assets, "warnings": WARNINGS}
        manifest_name = "client-assets.json"
        write_json(assets_dir / manifest_name, manifest)
        checksums = "".join(f"{sha256(file)}  {file.name}\n" for file in sorted(assets_dir.iterdir()))
        (assets_dir / "CLIENT-SHA256SUMS").write_text(checksums, encoding="utf-8")
        verify_output(assets_dir, repo, sha, version)
        output.mkdir(exist_ok=True)
        require(not output.is_symlink() and not any((output / name).exists() or (output / name).is_symlink() for name in expected_names), "Output client filename clash")
        ordered = sorted(assets_dir.iterdir(), key=lambda path: (path.name == manifest_name, path.name))
        for source in ordered:
            # Atomic no-replace install on the same filesystem, including races.
            os.link(source, output / source.name)
            source.unlink()
    return {"source_commit": sha, "version": version, "test_packages_run_id": run["id"], "manifest": str(output / manifest_name),
            "output": str(output), "platforms": list(TARGETS), "assets": assets, "warnings": WARNINGS}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sha", default=os.environ.get("GITHUB_SHA"), help="exact checked-out full SHA; defaults to GITHUB_SHA")
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY"), help="owner/repo; defaults to GITHUB_REPOSITORY")
    parser.add_argument("--output", type=Path, default=Path("release-assets"), help="output directory; permits unrelated server files, refuses client clashes")
    parser.add_argument("--version", required=True, help="prerelease version, e.g. 0.1.0-alpha.2")
    parser.add_argument("--verify-only", action="store_true", help="offline revalidation; no gh/token/network required")
    parser.add_argument("--timeout-seconds", type=int, default=5400)
    parser.add_argument("--poll-seconds", type=int, default=30)
    args = parser.parse_args()
    try:
        require(isinstance(args.sha, str) and re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", args.sha), "--sha must be a full lowercase commit SHA")
        require(isinstance(args.repo, str) and re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo), "--repo must be owner/repo")
        require(0 < args.timeout_seconds <= 5400 and 1 <= args.poll_seconds <= 300, "Timeout must be 1..5400s and poll interval 1..300s")
        require(not os.environ.get("GITHUB_SHA") or args.sha == os.environ["GITHUB_SHA"], "--sha conflicts with GITHUB_SHA")
        require(not os.environ.get("GITHUB_REPOSITORY") or args.repo == os.environ["GITHUB_REPOSITORY"], "--repo conflicts with GITHUB_REPOSITORY")
        require(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+-[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*", args.version), "--version must be a prerelease SemVer without v prefix")
        if args.verify_only:
            print(json.dumps(verify_output(args.output.absolute(), args.repo, args.sha, args.version), sort_keys=True))
            return 0
        require(bool(os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")), "Set GH_TOKEN with actions:read")
        client = GitHub(args.repo, time.monotonic() + args.timeout_seconds)
        ready = wait_for_runs(client, args.repo, args.sha, args.poll_seconds)
        print(json.dumps(collect(client, args.repo, args.sha, args.output.absolute(), ready, args.version), sort_keys=True))
    except (OSError, ValueError, RuntimeError, KeyError, TypeError, zipfile.BadZipFile, tarfile.TarError, subprocess.TimeoutExpired) as error:
        parser.exit(1, f"Asset collection failed: {error}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
