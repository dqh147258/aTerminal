#!/usr/bin/env python3
"""Validate and package prebuilt test artifacts; Python 3.10+, standard library only.

Examples (run from a checkout):
  python scripts/package-test-artifact.py desktop --target x86_64-unknown-linux-gnu \
      --binary target/x86_64-unknown-linux-gnu/release/aTerminal --sha FULL_COMMIT_SHA
  python scripts/package-test-artifact.py android \
      --apk apps/android/app/build/outputs/apk/debug/app-debug.apk --sha FULL_COMMIT_SHA
  python scripts/package-test-artifact.py --self-test

This does not compile, sign, execute, install, upload, or publish anything. Header
checks and hashes are structural evidence, not a runtime test or a signed attestation.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import struct
import sys
import tarfile
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "x86_64-pc-windows-msvc": ("PE", "x86_64", "aTerminal.exe", ".zip"),
    "x86_64-unknown-linux-gnu": ("ELF", "x86_64", "aTerminal", ".tar.gz"),
    "aarch64-apple-darwin": ("Mach-O", "arm64", "aTerminal", ".tar.gz"),
    "x86_64-apple-darwin": ("Mach-O", "x86_64", "aTerminal", ".tar.gz"),
}
ABIS = {"arm64-v8a": (2, 183), "x86_64": (2, 62), "x86": (1, 3)}
NATIVE_LIBS = ("libai_terminal_mobile.so", "libc++_shared.so")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def regular_file(path: Path) -> Path:
    require(not path.is_symlink() and path.is_file(), f"Not a regular non-symlink file: {path}")
    require(path.stat().st_size > 0, f"Empty file: {path}")
    return path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def elf_header(header: bytes, elf_class: int, machine: int, shared: bool) -> dict:
    minimum = 64 if elf_class == 2 else 52
    require(len(header) >= minimum and header[:4] == b"\x7fELF", "Missing or truncated ELF header")
    require(header[4:7] == bytes((elf_class, 1, 1)), "Wrong ELF class, endianness, or version")
    kind, actual_machine, version = struct.unpack_from("<HHI", header, 16)
    require(actual_machine == machine and version == 1, "Wrong ELF machine or version")
    require(kind == 3 if shared else kind in (2, 3), "Unexpected ELF file type")
    require(struct.unpack_from("<H", header, 52 if elf_class == 2 else 40)[0] == minimum,
            "Invalid ELF header size")
    return {"format": "ELF", "bits": 64 if elf_class == 2 else 32,
            "machine": machine, "elf_type": kind, "endianness": "little"}


def desktop_header(path: Path, target: str) -> dict:
    expected_format, architecture, _, _ = TARGETS[target]
    with path.open("rb") as stream:
        header = stream.read(64)
        if expected_format == "ELF":
            result = elf_header(header, 2, 62, shared=False)
        elif expected_format == "PE":
            require(len(header) == 64 and header[:2] == b"MZ", "Missing DOS/PE header")
            offset = struct.unpack_from("<I", header, 60)[0]
            require(64 <= offset <= path.stat().st_size - 26, "Invalid PE header offset")
            stream.seek(offset)
            pe = stream.read(26)
            require(pe[:4] == b"PE\0\0", "Missing PE signature")
            machine, sections = struct.unpack_from("<HH", pe, 4)
            optional_size, flags, magic = struct.unpack_from("<HHH", pe, 20)
            require(machine == 0x8664 and magic == 0x20B, "Expected x86_64 PE32+ executable")
            require(sections > 0 and optional_size >= 112, "Truncated PE optional header")
            require(offset + 24 + optional_size <= path.stat().st_size, "Truncated PE optional header")
            require(bool(flags & 0x2) and not flags & 0x2000, "Expected executable PE, not DLL")
            result = {"format": "PE", "bits": 64, "machine": machine, "pe_magic": magic}
        else:
            require(len(header) >= 32 and header[:4] == b"\xcf\xfa\xed\xfe",
                    "Expected a thin little-endian 64-bit Mach-O executable")
            cpu, _, file_type = struct.unpack_from("<III", header, 4)
            expected_cpu = 0x0100000C if architecture == "arm64" else 0x01000007
            require(cpu == expected_cpu and file_type == 2, "Wrong Mach-O architecture or file type")
            result = {"format": "Mach-O", "bits": 64, "cpu_type": cpu, "file_type": file_type}
    result["architecture"] = architecture
    return result


def android_structure(path: Path) -> dict:
    libraries = []
    with zipfile.ZipFile(path) as apk:
        names = apk.namelist()
        require(len(names) == len(set(names)), "APK contains duplicate ZIP entries")
        for name in names:
            parts = PurePosixPath(name).parts
            require(not name.startswith("/") and "\\" not in name and ".." not in parts,
                    f"Unsafe APK entry: {name}")
        require("AndroidManifest.xml" in names and "classes.dex" in names,
                "APK must contain AndroidManifest.xml and classes.dex")
        require(apk.getinfo("AndroidManifest.xml").file_size > 0 and apk.getinfo("classes.dex").file_size > 0,
                "Empty AndroidManifest.xml or classes.dex")
        present_abis = {name.split("/")[1] for name in names
                        if name.startswith("lib/") and name.endswith(".so") and len(name.split("/")) == 3}
        require(present_abis == set(ABIS), f"Expected exactly {sorted(ABIS)}; found {sorted(present_abis)}")
        require(apk.testzip() is None, "APK ZIP CRC check failed")
        for abi, (elf_class, machine) in ABIS.items():
            for library in NATIVE_LIBS:
                name = f"lib/{abi}/{library}"
                require(name in names, f"APK is missing {name}")
                with apk.open(name) as stream:
                    header = stream.read(64)
                record = elf_header(header, elf_class, machine, shared=True)
                digest = hashlib.sha256()
                with apk.open(name) as stream:
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                        digest.update(chunk)
                record.update(path=name, sha256=digest.hexdigest(), size=apk.getinfo(name).file_size)
                libraries.append(record)
    return {"format": "APK/ZIP", "abis": sorted(ABIS), "native_libraries": libraries,
            "zip_crc_verified": True,
            "not_verified_here": ["manifest values", "DEX build configuration", "APK signature", "device runtime"]}


def json_bytes(value: dict) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def write_archive(path: Path, stage: Path, name: str, modes: dict[str, int], epoch: int) -> None:
    if path.name.endswith(".tar.gz"):
        with path.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=epoch) as gz:
            with tarfile.open(fileobj=gz, mode="w", format=tarfile.PAX_FORMAT) as archive:
                for relative in sorted(modes):
                    source = stage / relative
                    info = tarfile.TarInfo(f"{name}/{relative}")
                    info.size, info.mode, info.mtime = source.stat().st_size, modes[relative], epoch
                    with source.open("rb") as stream:
                        archive.addfile(info, stream)
    else:
        # ZIP timestamps start in 1980; archive metadata retains the requested epoch.
        date = datetime.fromtimestamp(max(315532800, epoch), timezone.utc).timetuple()[:6]
        with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for relative in sorted(modes):
                info = zipfile.ZipInfo(f"{name}/{relative}", date_time=date)
                info.create_system = 3
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (stat.S_IFREG | modes[relative]) << 16
                with (stage / relative).open("rb") as src, archive.open(info, "w", force_zip64=True) as dst:
                    shutil.copyfileobj(src, dst)


def verify_archive(path: Path, stage: Path, name: str, modes: dict[str, int]) -> None:
    expected = {f"{name}/{relative}" for relative in modes}
    if path.name.endswith(".tar.gz"):
        with tarfile.open(path, "r:gz") as archive:
            members = archive.getmembers()
            require(len(members) == len(expected) and {m.name for m in members} == expected,
                    "Archive inventory mismatch")
            for member in members:
                relative = member.name[len(name) + 1:]
                require(member.isfile() and member.mode == modes[relative], "Archive mode/type mismatch")
                with archive.extractfile(member) as stream:
                    digest = hashlib.sha256()
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                        digest.update(chunk)
                require(digest.hexdigest() == sha256(stage / relative), "Archive content mismatch")
    else:
        with zipfile.ZipFile(path) as archive:
            require(len(archive.namelist()) == len(expected) and set(archive.namelist()) == expected,
                    "Archive inventory mismatch")
            require(archive.testzip() is None, "Archive CRC mismatch")
            for member in archive.infolist():
                relative = member.filename[len(name) + 1:]
                require((member.external_attr >> 16) == stat.S_IFREG | modes[relative], "Archive mode mismatch")
                with archive.open(member) as stream:
                    digest = hashlib.sha256()
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                        digest.update(chunk)
                require(digest.hexdigest() == sha256(stage / relative), "Archive content mismatch")


def package(args: argparse.Namespace, root: Path = ROOT) -> dict:
    require(re.fullmatch(r"[0-9a-fA-F]{40}|[0-9a-fA-F]{64}", args.sha) is not None,
            "--sha must be a full hexadecimal Git commit SHA (not an artifact hash or branch)")
    commit = args.sha.lower()
    if args.kind == "desktop":
        source = regular_file(args.binary)
        inspection = desktop_header(source, args.target)
        _, _, payload_name, suffix = TARGETS[args.target]
        target, profile = args.target, "release"
        mode = stat.S_IMODE(source.stat().st_mode) & 0o777
        if suffix == ".tar.gz":
            require(bool(mode & 0o111), "Unix desktop binary must already have executable permission")
        else:
            mode = 0o755  # PE executable; Unix mode is not authoritative on a Windows runner.
        signing = "No distribution identity signing or notarization; platform linker ad-hoc signatures may exist"
    else:
        source = regular_file(args.apk)
        inspection = android_structure(source)
        payload_name, suffix, target, profile, mode = "aTerminal-debug.apk", ".zip", "android", "debug", 0o644
        signing = "Gradle debug signing expected; APK signature must be verified separately with apksigner"
    stem = f"aTerminal-test-{target}-{commit[:12]}"
    output = args.output_dir or root / "dist"
    output.mkdir(parents=True, exist_ok=True)
    destinations = [output / (stem + suffix), output / (stem + suffix + ".sha256"),
                    output / (stem + ".build-info.json")]
    require(not any(p.exists() for p in destinations), f"Output already exists for {stem}; choose a clean --output-dir")
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", str(int(datetime.now(timezone.utc).timestamp()))))
    require(0 <= epoch <= 0xFFFFFFFF, "SOURCE_DATE_EPOCH must fit an unsigned 32-bit Unix timestamp")
    with tempfile.TemporaryDirectory(prefix=".test-package-", dir=output) as temporary:
        temporary = Path(temporary)
        stage = temporary / "contents"
        stage.mkdir()
        shutil.copyfile(source, stage / payload_name)
        modes = {payload_name: mode}
        documents = [("README.md", "README.md"), ("THIRD_PARTY.md", "THIRD_PARTY.md"),
                     ("deploy/TEST-PACKAGES.md", "TEST-PACKAGES.md")]
        if args.kind == "desktop":
            documents.append(("crates/desktop-agent/assets/fonts/LICENSE-Noto-CJK.txt", "LICENSE-Noto-CJK.txt"))
        for relative, archive_name in documents:
            shutil.copyfile(regular_file(root / relative), stage / archive_name)
            modes[archive_name] = 0o644
        manifest = {
            "schema_version": 1, "purpose": "test-only", "source_commit": commit,
            "source_commit_basis": "caller-supplied --sha; use checked-out git HEAD in CI",
            "target": target, "build_profile_expected": profile,
            "packaged_at_utc": datetime.fromtimestamp(epoch, timezone.utc).isoformat(),
            "signing_notes": signing, "header_or_apk_inspection": inspection,
            "ci": {key: os.environ[key] for key in ["GITHUB_REPOSITORY", "GITHUB_SHA", "GITHUB_RUN_ID",
                                                      "GITHUB_RUN_ATTEMPT", "GITHUB_WORKFLOW", "RUNNER_OS", "RUNNER_ARCH"]
                   if key in os.environ},
            "files": [{"path": relative, "size": (stage / relative).stat().st_size,
                       "sha256": sha256(stage / relative), "mode": format(modes[relative], "04o")}
                      for relative in sorted(modes)],
            "verification_limits": ["No runtime execution by this helper", "No cryptographic build attestation",
                                    "Build profile and source provenance require workflow evidence",
                                    "No complete third-party license compliance audit"],
        }
        (stage / "BUILD-INFO.json").write_bytes(json_bytes(manifest))
        modes["BUILD-INFO.json"] = 0o644
        checksums = "".join(f"{sha256(stage / relative)}  {relative}\n" for relative in sorted(modes))
        (stage / "SHA256SUMS").write_text(checksums, encoding="utf-8", newline="\n")
        modes["SHA256SUMS"] = 0o644
        archive = temporary / destinations[0].name
        write_archive(archive, stage, stem, modes, epoch)
        verify_archive(archive, stage, stem, modes)
        digest = sha256(archive)
        summary = {"archive": destinations[0].name, "archive_sha256": digest,
                   "archive_size": archive.stat().st_size, "archive_inventory_verified": True,
                   "build_info": manifest,
                   "archive_files": [{"path": relative, "sha256": sha256(stage / relative),
                                      "size": (stage / relative).stat().st_size, "mode": format(modes[relative], "04o")}
                                     for relative in sorted(modes)]}
        checksum = temporary / destinations[1].name
        checksum.write_text(f"{digest}  {archive.name}\n", encoding="utf-8", newline="\n")
        summary_path = temporary / destinations[2].name
        summary_path.write_bytes(json_bytes(summary))
        for source_path, destination in zip((archive, checksum, summary_path), destinations):
            os.replace(source_path, destination)
    return summary


def self_test() -> bool:
    """Synthetic fixtures exercise packaging only; these are not runnable binaries/APKs."""
    class PackagingTests(unittest.TestCase):
        def setUp(self):
            self.tmp = tempfile.TemporaryDirectory()
            self.addCleanup(self.tmp.cleanup)
            self.root = Path(self.tmp.name)
            (self.root / "deploy").mkdir()
            font = self.root / "crates/desktop-agent/assets/fonts/LICENSE-Noto-CJK.txt"
            font.parent.mkdir(parents=True)
            font.write_text("Synthetic license fixture\n", encoding="utf-8")
            for name in ("README.md", "THIRD_PARTY.md", "deploy/TEST-PACKAGES.md"):
                (self.root / name).write_text("Synthetic test fixture\n", encoding="utf-8")

        @staticmethod
        def elf(bits=2, machine=62, kind=3):
            data = bytearray(64 if bits == 2 else 52)
            data[:7] = b"\x7fELF" + bytes((bits, 1, 1))
            struct.pack_into("<HHI", data, 16, kind, machine, 1)
            struct.pack_into("<H", data, 52 if bits == 2 else 40, len(data))
            return bytes(data)

        def binary(self, target):
            if "linux" in target:
                data = self.elf()
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
            path = self.root / "binary"
            path.write_bytes(data)
            path.chmod(0o751)
            return path

        def apk(self, omit=None, wrong=False, duplicate=False):
            path = self.root / "input.apk"
            with zipfile.ZipFile(path, "w") as apk:
                apk.writestr("AndroidManifest.xml", b"synthetic manifest")
                apk.writestr("classes.dex", b"synthetic dex")
                for abi, (bits, machine) in ABIS.items():
                    for library in NATIVE_LIBS:
                        name = f"lib/{abi}/{library}"
                        if name != omit:
                            apk.writestr(name, self.elf(bits, 3 if wrong else machine))
                if duplicate:
                    import warnings
                    with warnings.catch_warnings():
                        warnings.simplefilter("ignore", UserWarning)
                        apk.writestr("classes.dex", b"duplicate")
            return path

        def args(self, target=None, **kwargs):
            target = target or ("x86_64-pc-windows-msvc" if os.name == "nt" else "x86_64-unknown-linux-gnu")
            return argparse.Namespace(kind="desktop", target=target, binary=self.binary(target),
                                      sha="a" * 40, output_dir=None, **kwargs)

        def test_all_desktop_packages_and_permissions(self):
            for target in TARGETS:
                with self.subTest(target=target):
                    args = self.args(target)
                    self.assertEqual(desktop_header(args.binary, target)["architecture"], TARGETS[target][1])
                    if os.name == "nt" and "windows" not in target:
                        continue  # Windows chmod cannot create the POSIX modes tested below.
                    result = package(args, self.root)
                    self.assertEqual(result["build_info"]["target"], target)
                    payload = next(
                        f for f in result["build_info"]["files"] if f["path"].startswith("aTerminal"))
                    self.assertEqual(payload["mode"], "0755" if "windows" in target else "0751")
                    self.assertEqual(result["archive_sha256"], sha256(self.root / "dist" / result["archive"]))

        def test_wrong_desktop_architecture(self):
            path = self.binary("aarch64-apple-darwin")
            with self.assertRaises(ValueError):
                desktop_header(path, "x86_64-apple-darwin")
            path.write_bytes(self.elf(machine=183))
            with self.assertRaises(ValueError):
                desktop_header(path, "x86_64-unknown-linux-gnu")

        def test_android_package(self):
            args = argparse.Namespace(kind="android", apk=self.apk(), sha="b" * 40, output_dir=None)
            result = package(args, self.root)
            self.assertEqual(len(result["build_info"]["header_or_apk_inspection"]["native_libraries"]), 6)

        def test_android_rejects_missing_wrong_and_duplicate(self):
            for options in ({"omit": "lib/x86/libc++_shared.so"}, {"wrong": True}, {"duplicate": True}):
                with self.subTest(options=options), self.assertRaises(ValueError):
                    android_structure(self.apk(**options))

        def test_invalid_commit_and_nonexecutable(self):
            args = self.args()
            args.sha = "main"
            with self.assertRaises(ValueError):
                package(args, self.root)
            if os.name == "nt":
                return  # Windows packaging does not rely on POSIX execution permissions.
            args.sha = "a" * 40
            args.binary.chmod(0o644)
            with self.assertRaises(ValueError):
                package(args, self.root)

        def test_no_overwrite(self):
            args = self.args()
            result = package(args, self.root)
            before = sha256(self.root / "dist" / result["archive"])
            with self.assertRaises(ValueError):
                package(args, self.root)
            self.assertEqual(before, sha256(self.root / "dist" / result["archive"]))

    return unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(PackagingTests)).wasSuccessful()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true", help="test synthetic fixtures without compiling or executing aTerminal")
    subparsers = parser.add_subparsers(dest="kind")
    for kind in ("desktop", "android"):
        command = subparsers.add_parser(kind)
        command.add_argument("--sha", required=True, help="full checked-out Git commit SHA, not a payload hash")
        command.add_argument("--output-dir", type=Path, help="output directory (default: repository/dist); refuses overwrites")
        if kind == "desktop":
            command.add_argument("--target", required=True, choices=TARGETS)
            command.add_argument("--binary", required=True, type=Path)
        else:
            command.add_argument("--apk", required=True, type=Path)
    args = parser.parse_args()
    if args.self_test:
        require(args.kind is None, "Use --self-test without a packaging subcommand")
        return 0 if self_test() else 1
    if not args.kind:
        parser.error("choose desktop or android, or use --self-test")
    try:
        print(json.dumps(package(args), sort_keys=True))
    except (OSError, ValueError, zipfile.BadZipFile, tarfile.TarError, RuntimeError) as error:
        parser.exit(1, f"Packaging failed: {error}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
