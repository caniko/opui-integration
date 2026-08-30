#!/usr/bin/env python3
"""Materialize Cargo path dependencies before any Cargo command runs."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import urlsplit

REQUIRED = {
    "opui": "../opui",
    "bevy_openpencil": "../bevy_openpencil",
}
SHA_RE = re.compile(r"[0-9a-f]{40}")


@dataclass(frozen=True)
class Sibling:
    id: str
    url: str
    sha: str
    destination: Path


def fail(message: str) -> RuntimeError:
    return RuntimeError(message)


def unique_by_id(items: object, section: str) -> dict[str, dict[str, object]]:
    if not isinstance(items, list):
        raise fail(f"{section} must be an array")
    result: dict[str, dict[str, object]] = {}
    for item in items:
        if not isinstance(item, dict) or not isinstance(item.get("id"), str):
            raise fail(f"invalid {section} entry")
        item_id = item["id"]
        if item_id in result:
            raise fail(f"duplicate {section} id {item_id}")
        result[item_id] = item
    return result


def validate_url(url: object) -> str:
    if not isinstance(url, str) or "\\" in url or any(ord(c) < 32 for c in url):
        raise fail("repository URL must be a public HTTPS URL")
    parsed = urlsplit(url)
    try:
        port = parsed.port
    except ValueError as error:
        raise fail(f"rejected repository URL {url!r}") from error
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or port not in (None, 443)
        or parsed.query
        or parsed.fragment
        or not parsed.path.endswith(".git")
        or any(part in ("", ".", "..") for part in parsed.path.split("/")[1:])
    ):
        raise fail(f"rejected repository URL {url!r}")
    return url


def load_siblings(root: Path) -> list[Sibling]:
    root = root.resolve(strict=True)
    lock_path = root / "repos.lock.toml"
    if lock_path.is_symlink() or not lock_path.is_file():
        raise fail("repos.lock.toml must be a regular file")
    with lock_path.open("rb") as handle:
        lock = tomllib.load(handle)
    if lock.get("format_version") != 1:
        raise fail("unsupported repos.lock.toml format")
    repositories = unique_by_id(lock.get("repositories"), "repositories")
    sources = unique_by_id(lock.get("public_sources"), "public_sources")
    workspace = root.parent.resolve(strict=True)
    siblings = []
    for item_id, expected_path in REQUIRED.items():
        repo = repositories.get(item_id)
        source = sources.get(item_id)
        if repo is None or source is None:
            raise fail(f"missing required repository {item_id}")
        if repo.get("path") != expected_path:
            raise fail(f"unexpected destination for {item_id}")
        sha = repo.get("sha")
        if not isinstance(sha, str) or SHA_RE.fullmatch(sha) is None:
            raise fail(f"invalid SHA for {item_id}")
        if source.get("sha") != sha or source.get("relation") != "exact":
            raise fail(f"public source mismatch for {item_id}")
        destination = (root / expected_path).resolve(strict=False)
        if destination.parent != workspace or destination.name != Path(expected_path).name:
            raise fail(f"destination escapes workspace for {item_id}")
        siblings.append(Sibling(item_id, validate_url(source.get("url")), sha, destination))
    return siblings


def require_absent(destination: Path) -> None:
    if destination.is_symlink() or os.path.lexists(destination):
        raise fail(f"destination already exists: {destination}")


def git_env() -> dict[str, str]:
    env = os.environ.copy()
    for name in tuple(env):
        if name.startswith(("GIT_", "SSH_")) or name in ("GH_TOKEN", "GITHUB_TOKEN"):
            env.pop(name)
    env.update(
        {
            "GIT_CONFIG_COUNT": "0",
            "GIT_CONFIG_GLOBAL": "/dev/null",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_TERMINAL_PROMPT": "0",
        }
    )
    return env


def git(*args: str, cwd: Path | None = None, capture: bool = False) -> str:
    command = ["git", "-c", "credential.helper=", "-c", "http.extraHeader=", *args]
    result = subprocess.run(
        command,
        cwd=cwd,
        env=git_env(),
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )
    return result.stdout.strip() if capture else ""


def stage(sibling: Sibling, directory: Path) -> Path:
    repository = directory / sibling.id
    git("clone", "--no-checkout", "--", sibling.url, str(repository))
    git("checkout", "--detach", sibling.sha, cwd=repository)
    if git("rev-parse", "HEAD", cwd=repository, capture=True) != sibling.sha:
        raise fail(f"checkout mismatch for {sibling.id}")
    if git("remote", "get-url", "origin", cwd=repository, capture=True) != sibling.url:
        raise fail(f"origin mismatch for {sibling.id}")
    if git("status", "--porcelain", cwd=repository, capture=True):
        raise fail(f"dirty checkout for {sibling.id}")
    return repository


def bootstrap(root: Path) -> None:
    siblings = load_siblings(root)
    for sibling in siblings:
        require_absent(sibling.destination)
    staging = Path(tempfile.mkdtemp(prefix=".opui-bootstrap-", dir=root.parent))
    try:
        repositories = [(sibling, stage(sibling, staging)) for sibling in siblings]
        for sibling, _ in repositories:
            require_absent(sibling.destination)
        for sibling, repository in repositories:
            repository.rename(sibling.destination)
    finally:
        shutil.rmtree(staging, ignore_errors=True)


def main() -> int:
    try:
        bootstrap(Path(__file__).resolve().parent.parent)
    except (OSError, subprocess.CalledProcessError, RuntimeError, tomllib.TOMLDecodeError) as error:
        print(f"bootstrap failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
