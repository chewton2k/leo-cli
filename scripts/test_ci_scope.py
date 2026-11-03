import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
FLAGS = ('rust', 'browser', 'node', 'installers', 'go_runner', 'java_runner', 'rust_runner')


class ScopeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = Path(self.tmp.name)
        self.env = dict(os.environ, GIT_AUTHOR_NAME='ci-test', GIT_AUTHOR_EMAIL='ci@example.com',
                        GIT_COMMITTER_NAME='ci-test', GIT_COMMITTER_EMAIL='ci@example.com')
        self.git('init', '-q')
        self.write('README.md')
        self.commit()
        self.base = self.git('rev-parse', 'HEAD').strip()

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.repo, env=self.env,
                                       stderr=subprocess.STDOUT, text=True, timeout=10)

    def write(self, name):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('changed\n')

    def commit(self):
        self.git('add', '.')
        self.git('commit', '-qm', 'fixture')

    def scope(self, *paths, base=None):
        for path in paths:
            self.write(path)
        if paths:
            self.commit()
        output = subprocess.check_output(['sh', str(ROOT / 'scripts/ci-scope.sh'),
                                          self.base if base is None else base],
                                         cwd=self.repo, text=True, timeout=10)
        return dict(line.split('=', 1) for line in output.splitlines())

    def assert_scope(self, result, *enabled):
        self.assertEqual(set(result), {*FLAGS, 'systems'})
        for flag in FLAGS:
            self.assertEqual(result[flag], str(flag in enabled).lower(), flag)
        expected = ['macos-latest', 'windows-latest'] if 'rust' in enabled else []
        self.assertEqual(json.loads(result['systems']), expected)

    def test_docs_and_empty_diffs_need_no_expensive_jobs(self):
        self.assert_scope(self.scope())
        self.assert_scope(self.scope('guide.md', 'docs/design.txt', 'LICENSE'))

    def test_page_changes_run_node_and_both_browser_suites(self):
        self.assert_scope(self.scope('crates/leo-web/src/web/app/notes.js'), 'node', 'browser')

    def test_browser_specs_do_not_require_rust_unit_tests(self):
        self.assert_scope(self.scope('crates/leo-web/tests/browser/flows.spec.js'), 'browser')

    def test_node_specs_do_not_require_rust_or_browsers(self):
        self.assert_scope(self.scope('crates/leo-web/tests/saving.test.js'), 'node')

    def test_shared_rust_changes_keep_cross_platform_and_browser_coverage(self):
        self.assert_scope(self.scope('crates/leo-core/src/store.rs'), 'rust', 'browser')

    def test_desktop_and_cli_changes_keep_cross_platform_coverage(self):
        self.assert_scope(self.scope('crates/leo-desktop/src/lib.rs', 'src/main.rs'), 'rust')

    def test_rust_integration_tests_run_without_browsers(self):
        self.assert_scope(self.scope('tests/e2e.rs', 'crates/leo-web/tests/markdown.rs'), 'rust')

    def test_installer_changes_run_their_own_checks(self):
        self.assert_scope(self.scope('scripts/install-app.ps1', 'scripts/test_desktop_release.py'), 'rust', 'installers')

    def test_runner_changes_select_only_the_matching_build_check_and_consumers(self):
        for name in ('go', 'java', 'rust'):
            with self.subTest(name=name):
                result = self.scope(f'scripts/{name}-runner/MANIFEST')
                self.assert_scope(result, 'rust', 'browser', f'{name}_runner')
                self.base = self.git('rev-parse', 'HEAD').strip()

    def test_runtime_catalog_changes_check_all_runner_pins(self):
        self.assert_scope(self.scope('crates/leo-services/src/runtimes.rs'),
                          'rust', 'browser', 'go_runner', 'java_runner', 'rust_runner')

    def test_unknown_inputs_and_workflow_changes_fail_open_to_full_coverage(self):
        for path in ('Cargo.lock', '.cargo/config.toml', '.github/workflows/ci.yml',
                     'scripts/ci-scope.sh', 'new-build-input.txt'):
            with self.subTest(path=path):
                self.assert_scope(self.scope(path), *FLAGS)
                self.base = self.git('rev-parse', 'HEAD').strip()

    def test_missing_and_invalid_bases_run_everything(self):
        self.assert_scope(self.scope(base=''), *FLAGS)
        self.assert_scope(self.scope(base='does-not-exist'), *FLAGS)

    def test_mixed_changes_union_the_checks(self):
        self.assert_scope(self.scope('scripts/install-app.sh', 'crates/leo-web/tests/saving.test.js'),
                          'rust', 'installers', 'node')

    def test_failed_or_cancelled_intermediate_changes_remain_in_scope(self):
        self.write('crates/leo-core/src/store.rs')
        self.commit()
        self.assert_scope(self.scope('new-guide.md'), 'rust', 'browser')

    def test_deletions_and_renames_keep_both_sides_in_scope(self):
        self.write('crates/leo-core/src/old.rs')
        self.commit()
        self.base = self.git('rev-parse', 'HEAD').strip()
        self.git('mv', 'crates/leo-core/src/old.rs', 'guide.md')
        self.commit()
        self.assert_scope(self.scope(), 'rust', 'browser')

    def test_deleted_sources_still_select_their_checks(self):
        self.write('crates/leo-core/src/old.rs')
        self.commit()
        self.base = self.git('rev-parse', 'HEAD').strip()
        self.git('rm', 'crates/leo-core/src/old.rs')
        self.commit()
        self.assert_scope(self.scope(), 'rust', 'browser')

    def test_mixed_scopes_are_independent_of_path_order(self):
        for paths in [('scripts/go-runner/COMMIT', 'scripts/java-runner/COMMIT'),
                      ('scripts/java-runner/FILES', 'scripts/go-runner/FILES')]:
            self.assert_scope(self.scope(*paths), 'rust', 'browser', 'go_runner', 'java_runner')
            self.base = self.git('rev-parse', 'HEAD').strip()


if __name__ == '__main__':
    unittest.main()
