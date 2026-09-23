#!/usr/bin/env bash
# Creates a small but realistic demo repository (releases, features, hotfixes, a remote) for
# screenshots and manual testing: scripts/make-demo-repo.sh /tmp/gitgraph-demo
set -euo pipefail
dir=${1:-/tmp/gitgraph-demo}
rm -rf "$dir" "$dir-origin.git"
git init -q -b main "$dir"
cd "$dir"
git config user.name "Demo"
git config user.email "demo@example.com"
t=1700000000
c() { t=$((t + 3600)); GIT_AUTHOR_DATE="$t +0000" GIT_COMMITTER_DATE="$t +0000" git commit -q --allow-empty -m "$1"; }
m() { t=$((t + 3600)); GIT_AUTHOR_DATE="$t +0000" GIT_COMMITTER_DATE="$t +0000" git merge -q --no-ff -m "$2" "$1"; }

echo demo > README && git add README && c "Initial commit"
c "Project skeleton"
git tag -a v0.1.0 -m "First release" && c "Build pipeline"
git switch -q -c feature/login && c "Login form" && c "Password reset"
git switch -q main && c "Logging"
git switch -q -c feature/search && c "Search index" && c "Search UI"
git switch -q main && m feature/login "Merge feature/login"
git tag -a v0.2.0 -m "Login"
git switch -q -c release/0.2 && c "Release notes" && git tag v0.2.1
git switch -q main && m feature/search "Merge feature/search"
git switch -q -c feature/reports && c "Report engine"
git switch -q release/0.2 && c "Fix crash on empty search" && git tag v0.2.2
git switch -q main && m release/0.2 "Merge release/0.2 fixes"
git switch -q -c feature/dark-mode && c "Dark palette" && c "Theme switcher"
git switch -q main && c "Upgrade dependencies"
git tag -a v0.3.0 -m "Search"
git switch -q feature/reports && c "Export to PDF"
git switch -q -c experiment/charts && c "Chart prototype"
git switch -q main && c "Refactor settings"
git switch -q -c hotfix/token-expiry && c "Fix token expiry"
git switch -q main && m hotfix/token-expiry "Merge hotfix/token-expiry"
git branch -q -d hotfix/token-expiry

git clone -q --bare "$dir" "$dir-origin.git"
git remote add origin "$dir-origin.git"
git fetch -q origin
git branch -q -u origin/main main
c "Work in progress on main"
git switch -q feature/dark-mode && c "Contrast tweaks"
git switch -q main
echo "Demo repository in $dir"
