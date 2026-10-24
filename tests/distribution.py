import gzip
import hashlib
import io
import os
from pathlib import Path
import pwd
import subprocess
import sys
import tarfile
import tempfile
import unittest


BUILDER = Path(__file__).resolve().parents[1] / "scripts/build-apt-repo.py"
TARGETS = {"amd64": "x86_64", "arm64": "aarch64"}


class DistributionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="undo-task2-distribution-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.gnupg = self.home / ".gnupg"
        self.gnupg.mkdir(mode=0o700)
        self.env = os.environ.copy()
        self.env.update(
            HOME=str(self.home),
            UNDO_HOME=str(self.home / ".local/share/undo"),
            GNUPGHOME=str(self.gnupg),
        )
        self.command(
            "gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "",
            "--quick-generate-key", "undo test <undo@example.invalid>", "ed25519", "sign", "0",
        )
        self.key = self.root / "key.asc"
        self.key.write_bytes(self.command("gpg", "--armor", "--export").stdout)
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()
        for cpu in TARGETS.values():
            stem = f"undo-cli-{cpu}-unknown-linux-gnu"
            archive = self.artifacts / f"{stem}.tar.xz"
            binary = b"#!/bin/sh\nprintf 'undo 0.1.0\\n'\n"
            member = tarfile.TarInfo(f"{stem}/undo")
            member.size = len(binary)
            member.mode = 0o755
            with tarfile.open(archive, "w:xz") as tar:
                tar.addfile(member, io.BytesIO(binary))
            checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
            archive.with_suffix(".xz.sha256").write_text(f"{checksum}  {archive.name}\n")

    def command(self, *args, check=True):
        result = subprocess.run(args, cwd=self.root, env=self.env, capture_output=True)
        if check:
            self.assertEqual(result.returncode, 0, result.stderr.decode())
        return result

    def build(self, destination="repo", version="0.1.0", check=True):
        output = self.root / destination
        result = self.command(
            sys.executable, str(BUILDER), version, str(self.artifacts), str(output), str(self.key),
            check=check,
        )
        return output, result

    def apt(self, repo, state="apt"):
        root = self.root / state
        for directory in (
            "etc/apt/apt.conf.d", "etc/apt/sources.list.d", "var/lib/apt/lists/partial",
            "var/cache/apt/archives/partial", "var/lib/dpkg", "var/log/apt",
        ):
            (root / directory).mkdir(parents=True, exist_ok=True)
        (root / "var/lib/dpkg/status").write_text("")
        (root / "etc/apt/sources.list").write_text(
            f"deb [arch=amd64 signed-by={repo}/undo.asc] {repo.as_uri()} stable main\n"
        )
        return [
            "apt-get", "-o", f"Dir={root}", "-o", f"Dir::State::status={root}/var/lib/dpkg/status",
            "-o", "APT::Architecture=amd64", "-o", "Acquire::Languages=none",
            "-o", f"APT::Sandbox::User={pwd.getpwuid(os.getuid()).pw_name}",
        ]

    def test_signed_repository_installs_both_architectures(self):
        repo, _ = self.build()
        for architecture in TARGETS:
            package = repo / f"pool/main/{architecture}/undo_0.1.0_{architecture}.deb"
            fields = self.command("dpkg-deb", "--field", str(package)).stdout.decode()
            self.assertIn("Package: undo\n", fields)
            self.assertIn(f"Architecture: {architecture}\n", fields)
            self.assertIn("git (>= 2.25)", fields)
            self.assertIn("libc6 (>= 2.35)", fields)
            self.assertIn("libgcc-s1", fields)
            listing = self.command("dpkg-deb", "--contents", str(package)).stdout.decode()
            self.assertIn("root/root", listing)
            self.assertIn("./usr/bin/undo", listing)
        apt = self.apt(repo)
        self.command(*apt, "update")
        self.command(*apt, "download", "undo:amd64")
        downloaded = self.root / "undo_0.1.0_amd64.deb"
        self.assertEqual(downloaded.read_bytes(), (repo / "pool/main/amd64" / downloaded.name).read_bytes())

        index = repo / "dists/stable/main/binary-amd64/Packages"
        index.write_bytes(index.read_bytes() + b"tampered\n")
        index.with_suffix(".gz").write_bytes(gzip.compress(index.read_bytes()))
        self.assertNotEqual(self.command(*self.apt(repo, "tampered-apt"), "update", check=False).returncode, 0)

    def test_rejects_incorrect_archive_checksum(self):
        archive = next(self.artifacts.glob("*.tar.xz"))
        archive.write_bytes(archive.read_bytes() + b"tampered")
        repo, result = self.build(check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"checksum", result.stderr.lower())
        self.assertFalse(repo.exists())

    def test_packages_are_reproducible(self):
        first, _ = self.build("first")
        second, _ = self.build("second")
        for package in first.glob("pool/main/*/*.deb"):
            self.assertEqual(package.read_bytes(), (second / package.relative_to(first)).read_bytes())

    def test_rejects_invalid_version(self):
        repo, result = self.build(version="0.1.0\nInjected: true", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"version", result.stderr)
        self.assertFalse(repo.exists())


if __name__ == "__main__":
    unittest.main()
