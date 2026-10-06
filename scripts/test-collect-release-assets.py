#!/usr/bin/env python3
"""Network-free collector regressions. Fixtures are not runnable binaries/APKs."""
import argparse
import copy
from datetime import datetime, timezone
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import time
import unittest
from unittest.mock import patch
import warnings
import zipfile

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("collector", HERE / "collect-release-assets.py")
C = importlib.util.module_from_spec(spec)
spec.loader.exec_module(C)
P = C.packager_module()
SHA, REPO, VERSION = "a" * 40, "example/aTerminal", "0.1.0-alpha.2"


def elf(bits=2, machine=62):
    data = bytearray(64 if bits == 2 else 52)
    data[:7] = b"\x7fELF" + bytes((bits, 1, 1))
    struct.pack_into("<HHI", data, 16, 3, machine, 1)
    struct.pack_into("<H", data, 52 if bits == 2 else 40, len(data))
    return data


def make_binary(path, target):
    if "linux" in target:
        data = elf()
    elif "windows" in target:
        data = bytearray(512)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 64)
        data[64:68] = b"PE\0\0"
        struct.pack_into("<HH", data, 68, 0x8664, 1)
        struct.pack_into("<HHH", data, 84, 240, 2, 0x20B)
    else:
        data = bytearray(32)
        data[:4] = b"\xcf\xfa\xed\xfe"
        struct.pack_into("<III", data, 4, 0x0100000C if "aarch64" in target else 0x01000007, 0, 2)
    path.write_bytes(data)
    path.chmod(0o755)


def make_apk(path):
    with zipfile.ZipFile(path, "w") as apk:
        apk.writestr("AndroidManifest.xml", b"synthetic manifest")
        apk.writestr("classes.dex", b"synthetic dex")
        for abi, (bits, machine) in P.ABIS.items():
            for library in P.NATIVE_LIBS:
                apk.writestr(f"lib/{abi}/{library}", elf(bits, machine))


def make_run(path, run_id):
    return {"id": run_id, "head_sha": SHA, "head_branch": "main", "event": "push", "path": path,
            "name": C.WORKFLOWS[path][0], "repository": {"id": 7, "full_name": REPO},
            "head_repository": {"id": 7, "full_name": REPO}, "status": "completed", "conclusion": "success", "run_attempt": 1}


def make_job(run, name, job_id, attempt=1):
    names = {C.UPLOAD_STEP, C.SIGNATURE_STEP, "Build APK and lint", "Package and verify native libraries",
             "Run workspace tests", "Build optimized test binary", "Check executable starts",
             "Package and verify contents", "Exercise host terminal"}
    return {"id": job_id, "name": name, "run_id": run["id"], "head_sha": SHA, "run_attempt": attempt,
            "status": "completed", "conclusion": "success", "created_at": "2026-10-05T23:59:58Z",
            "runner_id": job_id, "runner_name": "synthetic-runner", "labels": ["synthetic"],
            "started_at": "2026-10-06T00:00:00Z",
            "completed_at": "2026-10-06T01:00:00Z",
            "steps": [{"name": name, "status": "completed", "conclusion": "success", "number": number,
                       "started_at": "2026-10-06T00:00:10Z", "completed_at": "2026-10-06T00:00:20Z"}
                      for number, name in enumerate(sorted(names), 1)]}


def make_artifact(run, target, artifact_id, attempt=1):
    name_target = "android-debug" if target == "android" else target
    return {"id": artifact_id, "name": f"aTerminal-test-{name_target}-{SHA}-attempt-{attempt}",
            "expired": False, "expires_at": "2030-01-01T00:00:00Z", "created_at": "2026-10-06T00:30:00Z",
            "size_in_bytes": 1000, "digest": "sha256:" + "b" * 64,
            "workflow_run": {"id": run["id"], "head_sha": SHA, "head_branch": "main", "repository_id": 7, "head_repository_id": 7}}


class FakeClient:
    def __init__(self, runs, jobs, artifacts, directories):
        self.run_values, self.job_values, self.artifact_values, self.directories = runs, jobs, artifacts, directories
        self.base, self.deadline = f"repos/{REPO}", time.monotonic() + 10
        self.downloads = []

    def runs(self, sha):
        return self.run_values

    def jobs(self, run_id):
        return self.job_values[run_id]

    def artifacts(self, run_id):
        return self.artifact_values

    def download(self, run_id, artifact, output):
        self.downloads.append(artifact["id"])
        for path in self.directories[artifact["id"]].iterdir():
            shutil.copyfile(path, output / path.name)

    def api(self, endpoint):
        return next(run for run in self.run_values if endpoint.endswith("/" + str(run["id"])))


class CollectorTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "source"
        for name in ("README.md", "THIRD_PARTY.md", "deploy/TEST-PACKAGES.md", "crates/desktop-agent/assets/fonts/LICENSE-Noto-CJK.txt"):
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("Synthetic test fixture\n", encoding="utf-8")
        self.verify = make_run(C.VERIFY_PATH, 100)
        self.run = make_run(C.PACKAGES_PATH, 101)
        self.jobs = {target: make_job(self.run, C.ANDROID_JOB if target == "android" else "Desktop " + target, 1000 + number)
                     for number, target in enumerate(C.TARGETS)}
        self.artifacts = {target: make_artifact(self.run, target, 2000 + number) for number, target in enumerate(C.TARGETS)}
        self.directories = {}
        for target in C.TARGETS:
            self.make_package(target)
        verify_jobs = [make_job(self.verify, name, 3000 + number) for number, name in enumerate(sorted(C.VERIFY_JOBS))]
        self.client = FakeClient([self.verify, self.run], {100: verify_jobs, 101: list(self.jobs.values())},
                                 list(self.artifacts.values()), self.directories)
        self.output = self.root / "dist"

    def make_package(self, target, attempt=1):
        payload = self.source / "input"
        if target == "android":
            make_apk(payload)
        else:
            make_binary(payload, target)
        directory = self.root / f"package-{target}-{attempt}"
        ci = {"GITHUB_REPOSITORY": REPO, "GITHUB_SHA": SHA, "GITHUB_RUN_ID": "101", "GITHUB_RUN_ATTEMPT": str(attempt),
              "GITHUB_WORKFLOW": "Test Packages", "RUNNER_OS": C.TARGETS[target][2], "RUNNER_ARCH": C.TARGETS[target][3],
              "SOURCE_DATE_EPOCH": "1791246600"}
        args = argparse.Namespace(kind="android" if target == "android" else "desktop", target=target,
                                  binary=payload, apk=payload, sha=SHA, output_dir=directory)
        with patch.dict(os.environ, ci, clear=True):
            P.package(args, self.source)
        self.directories[self.artifacts[target]["id"]] = directory
        return directory

    def ready(self):
        ready, pending = C.inspect_runs(self.client, REPO, SHA)
        self.assertEqual(pending, [])
        return ready

    def selection(self, target):
        return next(item for item in C.select_artifacts(self.run, {job["name"]: job for job in self.jobs.values()}, list(self.artifacts.values())) if item["target"] == target)

    def validate(self, target):
        artifact = self.artifacts[target]
        return C.validate_package(self.directories[artifact["id"]], self.root / "unpack", self.selection(target), self.run, REPO)

    def update_external(self, target):
        directory = self.directories[self.artifacts[target]["id"]]
        archive = next(path for path in directory.iterdir() if path.name.endswith((".zip", ".tar.gz")))
        summary_path = next(directory.glob("*.build-info.json"))
        summary = json.loads(summary_path.read_text())
        summary.update(archive_sha256=C.sha256(archive), archive_size=archive.stat().st_size)
        C.write_json(summary_path, summary)
        (directory / (archive.name + ".sha256")).write_text(f"{C.sha256(archive)}  {archive.name}\n")
        return archive

    def test_matching_runs_and_all_five_packages(self):
        self.assertEqual(set(self.ready()), set(C.WORKFLOWS))
        for target in C.TARGETS:
            with self.subTest(target=target):
                result = self.validate(target)
                self.assertTrue(result["payload"].is_file())
                shutil.rmtree(self.root / "unpack")

    def test_collect_into_server_dist_and_verify_offline(self):
        self.output.mkdir()
        (self.output / "server.tar").write_bytes(b"unrelated server payload")
        result = C.collect(self.client, REPO, SHA, self.output, self.ready(), VERSION)
        self.assertEqual(len(result["assets"]), 17)
        self.assertEqual(len(self.client.downloads), 5)
        self.assertEqual((self.output / "server.tar").read_bytes(), b"unrelated server payload")
        self.assertTrue(C.verify_output(self.output, REPO, SHA, VERSION)["verified"])
        self.assertTrue((self.output / f"aTerminal-{VERSION}-android-debug-{SHA[:12]}.apk").is_file())
        self.assertFalse((self.output / "SHA256SUMS").exists())
        with patch.dict(os.environ, {}, clear=True), patch.object(C.GitHub, "api", side_effect=AssertionError("network forbidden")):
            with patch.object(sys, "argv", ["collector", "--verify-only", "--repo", REPO, "--sha", SHA, "--version", VERSION, "--output", str(self.output)]), patch("builtins.print"):
                self.assertEqual(C.main(), 0)

    def test_missing_or_wrong_sha_run_waits(self):
        self.client.run_values = [self.verify]
        ready, pending = C.inspect_runs(self.client, REPO, SHA)
        self.assertIsNone(ready)
        self.assertEqual(len(pending), 1)
        self.run["head_sha"] = "c" * 40
        self.client.run_values.append(self.run)
        self.assertIsNone(C.inspect_runs(self.client, REPO, SHA)[0])

    def test_foreign_repo_branch_event_and_path_not_eligible(self):
        for field, value in (("head_branch", "other"), ("event", "pull_request"), ("path", "other.yml"),
                             ("head_repository", {"id": 8, "full_name": "attacker/aTerminal"})):
            run = copy.deepcopy(self.run)
            run[field] = value
            self.assertFalse(C.run_matches(run, REPO, SHA, C.PACKAGES_PATH))

    def test_latest_matching_failed_run_is_not_hidden_by_old_success(self):
        older = copy.deepcopy(self.run)
        older["id"] = 90
        self.run["conclusion"] = "failure"
        self.client.run_values.extend([older])
        with self.assertRaisesRegex(ValueError, "definitively"):
            self.ready()

    def test_failure_is_reported_even_if_other_workflow_missing(self):
        self.client.run_values = [self.run]
        self.run["conclusion"] = "cancelled"
        with self.assertRaisesRegex(ValueError, "definitively"):
            C.inspect_runs(self.client, REPO, SHA)

    def test_failed_required_job_in_running_run_fails_immediately(self):
        self.run.update(status="in_progress", conclusion=None)
        self.jobs["android"]["conclusion"] = "failure"
        with self.assertRaisesRegex(ValueError, "Required job failed"):
            self.ready()

    def test_missing_success_job_rejected(self):
        self.client.job_values[101] = list(self.jobs.values())[:-1]
        with self.assertRaisesRegex(ValueError, "Missing required jobs"):
            self.ready()

    def test_missing_expired_wrong_sha_and_duplicate_artifact_rejected(self):
        for mutation in ("missing", "expired", "wrong-sha", "duplicate", "foreign-repo", "outside-job"):
            artifacts = copy.deepcopy(list(self.artifacts.values()))
            if mutation == "missing":
                artifacts.pop()
            elif mutation == "expired":
                artifacts[0]["expired"] = True
            elif mutation == "wrong-sha":
                artifacts[0]["workflow_run"]["head_sha"] = "b" * 40
            elif mutation == "duplicate":
                artifacts.append(copy.deepcopy(artifacts[0]))
            elif mutation == "foreign-repo":
                artifacts[0]["workflow_run"]["head_repository_id"] = 99
            else:
                artifacts[0]["created_at"] = "2026-10-06T02:00:00Z"
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                C.select_artifacts(self.run, {job["name"]: job for job in self.jobs.values()}, artifacts)

    def test_rerun_preserves_earlier_successful_artifacts(self):
        target = "x86_64-pc-windows-msvc"
        self.run["run_attempt"] = 2
        old_job = copy.deepcopy(self.jobs[target])
        old_job["conclusion"] = "failure"
        self.jobs[target] = make_job(self.run, "Desktop " + target, 5000, attempt=2)
        old_artifact = copy.deepcopy(self.artifacts[target])
        self.artifacts[target] = make_artifact(self.run, target, 5001, attempt=2)
        self.make_package(target, attempt=2)
        self.client.job_values[101] = [old_job] + list(self.jobs.values())
        self.client.artifact_values = [old_artifact] + list(self.artifacts.values())
        result = C.collect(self.client, REPO, SHA, self.output, self.ready(), VERSION)
        self.assertEqual(set(self.client.downloads), {artifact["id"] for artifact in self.artifacts.values()})
        manifest = C.read_json(self.output / "client-assets.json")
        self.assertEqual(sorted(source["job_attempt"] for source in manifest["sources"]), [1, 1, 1, 1, 2])
        self.assertEqual(result["test_packages_run_id"], 101)

    def real_retry_fixture(self, simulate_success=False):
        data = C.read_json(HERE / "fixtures/test-packages-retained-jobs.json")
        run = make_run(C.PACKAGES_PATH, data["jobs"][0]["run_id"])
        run.update(head_sha=data["jobs"][0]["head_sha"], run_attempt=2,
                   repository={"id": 1405019962, "full_name": "dqh147258/aTerminal"},
                   head_repository={"id": 1405019962, "full_name": "dqh147258/aTerminal"},
                   conclusion="success" if simulate_success else "cancelled")
        if simulate_success:
            # Only this hypothetical Intel final outcome changes; authentic
            # retained-clone records and original timestamps/steps stay untouched.
            job = next(job for job in data["jobs"] if job["name"] == "Desktop x86_64-apple-darwin" and job["run_attempt"] == 2)
            job["conclusion"] = "success"
        return run, data

    def test_real_cancelled_retry_is_never_accepted(self):
        run, data = self.real_retry_fixture()
        with self.assertRaisesRegex(ValueError, "Required job failed"):
            C.effective_jobs(run, data["jobs"], C.PACKAGE_JOBS)
        client = FakeClient([run], {run["id"]: data["jobs"]}, data["artifacts"], {})
        with self.assertRaisesRegex(ValueError, "definitively ended: cancelled"):
            C.inspect_runs(client, "dqh147258/aTerminal", run["head_sha"])

    def test_real_retained_clones_resolve_original_execution_roundtrip(self):
        run, data = self.real_retry_fixture(simulate_success=True)
        jobs = C.effective_jobs(run, data["jobs"], C.PACKAGE_JOBS)
        selections = C.select_artifacts(run, jobs, data["artifacts"], now=datetime(2026, 10, 6, 5, tzinfo=timezone.utc))
        self.assertEqual(sorted(item["job"]["run_attempt"] for item in selections), [1, 1, 1, 1, 2])
        self.assertTrue(all(item["effective_job"]["run_attempt"] == 2 for item in selections))
        for selection in selections:
            if selection["target"] != "x86_64-apple-darwin":
                self.assertNotEqual(selection["job"]["id"], selection["effective_job"]["id"])
        compact = C.compact_run({"run": run, "jobs": jobs})
        self.assertEqual(len(compact["jobs"]), 9)  # Five effective records + four original executions.
        replay = C.effective_jobs(compact, compact["jobs"], C.PACKAGE_JOBS)
        replay_selections = C.select_artifacts(compact, replay, data["artifacts"], now=datetime(2026, 10, 6, 5, tzinfo=timezone.utc))
        self.assertEqual([item["job"]["id"] for item in replay_selections], [item["job"]["id"] for item in selections])

    def test_real_retained_clones_across_third_attempt(self):
        run, data = self.real_retry_fixture(simulate_success=True)
        run["run_attempt"] = 3
        for job in list(data["jobs"]):
            if job["run_attempt"] == 2:
                clone = copy.deepcopy(job)
                clone.update(id=job["id"] + 100000, run_attempt=3, created_at="2026-10-06T05:00:00Z")
                data["jobs"].append(clone)
        jobs = C.effective_jobs(run, data["jobs"], C.PACKAGE_JOBS)
        selections = C.select_artifacts(run, jobs, data["artifacts"], now=datetime(2026, 10, 6, 5, 1, tzinfo=timezone.utc))
        self.assertEqual(sorted(item["job"]["run_attempt"] for item in selections), [1, 1, 1, 1, 2])
        self.assertTrue(all(item["effective_job"]["run_attempt"] == 3 for item in selections))

    def test_real_clone_requires_exact_successful_original_steps_and_times(self):
        for mutation in ("missing-original", "failed-original", "changed-start", "changed-end", "changed-step-time", "missing-step-number"):
            run, data = self.real_retry_fixture(simulate_success=True)
            original = next(job for job in data["jobs"] if job["name"] == C.ANDROID_JOB and job["run_attempt"] == 1)
            if mutation == "missing-original":
                data["jobs"].remove(original)
            elif mutation == "failed-original":
                original["conclusion"] = "failure"
            elif mutation == "changed-start":
                original["started_at"] = "2026-10-06T03:52:54Z"
            elif mutation == "changed-end":
                original["completed_at"] = "2026-10-06T04:07:58Z"
            elif mutation == "changed-step-time":
                original["steps"][0]["completed_at"] = "2026-10-06T03:53:00Z"
            else:
                clone = next(job for job in data["jobs"] if job["name"] == C.ANDROID_JOB and job["run_attempt"] == 2)
                clone["steps"][0].pop("number")
            with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, "Retained-job clone"):
                C.effective_jobs(run, data["jobs"], C.PACKAGE_JOBS)

    def test_real_genuine_new_execution_cannot_use_old_artifact(self):
        run, data = self.real_retry_fixture(simulate_success=True)
        clone = next(job for job in data["jobs"] if job["name"] == C.ANDROID_JOB and job["run_attempt"] == 2)
        # A genuine new execution has new times, not the retained completion.
        clone.update(started_at="2026-10-06T04:10:41Z", completed_at="2026-10-06T04:20:00Z")
        jobs = C.effective_jobs(run, data["jobs"], C.PACKAGE_JOBS)
        self.assertNotIn("retained_source_job", jobs[C.ANDROID_JOB])
        with self.assertRaisesRegex(ValueError, "Expected one exact artifact.*attempt-2"):
            C.select_artifacts(run, jobs, data["artifacts"], now=datetime(2026, 10, 6, 5, tzinfo=timezone.utc))

    def test_retained_clones_full_collection_and_offline_provenance(self):
        self.run["run_attempt"] = 2
        original_jobs = list(self.jobs.values())
        clones = []
        for original in original_jobs:
            clone = copy.deepcopy(original)
            clone.update(id=original["id"] + 10000, run_attempt=2, created_at="2026-10-06T01:01:00Z")
            clones.append(clone)
        self.client.job_values[101] = original_jobs + clones
        C.collect(self.client, REPO, SHA, self.output, self.ready(), VERSION)
        manifest = C.read_json(self.output / "client-assets.json")
        self.assertTrue(all(source["retained_execution"] for source in manifest["sources"]))
        self.assertTrue(all(source["job_attempt"] == 1 and source["effective_job_attempt"] == 2 for source in manifest["sources"]))
        self.assertTrue(C.verify_output(self.output, REPO, SHA, VERSION)["verified"])
        # Removing original execution evidence cannot pass offline preflight.
        package_evidence = next(run for run in manifest["workflow_evidence"] if run["path"] == C.PACKAGES_PATH)
        package_evidence["jobs"] = [job for job in package_evidence["jobs"] if job["run_attempt"] == 2]
        C.write_json(self.output / "client-assets.json", manifest)
        with self.assertRaisesRegex(ValueError, "Retained-job clone"):
            C.verify_output(self.output, REPO, SHA, VERSION)

    def test_offline_injected_source_execution_cannot_bypass_resolver(self):
        self.run["run_attempt"] = 2
        originals = list(self.jobs.values())
        clones = []
        for original in originals:
            clone = copy.deepcopy(original)
            clone.update(id=original["id"] + 10000, run_attempt=2, created_at="2026-10-06T01:01:00Z")
            clones.append(clone)
        self.client.job_values[101] = originals + clones
        C.collect(self.client, REPO, SHA, self.output, self.ready(), VERSION)
        path = self.output / "client-assets.json"
        manifest = C.read_json(path)
        evidence = next(run for run in manifest["workflow_evidence"] if run["path"] == C.PACKAGES_PATH)
        originals_by_name = {job["name"]: job for job in evidence["jobs"] if job["run_attempt"] == 1}
        evidence["jobs"] = [job for job in evidence["jobs"] if job["run_attempt"] == 2]
        for job in evidence["jobs"]:
            job["retained_source_job"] = originals_by_name[job["name"]]
            job["created_at"] = job["retained_source_job"]["created_at"]
        C.write_json(path, manifest)
        checksums = C.checksum_records((self.output / "CLIENT-SHA256SUMS").read_text())
        checksums["client-assets.json"] = C.sha256(path)
        (self.output / "CLIENT-SHA256SUMS").write_text("".join(f"{digest}  {name}\n" for name, digest in sorted(checksums.items())))
        with self.assertRaisesRegex(ValueError, "reserved retained_source_job"):
            C.verify_output(self.output, REPO, SHA, VERSION)

    def test_rerun_does_not_fall_back_to_old_artifact_for_rerun_job(self):
        self.run["run_attempt"] = 2
        self.jobs["android"]["run_attempt"] = 2
        with self.assertRaisesRegex(ValueError, "Expected one exact artifact"):
            self.selection("android")

    def test_android_signature_step_must_succeed(self):
        step = next(step for step in self.jobs["android"]["steps"] if step["name"] == C.SIGNATURE_STEP)
        step["conclusion"] = "skipped"
        with self.assertRaisesRegex(ValueError, "Missing successful step"):
            self.selection("android")

    def test_api_auth_failure_is_fatal_not_absence(self):
        with patch.object(self.client, "runs", side_effect=RuntimeError("HTTP 403 forbidden")):
            with self.assertRaisesRegex(RuntimeError, "403"):
                C.wait_for_runs(self.client, REPO, SHA, 30)

    def test_gh_command_failure_is_fatal_and_token_redacted(self):
        result = subprocess.CompletedProcess([], 1, "", "HTTP 401 token=secret-token")
        with patch("subprocess.run", return_value=result), patch.dict(os.environ, {"GH_TOKEN": "secret-token"}):
            with self.assertRaisesRegex(RuntimeError, r"401 token=\[redacted\]"):
                C.GitHub(REPO, time.monotonic() + 10).api("repos/example/aTerminal/actions/runs")

    def test_gh_download_uses_official_run_download(self):
        client = C.GitHub(REPO, time.monotonic() + 10)
        artifact = self.artifacts["android"]
        with patch.object(client, "command", return_value="") as command, patch.object(client, "api", return_value=artifact):
            client.download(101, artifact, self.root / "download")
        self.assertEqual(command.call_args.args[0][:3], ["run", "download", "101"])
        self.assertIn("--name", command.call_args.args[0])
        self.assertNotIn("--header", command.call_args.args[0])

    def test_checksum_corruption_rejected(self):
        target = "android"
        directory = self.directories[self.artifacts[target]["id"]]
        archive = next(directory.glob("*.zip"))
        with archive.open("ab") as stream:
            stream.write(b"corruption")
        with self.assertRaisesRegex(ValueError, "External archive SHA"):
            self.validate(target)

    def test_inner_manifest_mismatch_rejected(self):
        target = "android"
        directory = self.directories[self.artifacts[target]["id"]]
        summary_path = next(directory.glob("*.build-info.json"))
        summary = C.read_json(summary_path)
        summary["build_info"]["source_commit"] = "b" * 40
        C.write_json(summary_path, summary)
        with self.assertRaisesRegex(ValueError, "Inner/outer"):
            self.validate(target)

    def test_unsafe_tar_paths_and_types_rejected(self):
        target = "x86_64-unknown-linux-gnu"
        directory = self.directories[self.artifacts[target]["id"]]
        archive = next(directory.glob("*.tar.gz"))
        original = archive.read_bytes()
        for name, kind in (("../escape", tarfile.REGTYPE), ("/absolute", tarfile.REGTYPE),
                           (f"aTerminal-test-{target}-{SHA[:12]}/aTerminal", tarfile.SYMTYPE),
                           (f"aTerminal-test-{target}-{SHA[:12]}/aTerminal", tarfile.LNKTYPE)):
            with tarfile.open(archive, "w:gz") as package:
                item = tarfile.TarInfo(name)
                item.type, item.size, item.mode, item.linkname = kind, 1, 0o755, "../../outside"
                package.addfile(item, io.BytesIO(b"x"))
            self.update_external(target)
            with self.subTest(name=name, kind=kind), self.assertRaises(ValueError):
                self.validate(target)
            if (self.root / "unpack").exists():
                shutil.rmtree(self.root / "unpack")
            archive.write_bytes(original)
        self.assertFalse((self.root / "escape").exists())

    def test_duplicate_zip_entry_rejected(self):
        target = "android"
        directory = self.directories[self.artifacts[target]["id"]]
        archive = next(directory.glob("*.zip"))
        with zipfile.ZipFile(archive, "a") as package, warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            package.writestr(package.namelist()[0], b"duplicate")
        self.update_external(target)
        with self.assertRaisesRegex(ValueError, "inventory"):
            self.validate(target)

    def test_apk_symlink_path_and_duplicate_rejected(self):
        path = self.root / "unsafe.apk"
        for name, mode in (("../escape", stat.S_IFREG | 0o644), ("symlink", stat.S_IFLNK | 0o777),
                           ("C:/windows", stat.S_IFREG | 0o644), ("a//b", stat.S_IFREG | 0o644)):
            with zipfile.ZipFile(path, "w") as apk:
                item = zipfile.ZipInfo(name)
                item.create_system, item.external_attr = 3, mode << 16
                apk.writestr(item, b"target")
            with self.subTest(name=name), self.assertRaises(ValueError):
                C.check_apk_paths(path)

    def test_raw_nul_zip_names_rejected_before_python_normalization(self):
        for apk in (True, False):
            path = self.root / ("nul.apk" if apk else "nul.zip")
            name = "safeXevil" if apk else "stem/safeXevil"
            with zipfile.ZipFile(path, "w") as archive:
                item = zipfile.ZipInfo(name)
                item.create_system, item.external_attr = 3, (stat.S_IFREG | 0o644) << 16
                archive.writestr(item, b"x")
            # Alter local and central-directory raw names without changing length.
            path.write_bytes(path.read_bytes().replace(name.encode(), name.replace("X", "\x00").encode()))
            with self.subTest(apk=apk), self.assertRaisesRegex(ValueError, "truncated or normalized"):
                if apk:
                    C.check_apk_paths(path)
                else:
                    C.extract_package(path, self.root / "nul-unpacked", "stem", {"safe"})

    def test_download_symlink_rejected(self):
        directory = self.directories[self.artifacts["android"]["id"]]
        summary_path = next(directory.glob("*.build-info.json"))
        other = self.root / "summary"
        summary_path.rename(other)
        summary_path.symlink_to(other)
        with self.assertRaisesRegex(ValueError, "Unsafe downloaded"):
            self.validate("android")

    def test_output_checksum_mutation_rejected(self):
        C.collect(self.client, REPO, SHA, self.output, self.ready(), VERSION)
        apk = next(self.output.glob("*.apk"))
        with apk.open("ab") as stream:
            stream.write(b"tamper")
        with self.assertRaisesRegex(ValueError, "Output checksum"):
            C.verify_output(self.output, REPO, SHA, VERSION)

    def test_verify_version_and_missing_platform_rejected(self):
        C.collect(self.client, REPO, SHA, self.output, self.ready(), VERSION)
        with self.assertRaisesRegex(ValueError, "identity/version"):
            C.verify_output(self.output, REPO, SHA, "0.1.0-alpha.3")
        path = self.output / "client-assets.json"
        manifest = C.read_json(path)
        manifest["platforms"].pop()
        C.write_json(path, manifest)
        with self.assertRaisesRegex(ValueError, "five required"):
            C.verify_output(self.output, REPO, SHA, VERSION)

    def test_output_clash_prevents_download_and_never_overwrites(self):
        self.output.mkdir()
        (self.output / "client-assets.json").write_text("existing")
        with self.assertRaisesRegex(ValueError, "clash"):
            C.collect(self.client, REPO, SHA, self.output, self.ready(), VERSION)
        self.assertEqual(self.client.downloads, [])
        self.assertEqual((self.output / "client-assets.json").read_text(), "existing")

    def test_source_run_rerun_during_download_prevents_install(self):
        fresh = copy.deepcopy(self.run)
        fresh["run_attempt"] = 2
        with patch.object(self.client, "api", side_effect=lambda endpoint: self.verify if endpoint.endswith("/100") else fresh):
            with self.assertRaisesRegex(ValueError, "changed during"):
                C.collect(self.client, REPO, SHA, self.output, self.ready(), VERSION)
        self.assertFalse(self.output.exists())

    def test_duplicate_json_and_checksums_rejected(self):
        with self.assertRaisesRegex(ValueError, "Duplicate JSON"):
            C.strict_json('{"x":1,"x":2}')
        with self.assertRaisesRegex(ValueError, "Duplicate checksum"):
            C.checksum_records(("a" * 64 + "  path\n") * 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
