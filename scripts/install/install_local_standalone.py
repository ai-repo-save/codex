#!/usr/bin/env python3
"""Build the local Codex workspace and install a standalone package.

This script encodes the canonical local install layout so agents and developers
do not need to rediscover paths or pass fragile --bwrap-bin locations.

Key properties:
- Builds into a staging directory, then atomically replaces the release dir.
- Keeps a target-scoped bwrap cache outside any release directory.
- Never points --bwrap-bin at a path inside the directory being replaced.

Local iteration defaults to the ``dev-small`` Cargo profile (same as
``build_codex_package.py``). Pass ``--cargo-profile release`` only when you
intentionally want a production-like binary; release enables fat LTO and often
spends many minutes in the final link step.
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import platform
import shutil
import stat
import subprocess
import sys
from dataclasses import asdict
from dataclasses import dataclass
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
BUILD_PACKAGE_SCRIPT = REPO_ROOT / "scripts" / "build_codex_package.py"

sys.path.insert(0, str(REPO_ROOT / "scripts"))

from codex_package.cargo import cargo_profile_output_dir  # noqa: E402
from codex_package.cargo import cargo_target_dir  # noqa: E402
from codex_package.layout import validate_package_dir  # noqa: E402
from codex_package.targets import PACKAGE_VARIANTS  # noqa: E402
from codex_package.targets import TARGET_SPECS  # noqa: E402
from codex_package.targets import normalize_machine  # noqa: E402
from codex_package.v8 import default_cache_root  # noqa: E402
from codex_package.version import read_workspace_version  # noqa: E402


DEFAULT_BIN_DIR = Path.home() / ".local" / "bin"
DEFAULT_CODEX_HOME = Path.home() / ".codex"
DEFAULT_CARGO_PROFILE = "dev-small"
DEFAULT_VARIANT = "codex"
PRODUCTION_CARGO_PROFILE = "release"
LOCAL_HOST_TARGETS: dict[tuple[str, str], str] = {
    ("linux", "aarch64"): "aarch64-unknown-linux-gnu",
    ("linux", "x86_64"): "x86_64-unknown-linux-gnu",
    ("darwin", "aarch64"): "aarch64-apple-darwin",
    ("darwin", "x86_64"): "x86_64-apple-darwin",
    ("windows", "aarch64"): "aarch64-pc-windows-msvc",
    ("windows", "x86_64"): "x86_64-pc-windows-msvc",
}


def default_local_target() -> str:
    system = platform.system().lower()
    machine = normalize_machine(platform.machine())
    target = LOCAL_HOST_TARGETS.get((system, machine))
    if target is None:
        supported = ", ".join(sorted(TARGET_SPECS))
        raise RuntimeError(
            f"Unsupported host platform {platform.system()}/{platform.machine()}. "
            f"Pass --target explicitly. Supported targets: {supported}"
        )
    return target


@dataclass(frozen=True)
class InstallPaths:
    codex_home: Path
    bin_dir: Path
    bin_path: Path
    standalone_root: Path
    releases_dir: Path
    current_link: Path
    vendor_dir: Path
    target: str
    version: str
    cargo_profile: str
    release_name: str
    release_dir: Path
    bwrap_cache: Path
    cargo_entrypoint: Path

    @classmethod
    def resolve(
        cls,
        *,
        codex_home: Path | None,
        bin_dir: Path | None,
        target: str | None,
        variant: str,
        cargo_profile: str,
    ) -> InstallPaths:
        resolved_codex_home = (codex_home or Path(os.environ.get("CODEX_HOME", DEFAULT_CODEX_HOME))).expanduser().resolve()
        resolved_bin_dir = (bin_dir or Path(os.environ.get("CODEX_INSTALL_DIR", DEFAULT_BIN_DIR))).expanduser().resolve()
        resolved_target = target or default_local_target()
        resolved_version = read_workspace_version()
        package_variant = PACKAGE_VARIANTS[variant]
        spec = TARGET_SPECS[resolved_target]
        release_name = f"{resolved_version}-{resolved_target}"
        if cargo_profile != PRODUCTION_CARGO_PROFILE:
            release_name = f"{release_name}-{cargo_profile}"
        standalone_root = resolved_codex_home / "packages" / "standalone"
        releases_dir = standalone_root / "releases"
        vendor_dir = standalone_root / "vendor"
        entrypoint = cargo_profile_output_dir(spec, cargo_profile) / package_variant.entrypoint_name(spec)
        return cls(
            codex_home=resolved_codex_home,
            bin_dir=resolved_bin_dir,
            bin_path=resolved_bin_dir / "codex",
            standalone_root=standalone_root,
            releases_dir=releases_dir,
            current_link=standalone_root / "current",
            vendor_dir=vendor_dir,
            target=resolved_target,
            version=resolved_version,
            cargo_profile=cargo_profile,
            release_name=release_name,
            release_dir=releases_dir / release_name,
            bwrap_cache=vendor_dir / resolved_target / "bwrap",
            cargo_entrypoint=entrypoint,
        )


def configure_logging(*, verbose: bool) -> None:
    level = logging.DEBUG if verbose else logging.INFO
    logging.basicConfig(level=level, format="%(levelname)s %(message)s", stream=sys.stderr, force=True)


def is_executable(path: Path) -> bool:
    return path.is_file() and bool(path.stat().st_mode & stat.S_IXUSR)


def copy_executable(src: Path, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dest)
    mode = dest.stat().st_mode
    dest.chmod(mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def replace_symlink(link_path: Path, target: Path) -> None:
    link_path.parent.mkdir(parents=True, exist_ok=True)
    tmp_link = link_path.with_name(f".{link_path.name}.{os.getpid()}")
    if tmp_link.exists() or tmp_link.is_symlink():
        tmp_link.unlink()
    tmp_link.symlink_to(target)
    tmp_link.replace(link_path)


def discover_bwrap(paths: InstallPaths) -> Path | None:
    candidates: list[Path] = [
        paths.bwrap_cache,
        paths.current_link / "codex-resources" / "bwrap",
        paths.release_dir / "codex-resources" / "bwrap",
    ]
    candidates.extend(find_pnpm_bwrap_candidates(paths.target))

    seen: set[Path] = set()
    for candidate in candidates:
        resolved = candidate.expanduser().resolve()
        if resolved in seen:
            continue
        seen.add(resolved)
        if is_executable(resolved):
            return resolved
    return None


def find_pnpm_bwrap_candidates(target: str) -> list[Path]:
    home = Path.home()
    search_roots = [
        home / ".local" / "share" / "pnpm" / "global",
        home / ".local" / "share" / "pnpm",
    ]
    suffix = Path("vendor") / target / "codex-resources" / "bwrap"
    matches: list[Path] = []
    for root in search_roots:
        if not root.is_dir():
            continue
        direct = root / suffix
        if direct.is_file():
            matches.append(direct)
        for path in root.rglob("codex-resources/bwrap"):
            if path.is_file():
                matches.append(path)
    return matches


def refresh_bwrap_cache(paths: InstallPaths, source: Path) -> Path:
    if source.resolve() == paths.bwrap_cache.resolve():
        logging.debug("bwrap cache already current at %s", paths.bwrap_cache)
        return paths.bwrap_cache

    logging.info("Caching bwrap at %s", paths.bwrap_cache)
    copy_executable(source, paths.bwrap_cache)
    return paths.bwrap_cache


def resolve_bwrap(paths: InstallPaths) -> Path | None:
    spec = TARGET_SPECS[paths.target]
    if not spec.is_linux:
        return None

    discovered = discover_bwrap(paths)
    if discovered is None:
        logging.warning(
            "No prebuilt bwrap found. The package builder will try to compile bwrap; "
            "musl cross-compiles often fail on Linux hosts."
        )
        return None

    return refresh_bwrap_cache(paths, discovered)


def build_package(
    paths: InstallPaths,
    *,
    staging_dir: Path,
    cargo_profile: str,
    variant: str,
    skip_build: bool,
    bwrap_bin: Path | None,
) -> None:
    cmd = [
        sys.executable,
        str(BUILD_PACKAGE_SCRIPT),
        "--target",
        paths.target,
        "--variant",
        variant,
        "--cargo-profile",
        cargo_profile,
        "--package-dir",
        str(staging_dir),
        "--force",
    ]
    if skip_build:
        if not paths.cargo_entrypoint.is_file():
            raise RuntimeError(
                f"Missing prebuilt entrypoint for --skip-build: {paths.cargo_entrypoint}"
            )
        cmd.extend(["--entrypoint-bin", str(paths.cargo_entrypoint)])
    if bwrap_bin is not None:
        cmd.extend(["--bwrap-bin", str(bwrap_bin)])

    logging.info("Running package build: %s", " ".join(cmd))
    subprocess.run(cmd, cwd=REPO_ROOT, check=True)


def finalize_package_layout(staging_dir: Path) -> None:
    legacy_entrypoint = staging_dir / "codex"
    package_entrypoint = staging_dir / "bin" / "codex"
    if package_entrypoint.is_file() and not legacy_entrypoint.exists():
        legacy_entrypoint.symlink_to("bin/codex")


def activate_release(staging_dir: Path, release_dir: Path) -> None:
    if release_dir.exists() or release_dir.is_symlink():
        logging.info("Replacing existing release at %s", release_dir)
        shutil.rmtree(release_dir)
    staging_dir.replace(release_dir)


def update_install_links(paths: InstallPaths) -> None:
    replace_symlink(paths.current_link, paths.release_dir)
    replace_symlink(paths.bin_path, paths.current_link / "bin" / "codex")


def verify_install(paths: InstallPaths, *, variant: str) -> None:
    spec = TARGET_SPECS[paths.target]
    validate_package_dir(
        paths.release_dir,
        PACKAGE_VARIANTS[variant],
        spec,
        include_zsh=True,
    )
    subprocess.run([str(paths.bin_path), "--version"], check=True)


def git_tracked_codex_rs_files() -> list[str]:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "ls-files", "--", "codex-rs"],
        capture_output=True,
        text=True,
        check=True,
    )
    return sorted(line.strip() for line in result.stdout.splitlines() if line.strip())


def is_relevant_build_input(relative_path: str) -> bool:
    return (
        relative_path.endswith((".rs", ".toml"))
        or relative_path.endswith("Cargo.lock")
        or relative_path.startswith("codex-rs/hooks/schema/generated/")
    )


def entrypoint_build_status(paths: InstallPaths) -> tuple[str, list[str]]:
    entrypoint = paths.cargo_entrypoint
    if not entrypoint.is_file():
        return "missing", ["entrypoint binary does not exist yet"]

    bin_mtime = entrypoint.stat().st_mtime
    stale_files: list[str] = []
    for relative_path in git_tracked_codex_rs_files():
        if not is_relevant_build_input(relative_path):
            continue
        source_path = REPO_ROOT / relative_path
        if source_path.is_file() and source_path.stat().st_mtime > bin_mtime:
            stale_files.append(relative_path)

    if stale_files:
        return "stale", stale_files
    return "fresh", []


def resolve_build_mode(build_mode: str, skip_build: bool, paths: InstallPaths) -> str:
    if skip_build:
        return "never"
    return build_mode


def should_skip_cargo(build_mode: str, paths: InstallPaths) -> tuple[bool, str]:
    if build_mode == "never":
        if not paths.cargo_entrypoint.is_file():
            raise RuntimeError(
                f"--build never requires an existing entrypoint: {paths.cargo_entrypoint}"
            )
        return True, "build mode is never"

    if build_mode == "always":
        return False, "build mode is always"

    status, details = entrypoint_build_status(paths)
    if status == "fresh":
        return True, "entrypoint is newer than changed codex-rs sources"
    if status == "missing":
        return False, "entrypoint missing"
    preview = ", ".join(details[:5])
    if len(details) > 5:
        preview = f"{preview}, ..."
    return False, f"{len(details)} changed source file(s) newer than entrypoint: {preview}"


def format_bytes(num_bytes: int) -> str:
    units = ["B", "KiB", "MiB", "GiB"]
    size = float(num_bytes)
    for unit in units:
        if size < 1024 or unit == units[-1]:
            return f"{size:.1f} {unit}"
        size /= 1024
    raise AssertionError("unreachable")


def directory_size(path: Path) -> int | None:
    if not path.is_dir():
        return None
    total = 0
    for child in path.rglob("*"):
        if child.is_file():
            total += child.stat().st_size
    return total


def collect_diagnosis(paths: InstallPaths, *, build_mode: str, skip_build: bool) -> dict[str, object]:
    resolved_build_mode = resolve_build_mode(build_mode, skip_build, paths)
    status, details = entrypoint_build_status(paths)
    skip_cargo, skip_reason = should_skip_cargo(resolved_build_mode, paths)
    target_output_dir = paths.cargo_entrypoint.parent
    v8_cache_root = default_cache_root()

    recommendation = "repackage only (--build auto or --build never)"
    if not skip_cargo:
        if paths.cargo_profile == PRODUCTION_CARGO_PROFILE:
            recommendation = (
                "cargo rebuild required; release uses fat LTO and codegen-units=1, "
                "so expect many minutes in compile/link"
            )
        else:
            recommendation = (
                f"cargo rebuild required with {paths.cargo_profile}; this is the local "
                "dev path and should be much faster than release"
            )

    entrypoint_info: dict[str, object] | None = None
    if paths.cargo_entrypoint.is_file():
        stat_result = paths.cargo_entrypoint.stat()
        entrypoint_info = {
            "path": str(paths.cargo_entrypoint),
            "sizeBytes": stat_result.st_size,
            "mtimeUnix": int(stat_result.st_mtime),
        }

    return {
        "paths": {key: str(value) for key, value in asdict(paths).items()},
        "build": {
            "requestedMode": resolved_build_mode,
            "cargoProfile": paths.cargo_profile,
            "entrypointStatus": status,
            "staleSources": details,
            "willSkipCargo": skip_cargo,
            "decisionReason": skip_reason,
            "recommendation": recommendation,
        },
        "cache": {
            "cargoTargetDir": str(cargo_target_dir()),
            "cargoOutputDir": str(target_output_dir),
            "cargoOutputDirSizeBytes": directory_size(target_output_dir),
            "v8CacheRoot": str(v8_cache_root),
            "v8CacheExists": v8_cache_root.is_dir(),
            "bwrapCacheExists": paths.bwrap_cache.is_file(),
        },
        "notes": [
            "Default cargo profile is dev-small for local iteration; release is opt-in.",
            "just test builds debug artifacts under codex-rs/target/debug, not dev-small musl.",
            "V8 prebuilt artifacts are cached under $TMPDIR/codex-package by build_codex_package.py.",
            "Use --build never to repackage in about one second when the entrypoint is already current.",
            "Use --cargo-profile release only when you need a production-like binary size/LTO build.",
        ],
        "entrypoint": entrypoint_info,
    }


def print_diagnosis(diagnosis: dict[str, object], *, as_json: bool) -> None:
    if as_json:
        print(json.dumps(diagnosis, indent=2))
        return

    build = diagnosis["build"]
    assert isinstance(build, dict)
    cache = diagnosis["cache"]
    assert isinstance(cache, dict)
    notes = diagnosis["notes"]
    assert isinstance(notes, list)

    print("Local standalone install diagnosis")
    print(f"  recommendation: {build['recommendation']}")
    print(f"  build mode: {build['requestedMode']}")
    print(f"  cargo profile: {build['cargoProfile']}")
    print(f"  entrypoint status: {build['entrypointStatus']}")
    print(f"  will skip cargo: {build['willSkipCargo']} ({build['decisionReason']})")
    stale_sources = build["staleSources"]
    assert isinstance(stale_sources, list)
    if stale_sources:
        print("  stale sources:")
        for relative_path in stale_sources:
            print(f"    - {relative_path}")

    release_size = cache["cargoOutputDirSizeBytes"]
    if isinstance(release_size, int):
        print(f"  cargo output size: {format_bytes(release_size)}")
    print(f"  cargo output dir: {cache['cargoOutputDir']}")
    print(f"  v8 cache root: {cache['v8CacheRoot']} (exists={cache['v8CacheExists']})")
    print(f"  bwrap cache: {cache['bwrapCacheExists']}")

    entrypoint = diagnosis["entrypoint"]
    if isinstance(entrypoint, dict):
        print(f"  entrypoint: {entrypoint['path']} ({format_bytes(int(entrypoint['sizeBytes']))})")

    print("Notes:")
    for note in notes:
        if isinstance(note, str):
            print(f"  - {note}")


def print_paths(paths: InstallPaths, *, as_json: bool) -> None:
    payload = asdict(paths)
    payload = {key: str(value) for key, value in payload.items()}
    if as_json:
        print(json.dumps(payload, indent=2))
        return

    print("Local standalone install paths")
    for key, value in payload.items():
        print(f"  {key}: {value}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build the local Codex workspace and install a standalone release.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--codex-home", type=Path, help="Codex home directory (default: $CODEX_HOME or ~/.codex).")
    parser.add_argument("--bin-dir", type=Path, help="Directory for the visible codex symlink (default: $CODEX_INSTALL_DIR or ~/.local/bin).")
    parser.add_argument("--target", choices=sorted(TARGET_SPECS), help="Rust target triple for the package.")
    parser.add_argument("--variant", choices=sorted(PACKAGE_VARIANTS), default=DEFAULT_VARIANT)
    parser.add_argument(
        "--cargo-profile",
        default=DEFAULT_CARGO_PROFILE,
        help=(
            "Cargo profile for local builds. Defaults to dev-small for fast iteration; "
            "use release only for production-like binaries."
        ),
    )
    parser.add_argument(
        "--build",
        choices=["auto", "always", "never"],
        default="auto",
        help=(
            "auto: repackage when the release entrypoint is newer than changed codex-rs "
            "sources, otherwise rebuild; always: invoke Cargo every time; never: repackage "
            "only."
        ),
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="Deprecated alias for --build never.",
    )
    parser.add_argument("--print-paths", action="store_true", help="Print resolved install paths and exit.")
    parser.add_argument(
        "--diagnose",
        action="store_true",
        help="Print cache/build recommendations without invoking Cargo or repackaging.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Use with --print-paths or --diagnose to emit JSON.",
    )
    parser.add_argument("--verbose", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    configure_logging(verbose=args.verbose)
    paths = InstallPaths.resolve(
        codex_home=args.codex_home,
        bin_dir=args.bin_dir,
        target=args.target,
        variant=args.variant,
        cargo_profile=args.cargo_profile,
    )

    build_mode = resolve_build_mode(args.build, args.skip_build, paths)

    if args.print_paths:
        print_paths(paths, as_json=args.json)
        return 0

    if args.diagnose:
        print_diagnosis(
            collect_diagnosis(paths, build_mode=build_mode, skip_build=args.skip_build),
            as_json=args.json,
        )
        return 0

    paths.releases_dir.mkdir(parents=True, exist_ok=True)
    staging_dir = paths.releases_dir / f".staging.{paths.release_name}.{os.getpid()}"

    skip_cargo, skip_reason = should_skip_cargo(build_mode, paths)
    logging.info("Release target: %s", paths.release_name)
    logging.info("Cargo profile: %s", paths.cargo_profile)
    if paths.cargo_profile == PRODUCTION_CARGO_PROFILE:
        logging.warning(
            "Using release profile locally enables fat LTO; prefer dev-small unless you "
            "explicitly need a production-like binary."
        )
    logging.info("Staging directory: %s", staging_dir)
    logging.info("Visible command: %s", paths.bin_path)
    logging.info("Build mode: %s (%s)", build_mode, skip_reason)

    bwrap_bin = resolve_bwrap(paths)
    try:
        build_package(
            paths,
            staging_dir=staging_dir,
            cargo_profile=args.cargo_profile,
            variant=args.variant,
            skip_build=skip_cargo,
            bwrap_bin=bwrap_bin,
        )
        finalize_package_layout(staging_dir)
        validate_package_dir(
            staging_dir,
            PACKAGE_VARIANTS[args.variant],
            TARGET_SPECS[paths.target],
            include_zsh=True,
        )

        packaged_bwrap = staging_dir / "codex-resources" / "bwrap"
        if packaged_bwrap.is_file():
            refresh_bwrap_cache(paths, packaged_bwrap)

        activate_release(staging_dir, paths.release_dir)
        update_install_links(paths)
        verify_install(paths, variant=args.variant)
    except Exception:
        if staging_dir.exists():
            logging.error("Removing failed staging directory: %s", staging_dir)
            shutil.rmtree(staging_dir, ignore_errors=True)
        raise

    logging.info("Installed standalone Codex %s to %s", paths.version, paths.release_dir)
    logging.info("Run: %s", paths.bin_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
