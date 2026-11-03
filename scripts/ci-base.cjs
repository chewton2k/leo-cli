module.exports = async ({ github, context, core, exec }) => {
  let base = '';
  if (context.eventName === 'pull_request') {
    base = context.payload.pull_request.base.sha;
  } else if (context.eventName === 'push') {
    try {
      const { data } = await github.rest.actions.listWorkflowRuns({
        ...context.repo,
        workflow_id: 'ci.yml',
        branch: context.ref.replace(/^refs\/heads\//, ''),
        event: 'push',
        status: 'success',
        per_page: 100,
        request: { timeout: 10000 },
      });
      for (const run of data.workflow_runs.slice(0, 100)) {
        if (run.conclusion !== 'success' || !/^[a-f0-9]{40}$/.test(run.head_sha)) continue;
        const result = await exec.getExecOutput('git', ['merge-base', '--is-ancestor', run.head_sha, 'HEAD'],
          { ignoreReturnCode: true, silent: true });
        if (result.exitCode === 0) {
          base = run.head_sha;
          break;
        }
      }
    } catch {
      core.warning('Could not read successful CI history; using the last release or testing everything.');
    }
    const release = await exec.getExecOutput('git', ['describe', '--tags', '--abbrev=0', '--match', 'v[0-9]*', 'HEAD'],
      { ignoreReturnCode: true, silent: true });
    if (release.exitCode === 0) {
      const tag = release.stdout.trim();
      if (!base) {
        base = tag;
      } else {
        const result = await exec.getExecOutput('git', ['merge-base', '--is-ancestor', base, tag],
          { ignoreReturnCode: true, silent: true });
        if (result.exitCode === 0) base = tag;
      }
    }
  }
  core.info(`Compared with ${base || 'nothing (testing everything)'}`);
  core.setOutput('base', base);
};
