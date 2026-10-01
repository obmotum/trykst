#!/usr/bin/env python3
"""End-to-end smoke test for a running Trykst server.

Signs in through the real OIDC flow (Trykst -> Keycloak login form -> callback)
like a browser would, then exercises the main API endpoints. Expects the dev
Keycloak from docker-compose.dev.yml and a server configured as described in
its header. Uses only the standard library so it runs on any CI runner.

    python tests/e2e.py [--base http://localhost:3000] [--user alice --password alice]
"""

import argparse
import html
import http.cookiejar
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

parser = argparse.ArgumentParser()
parser.add_argument("--base", default="http://localhost:3000")
parser.add_argument("--user", default="alice")
parser.add_argument("--password", default="alice")
args = parser.parse_args()

jar = http.cookiejar.CookieJar()
opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
failures = []


def request(method, url, body=None, form=None):
    data, headers = None, {}
    if body is not None:
        data, headers = json.dumps(body).encode(), {"Content-Type": "application/json"}
    elif form is not None:
        data = urllib.parse.urlencode(form).encode()
        headers = {"Content-Type": "application/x-www-form-urlencoded"}
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    # Keycloak marks its cookies Secure; browsers still send them to http://localhost,
    # urllib does not. The test only ever talks to local services.
    for cookie in jar:
        cookie.secure = False
    try:
        with opener.open(req, timeout=60) as resp:
            return resp.status, resp.geturl(), resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as err:
        return err.code, err.geturl(), err.read().decode("utf-8", "replace")


def login_form(page):
    """Returns (action, fields) of Keycloak's login form."""
    form = re.search(r'<form[^>]*id="kc-form-login"[^>]*>(.*?)</form>', page, re.S)
    if not form:
        return None, None
    action = html.unescape(re.search(r'action="([^"]+)"', form.group(0)).group(1))
    fields = {}
    for tag in re.findall(r"<input[^>]*>", form.group(1)):
        name = re.search(r'name="([^"]+)"', tag)
        if name:
            value = re.search(r'value="([^"]*)"', tag)
            fields[name.group(1)] = html.unescape(value.group(1)) if value else ""
    return action, fields


def sign_in():
    status, url, page = request("GET", f"{args.base}/api/auth/oidc/login?return_to=/api/auth/me")
    # Keycloak may ask for the username and the password on separate pages
    # (identifier-first login when Organizations are enabled).
    for _ in range(3):
        action, fields = login_form(page)
        if action is None:
            break
        if "username" in fields:
            fields["username"] = args.user
        if "password" in fields:
            fields["password"] = args.password
        status, url, page = request("POST", action, form=fields)
    if status != 200 or not url.endswith("/api/auth/me"):
        sys.exit(f"Sign-in failed: HTTP {status} at {url}\n{page[:500]}")
    return json.loads(page)


def check(name, method, path, body=None, expect=(200, 201, 204)):
    status, _, text = request(method, args.base + path, body=body)
    ok = status in expect
    print(f"{'ok  ' if ok else 'FAIL'} {status} {name}" + ("" if ok else f": {text[:200]}"))
    if not ok:
        failures.append(name)
    try:
        return json.loads(text) if text else None
    except json.JSONDecodeError:
        return None


me = sign_in()
print(f"ok   signed in as {me['username']} (admin={me['is_admin']}, guest={me['is_guest']})")

folder = check("create folder", "POST", "/api/folders", {"name": "E2E folder"}) or {}
check("list folders", "GET", "/api/folders")
doc = check("create document", "POST", "/api/docs",
            {"title": "E2E", "folder_id": folder.get("id"), "content": "= Hello"}) or {}
doc_id = doc.get("id")
check("list documents", "GET", "/api/docs")
check("get document", "GET", f"/api/docs/{doc_id}")
check("update document", "PATCH", f"/api/docs/{doc_id}", {"title": "E2E 2", "public_role": "viewer"})
people = check("directory search", "GET", "/api/directory/search?q=van") or []
subject = next((p["subject"] for p in people if p.get("subject")), None)
if subject:
    check("invite by directory subject", "POST", f"/api/docs/{doc_id}/invite",
          {"subject": subject, "role": "editor"})
else:
    failures.append("directory search returned no subject")
    print("FAIL directory search returned no subject")
check("list collaborators", "GET", f"/api/docs/{doc_id}/collaborators")
comment = check("add comment", "POST", f"/api/docs/{doc_id}/comments", {"content": "First"}) or {}
check("resolve comment", "PATCH", f"/api/comments/{comment.get('id')}", {"resolved": True})
comments = check("list comments", "GET", f"/api/docs/{doc_id}/comments") or [{}]
if comments[0].get("resolved") is not True:
    failures.append("comment not resolved")
    print("FAIL comment resolved flag not persisted")
check("create version", "POST", f"/api/docs/{doc_id}/versions", {"content": "= V1"})
check("list versions", "GET", f"/api/docs/{doc_id}/versions")
check("compile", "POST", "/api/compile", {"text": "= Hello", "document_id": doc_id})
space = check("create space", "POST", "/api/spaces", {"name": "E2E space"}) or {}
space_id = space.get("id")
check("add space file", "POST", f"/api/spaces/{space_id}/files",
      {"path": "refs.bib", "kind": "text", "content": "@book{x}"})
check("list space files", "GET", f"/api/spaces/{space_id}/files")
# Package versions are immutable; a unique version keeps the test repeatable on a reused database.
check("publish package", "POST", "/api/packages/publish", {"space_id": space_id, "version": f"0.0.{int(time.time())}"})
check("list packages", "GET", "/api/packages")
check("create API key", "POST", "/api/keys", {"name": "e2e"})
check("list API keys", "GET", "/api/keys")
check("API key usage", "GET", "/api/keys/usage?period=1week")
check("storage stats", "GET", "/api/auth/storage")
check("admin user list", "GET", "/api/admin/users")
check("delete comment", "DELETE", f"/api/comments/{comment.get('id')}")
check("delete space", "DELETE", f"/api/spaces/{space_id}")
check("delete document", "DELETE", f"/api/docs/{doc_id}")
check("logout", "POST", "/api/auth/logout")
check("session gone after logout", "GET", "/api/auth/me", expect=(401,))

if failures:
    sys.exit(f"\n{len(failures)} check(s) failed: {', '.join(failures)}")
print("\nAll checks passed.")
