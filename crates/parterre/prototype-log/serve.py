#!/usr/bin/env python3
"""PROTOTYPE, throwaway: serves the log-window prototype with live data from a git repository.

    python3 crates/parterre/prototype-log/serve.py [REPO] [--port 8766]

then open http://127.0.0.1:8766/ . REPO defaults to the current directory. Nothing is written
to disk: the page asks this server, which asks git. Read-only.
"""

import argparse
import http.server
import json
import os
import subprocess
import sys
import urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))


def git(repo, *args):
    return subprocess.run(
        ["git", "-C", repo, *args], check=True, capture_output=True
    ).stdout.decode("utf-8", "replace")


def refs(repo):
    """Refs by commit hash: [{name, full, kind}], plus the current branch."""
    head = git(repo, "symbolic-ref", "-q", "HEAD").strip() if subprocess.run(
        ["git", "-C", repo, "symbolic-ref", "-q", "HEAD"], capture_output=True
    ).returncode == 0 else ""
    out = git(repo, "for-each-ref", "--format=%(refname)%00%(objectname)%00%(*objectname)")
    by_commit = {}
    for line in out.splitlines():
        full, obj, peeled = line.split("\0")
        target = peeled or obj
        if full.startswith("refs/heads/"):
            kind, name = ("current" if full == head else "local"), full[len("refs/heads/"):]
        elif full.startswith("refs/remotes/"):
            if full.endswith("/HEAD"):
                continue
            kind, name = "remote", full[len("refs/remotes/"):]
        elif full.startswith("refs/tags/"):
            kind, name = "tag", full[len("refs/tags/"):]
        else:
            continue
        by_commit.setdefault(target, []).append({"name": name, "full": full, "kind": kind})
    order = {"current": 0, "local": 1, "remote": 2, "tag": 3}
    for rs in by_commit.values():
        rs.sort(key=lambda r: (order[r["kind"]], r["name"]))
    return by_commit


def log(repo, spec):
    fmt = "%H%x00%an%x00%ae%x00%ad%x00%s%x1e"
    out = git(repo, "log", "--date-order", "--date=format-local:%Y-%m-%d %H:%M",
              f"--format={fmt}", *spec.split(), "--")
    rows = []
    for rec in out.split("\x1e"):
        rec = rec.strip("\n")
        if not rec:
            continue
        h, an, ae, ad, s = rec.split("\0")
        rows.append([h, an, ae, ad, s])
    return rows


def message(repo, h):
    return git(repo, "log", "-1", "--format=%B", h)


def files(repo, h):
    parents = git(repo, "rev-list", "--parents", "-n1", h).split()[1:]
    base = [parents[0], h] if parents else ["--root", h]
    status = git(repo, "diff-tree", "-r", "-M", "--no-commit-id", "--name-status", "-z", *base)
    numstat = git(repo, "diff-tree", "-r", "-M", "--no-commit-id", "--numstat", "-z", *base)
    rows = []
    t = status.split("\0")
    i = 0
    while i < len(t) and t[i]:
        st = t[i]
        if st[0] in "RC":
            rows.append({"status": st[0], "old": t[i + 1], "path": t[i + 2]})
            i += 3
        else:
            rows.append({"status": st[0], "path": t[i + 1]})
            i += 2
    counts = {}
    t = numstat.split("\0")
    i = 0
    while i < len(t) and t[i]:
        added, removed, path = t[i].split("\t")
        if path == "":  # rename: old and new follow
            path = t[i + 2]
            i += 3
        else:
            i += 1
        counts[path] = (added, removed)
    for r in rows:
        a, d = counts.get(r["path"], ("-", "-"))
        r["added"], r["removed"] = a, d
    return {"parents": len(parents), "files": rows}


class Handler(http.server.SimpleHTTPRequestHandler):
    repo = "."

    def __init__(self, *a, **kw):
        super().__init__(*a, directory=HERE, **kw)

    def log_message(self, *a):
        pass

    def send_json(self, obj):
        body = json.dumps(obj, separators=(",", ":")).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        u = urllib.parse.urlparse(self.path)
        q = {k: v[0] for k, v in urllib.parse.parse_qs(u.query).items()}
        try:
            if u.path == "/api/repo":
                name = os.path.basename(os.path.abspath(git(self.repo, "rev-parse", "--show-toplevel").strip()))
                return self.send_json({"name": name, "refs": refs(self.repo)})
            if u.path == "/api/log":
                return self.send_json(log(self.repo, q["spec"]))
            if u.path == "/api/message":
                return self.send_json(message(self.repo, q["h"]))
            if u.path == "/api/files":
                return self.send_json(files(self.repo, q["h"]))
            if u.path == "/api/resolve":
                return self.send_json(git(self.repo, "rev-parse", q["rev"] + "^{commit}").strip())
            if u.path == "/api/is-ancestor":
                r = subprocess.run(["git", "-C", self.repo, "merge-base", "--is-ancestor", q["a"], q["b"]])
                return self.send_json(r.returncode == 0)
        except subprocess.CalledProcessError as e:
            self.send_response(500)
            self.end_headers()
            self.wfile.write(e.stderr)
            return
        return super().do_GET()


def main():
    p = argparse.ArgumentParser()
    p.add_argument("repo", nargs="?", default=".")
    p.add_argument("--port", type=int, default=8766)
    a = p.parse_args()
    Handler.repo = a.repo
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", a.port), Handler)
    print(f"PROTOTYPE log window for {os.path.abspath(a.repo)}: http://127.0.0.1:{a.port}/", file=sys.stderr)
    srv.serve_forever()


if __name__ == "__main__":
    main()
