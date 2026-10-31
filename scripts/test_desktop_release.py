import base64
import json
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class ReleaseTests(unittest.TestCase):
    def test_updater_public_key_is_real_and_matches_configuration(self):
        key = (ROOT / 'crates/leo-desktop/updater-public-key.txt').read_text().strip()
        decoded = base64.b64decode(key, validate=True)
        self.assertIn(b'untrusted comment:', decoded)
        config = json.loads((ROOT / 'crates/leo-desktop/tauri.conf.json').read_text())
        self.assertEqual(config['plugins']['updater']['pubkey'], key)
        self.assertNotIn('version', config)

    def test_assets_and_manifest_match_all_three_targets(self):
        import tempfile
        import desktop_release
        with tempfile.TemporaryDirectory() as folder:
            folder = pathlib.Path(folder)
            for target, suffix in [('aarch64-apple-darwin', '.app.tar.gz'), ('x86_64-apple-darwin', '.app.tar.gz'), ('x86_64-pc-windows-msvc', '-setup.exe')]:
                name = 'leo-' + target + suffix
                (folder / name).write_bytes(b'archive')
                (folder / (name + '.sig')).write_text('signature-' + target)
            manifest = desktop_release.manifest(folder, '1.2.3', 'What changed', '2026-10-31T00:00:00Z')
            self.assertEqual(set(manifest['platforms']), {'darwin-aarch64', 'darwin-x86_64', 'windows-x86_64'})
            self.assertEqual(manifest['version'], '1.2.3')
            self.assertIn('/v1.2.3/', manifest['platforms']['darwin-aarch64']['url'])
            self.assertTrue(manifest['platforms']['windows-x86_64']['url'].endswith('-setup.exe'))
            self.assertEqual(manifest['platforms']['darwin-x86_64']['signature'], 'signature-x86_64-apple-darwin')
            (folder / 'leo-aarch64-apple-darwin.app.tar.gz.sig').unlink()
            with self.assertRaises(ValueError):
                desktop_release.manifest(folder, '1.2.3', '', '2026-10-31T00:00:00Z')

    def test_packaging_requires_exactly_one_signed_artifact(self):
        import tempfile
        import desktop_release
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            source, output = root / 'bundle', root / 'out'
            (source / 'macos').mkdir(parents=True)
            (source / 'dmg').mkdir()
            (source / 'macos/Leo.app.tar.gz').write_bytes(b'app')
            (source / 'dmg/Leo_1.2.3_aarch64.dmg').write_bytes(b'dmg')
            with self.assertRaises(ValueError):
                desktop_release.package(source, output, 'aarch64-apple-darwin')
            (source / 'macos/Leo.app.tar.gz.sig').write_text('signature')
            desktop_release.package(source, output, 'aarch64-apple-darwin')
            self.assertEqual((output / 'leo-aarch64-apple-darwin.app.tar.gz').read_bytes(), b'app')
            self.assertTrue((output / 'leo-aarch64-apple-darwin.dmg.sha256').is_file())
            (source / 'macos/Other.app.tar.gz').write_bytes(b'bad')
            with self.assertRaises(ValueError):
                desktop_release.package(source, output, 'aarch64-apple-darwin')

    def test_release_configuration_ships_cli_and_signed_updater_artifacts(self):
        config = json.loads((ROOT / 'crates/leo-desktop/tauri.conf.json').read_text())
        release = json.loads((ROOT / 'crates/leo-desktop/tauri.release.conf.json').read_text())
        self.assertTrue(config['bundle']['createUpdaterArtifacts'])
        self.assertEqual(release['bundle']['externalBin'], ['binaries/leo'])
        self.assertEqual(config['bundle']['windows']['nsis']['installMode'], 'currentUser')

    def test_workflow_builds_only_supported_signed_apps(self):
        workflow = (ROOT / '.github/workflows/release.yml').read_text()
        build = workflow.split('  build:\n', 1)[1].split('  go-runner:', 1)[0]
        for target in ['aarch64-apple-darwin', 'x86_64-apple-darwin', 'x86_64-pc-windows-msvc']:
            self.assertIn(target, build)
        self.assertNotIn('linux', build)
        self.assertIn('tauri build --ci', build)
        self.assertIn('codesign --verify --deep --strict', build)
        self.assertIn('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', build)
        self.assertIn('desktop_release.py manifest', workflow)
        self.assertIn('secrets: inherit', (ROOT / '.github/workflows/ci.yml').read_text())


if __name__ == '__main__':
    unittest.main()
