#!/usr/bin/env python3
"""Package checksummed release binaries and create a signed apt repository."""

import argparse
import gzip
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile


TARGETS = {"amd64": "x86_64", "arm64": "aarch64"}
LICENSE = Path(__file__).resolve().parents[1] / "LICENSE"


def command(*args, **kwargs):
    return subprocess.check_output(args, **kwargs)


def build(version, artifacts, output, public_key):
    if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
        raise ValueError("version must be a stable semantic version")
    if not os.environ.get("GNUPGHOME"):
        raise ValueError("GNUPGHOME must name the repository signing-key directory")
    if output.exists():
        raise ValueError("output directory must not already exist")
    public_key = public_key.resolve()
    key_info = command("gpg", "--batch", "--with-colons", "--show-keys", str(public_key)).decode()
    fingerprint = next(line.split(":")[9] for line in key_info.splitlines() if line.startswith("fpr:"))
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="undo-apt-", dir=output.parent) as temporary:
        temporary = Path(temporary)
        repo = temporary / "repository"
        repo.mkdir()
        for architecture, cpu in TARGETS.items():
            name = f"undo-cli-{cpu}-unknown-linux-gnu.tar.xz"
            archive = artifacts / name
            checksum = (artifacts / f"{name}.sha256").read_text().split()
            if len(checksum) != 2 or checksum[1].lstrip("*") != name:
                raise ValueError(f"invalid checksum record for {name}")
            if hashlib.sha256(archive.read_bytes()).hexdigest() != checksum[0]:
                raise ValueError(f"checksum mismatch for {name}")
            with tarfile.open(archive) as tar:
                binaries = [member for member in tar.getmembers() if Path(member.name).name == "undo"]
                if len(binaries) != 1 or not binaries[0].isfile():
                    raise ValueError(f"expected one regular undo binary in {name}")
                binary = tar.extractfile(binaries[0]).read()
            stage = temporary / architecture
            (stage / "DEBIAN").mkdir(parents=True)
            (stage / "usr/bin").mkdir(parents=True)
            (stage / "usr/share/doc/undo").mkdir(parents=True)
            (stage / "usr/bin/undo").write_bytes(binary)
            (stage / "usr/bin/undo").chmod(0o755)
            shutil.copyfile(LICENSE, stage / "usr/share/doc/undo/copyright")
            # GNU release binaries are built on Ubuntu 22.04.
            (stage / "DEBIAN/control").write_text(
                f"Package: undo\nVersion: {version}\nArchitecture: {architecture}\n"
                "Maintainer: dpologdvinity <dpologdvinity@users.noreply.github.com>\n"
                "Section: utils\nPriority: optional\n"
                "Depends: git (>= 2.25), libc6 (>= 2.35), libgcc-s1\n"
                "Homepage: https://github.com/dpologdvinity/undo\n"
                "Description: Ctrl-Z for your terminal and your AI agent\n"
            )
            for path in [*stage.rglob("*"), stage]:
                path.chmod(0o755 if path.is_dir() or path == stage / "usr/bin/undo" else 0o644)
                os.utime(path, (0, 0))
            pool = repo / f"pool/main/{architecture}"
            pool.mkdir(parents=True)
            package = pool / f"undo_{version}_{architecture}.deb"
            command(
                "dpkg-deb", "--root-owner-group", "--build", str(stage), str(package),
                env={**os.environ, "SOURCE_DATE_EPOCH": "0"},
            )
            digest = hashlib.sha256(package.read_bytes()).hexdigest()
            package.with_suffix(".deb.sha256").write_text(f"{digest}  {package.name}\n")
            index = repo / f"dists/stable/main/binary-{architecture}/Packages"
            index.parent.mkdir(parents=True)
            contents = command("apt-ftparchive", "packages", f"pool/main/{architecture}", cwd=repo)
            index.write_bytes(contents)
            index.with_suffix(".gz").write_bytes(gzip.compress(contents, mtime=0))
        release = repo / "dists/stable/Release"
        release.write_bytes(command(
            "apt-ftparchive", "-o", "APT::FTPArchive::Release::Origin=undo",
            "-o", "APT::FTPArchive::Release::Label=undo",
            "-o", "APT::FTPArchive::Release::Suite=stable",
            "-o", "APT::FTPArchive::Release::Codename=stable",
            "-o", "APT::FTPArchive::Release::Architectures=amd64 arm64",
            "-o", "APT::FTPArchive::Release::Components=main", "release", "dists/stable", cwd=repo,
        ))
        for option, name in (("--clearsign", "InRelease"), ("--detach-sign", "Release.gpg")):
            command(
                "gpg", "--batch", "--yes", "--armor", "--local-user", fingerprint,
                "--output", str(release.with_name(name)), option, str(release),
            )
        shutil.copyfile(public_key, repo / "undo.asc")
        shutil.move(repo, output)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("artifacts", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("public_key", type=Path)
    args = parser.parse_args()
    try:
        build(args.version, args.artifacts.resolve(), args.output.resolve(), args.public_key)
    except (OSError, ValueError, subprocess.CalledProcessError, tarfile.TarError) as error:
        sys.exit(f"build-apt-repo: {error}")
