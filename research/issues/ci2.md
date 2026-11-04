## Summary

`dependency-updates.yml` referenced an action that **does not exist**:
`peter-evans/update-github-actions@v1`. `gh api repos/peter-evans/update-github-actions`
returns `404 Not Found`, and the owner's repository list contains no such project. The job
`github-actions-dependencies` therefore could never have succeeded — and the broken pin went
unnoticed because the scheduled trigger skips that job (`github.event.inputs.dependency-type`
is empty on a `schedule` event), so it was never executed.

## Reproduction / evidence

```
$ env -u LD_LIBRARY_PATH gh api repos/peter-evans/update-github-actions
{"message":"Not Found","documentation_url":"https://docs.github.com/rest/repos/repos#get-a-repository","status":404}

$ env -u LD_LIBRARY_PATH gh api "users/peter-evans/repos?per_page=100" --jq '.[].name' \
    | grep -i 'action\|update'
create-or-update-comment
create-or-update-project-card
approve-pull-request-action
create-or-update-comment
gradle-auto-dependency-updates
...
# no `update-github-actions`
```

Recorded in `research/CI_CD_AUDIT.md` as an unresolvable third-party reference; confirmed
independently by querying the REST API for every pin in every workflow (14 distinct pins).

## Root cause

The pin was added when the workflow first landed (commit `950a927`) and never exercised,
because the only automatic trigger (weekly `schedule`) leaves the input that gates the job
unset, and the job is `skipped` on every scheduled run — see run `41424692889`, where
`Update GitHub Actions Dependencies` shows `skipped` while `Update Rust Dependencies` failed.

## Impact

- The job is dead code: it cannot run, so GitHub Actions versions are never updated.
- Worse, it is a *silent* failure mode: an unresolvable action in any job only surfaces when
  that job runs, which may be never.

## Fix

Replaced the third-party updater with a self-contained audit step that depends on nothing
but `gh` (already present on the runner):

- enumerates every `uses:` pin across `.github/workflows/*.yml`;
- resolves each via `gh api repos/<owner>/<repo>/releases/latest`, falling back to the tag
  list for actions that publish no releases (branch-ref pins);
- writes a summary table to `$GITHUB_STEP_SUMMARY`;
- **fails the job when any pin cannot be resolved**, so a nonexistent action is caught here
  rather than silently breaking whichever job references it.

Resolving by exit status, not by output, matters: `gh api` prints its error body on stdout,
so an emptiness check treats a 404 as a resolved version. (The first version of this step had
exactly that bug; it was caught by running it against the previous revision.)

Verified both directions before committing:

```
A) PREVIOUS revision:
  peter-evans/update-github-actions  pinned=v1  latest=-   UNRESOLVABLE
  -> unresolved=1
  -> VERDICT: FAIL (exit 1)          # the gate catches it

B) CURRENT revision:
  -> unresolved=0
  -> VERDICT: pass (exit 0)
```

All 14 remaining pins were verified to exist against the REST API; the pin bump to modern
Node-24-compatible majors was done only where a release actually exists.

Labels: bug, ci, github-actions
