#!/bin/sh
# Fix the cargo-geiger step; push. POSIX sh.
set -e
cd "$(dirname "$0")/.."

echo "=== YAML sanity ==="
env -u LD_LIBRARY_PATH python3 -c "
import yaml
d=yaml.safe_load(open('.github/workflows/ci.yml'))
print('jobs:', list(d['jobs']))
steps=[s.get('name') or s.get('uses') for s in d['jobs']['security-audit']['steps']]
print('security-audit steps:'); [print('   ', s) for s in steps]
"
echo "=== no --output-format json left anywhere? ==="
grep -rn "output-format json\|geiger-report.json" .github/workflows/ || echo "none"

TOKEN=$(grep -o 'ghp_[A-Za-z0-9]\{20,\}' ~/.hermes/.env | head -1)
[ -n "$TOKEN" ] || { echo "token not found"; exit 1; }

MSG="$HOME/.hermes/cache/scratch/commitmsg"
mkdir -p "$MSG"
cat > "$MSG/16.txt" <<'EOF'
fix(ci): cargo-geiger has no json output; stop asking for one

With the advisories gate satisfied, CI run 41770082973 got one step further and
failed at `Check for unsafe code` (step 8):

    thread 'main' panicked at cargo-geiger-0.14.0/src/main.rs:121:67:
    called `Result::unwrap()` on an `Err` value: Utf8ArgumentParsingFailed {
      value: "json", cause: "Matching variant not found" }
    ##[error]Process completed with exit code 101.

cargo-geiger has **no** JSON output. In 0.14 the flag accepts exactly three
values — `Ascii`, `GitHubMarkdown`, `Utf8` (the default) — and its own help text
says so:

    --output-format    Output format for the report: Ascii, GitHubMarkdown, ...

So the step was written against a flag that never existed, and `|| true` hid both
of its failure modes: first `command not found` (cargo-geiger was not installed),
then this panic once it was.

The step is now honest about what it is: a **report**, not a gate.

- runs with the supported `--output-format Ascii`
- writes `geiger-report.txt` and prints it
- the upload artifact path follows the rename

It is deliberately not a gate. An unsafe-code count is a census, and this workspace
legitimately uses `unsafe` for libc calls (`libc::flock`, `fcntl`, `ioctl`), so any
threshold would be an arbitrary number dressed up as a policy. Making it a report
that is visible and comparable across runs is the honest version.

Also checked the rest of the job for the same class of defect (commands that only
look installed, flags that do not exist): the remaining `|| true` occurrences are
legitimate — optional `cp` of files that may not exist in a given matrix cell, and
fuzz targets that are best-effort under a timeout.
EOF

git add .github/workflows/ci.yml
git commit -F "$MSG/16.txt"

echo "=== PUSH ==="
git push "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git ls-remote "https://michaelamoetoevsky:${TOKEN}@github.com/michaelamoetoevsky/PDN.git" master | cut -f1)
echo "local : $LOCAL"
echo "remote: $REMOTE"
[ "$LOCAL" = "$REMOTE" ] && echo "PUSH CONFIRMED" || echo "MISMATCH"
