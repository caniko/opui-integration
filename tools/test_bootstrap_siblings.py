import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import bootstrap_siblings


class BootstrapSiblingsTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name) / "integration"
        self.root.mkdir()
        shutil.copy2(
            Path(__file__).resolve().parent.parent / "repos.lock.toml",
            self.root / "repos.lock.toml",
        )

    def tearDown(self):
        self.temporary.cleanup()

    def test_loads_only_required_exact_siblings(self):
        siblings = bootstrap_siblings.load_siblings(self.root)
        self.assertEqual([sibling.id for sibling in siblings], list(bootstrap_siblings.REQUIRED))
        self.assertTrue(all(sibling.destination.parent == self.root.parent for sibling in siblings))
        self.assertTrue(all(sibling.url.startswith("https://") for sibling in siblings))
        self.assertTrue(all(bootstrap_siblings.SHA_RE.fullmatch(sibling.sha) for sibling in siblings))

    def test_rejects_unsafe_lock_values(self):
        lock_path = self.root / "repos.lock.toml"
        original = lock_path.read_text()
        sha = bootstrap_siblings.load_siblings(self.root)[0].sha
        cases = (
            (
                'id = "opui"\npath = "../opui"',
                'id = "opui"\npath = "../../opui"',
                "unexpected destination",
            ),
            (f'sha = "{sha}"', 'sha = "deadbeef"', "invalid SHA"),
            (
                'url = "https://github.com/caniko/opui.git"',
                'url = "http://github.com/caniko/opui.git"',
                "rejected repository URL",
            ),
        )
        for old, new, message in cases:
            with self.subTest(message=message):
                lock_path.write_text(original.replace(old, new, 1))
                with self.assertRaisesRegex(RuntimeError, message):
                    bootstrap_siblings.load_siblings(self.root)
        lock_path.write_text(original)

    def test_rejects_existing_destination(self):
        destination = self.root.parent / "opui"
        destination.mkdir()
        with self.assertRaisesRegex(RuntimeError, "destination already exists"):
            bootstrap_siblings.require_absent(destination)

    def test_rejects_symlink_destination(self):
        target = self.root.parent / "target"
        target.mkdir()
        destination = self.root.parent / "opui"
        destination.symlink_to(target, target_is_directory=True)
        with self.assertRaisesRegex(RuntimeError, "destination already exists"):
            bootstrap_siblings.require_absent(destination)

    def test_git_environment_drops_credentials_and_overrides(self):
        inherited = {
            "PATH": os.environ["PATH"],
            "GH_TOKEN": "secret",
            "GIT_CONFIG_PARAMETERS": "'credential.helper=store'",
            "GIT_TEMPLATE_DIR": "/tmp/hooks",
            "SSH_AUTH_SOCK": "/tmp/agent",
        }
        with mock.patch.dict(os.environ, inherited, clear=True):
            env = bootstrap_siblings.git_env()
        self.assertEqual(env["PATH"], inherited["PATH"])
        self.assertEqual(env["GIT_CONFIG_GLOBAL"], "/dev/null")
        self.assertNotIn("GH_TOKEN", env)
        self.assertNotIn("GIT_TEMPLATE_DIR", env)
        self.assertNotIn("SSH_AUTH_SOCK", env)


if __name__ == "__main__":
    unittest.main()
