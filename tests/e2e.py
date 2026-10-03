#!/usr/bin/env python3
"""End-to-end test for a running Trykst server.

Signs in through the real OIDC flow (Trykst -> Keycloak login form -> callback)
like a browser would, then exercises projects, documents with nested folders and
files, sharing and permissions. Expects the dev Keycloak from
docker-compose.dev.yml and a server configured as described in its header:
alice is an admin, bob a regular member and erik a guest (Partner AG).
Uses only the standard library so it runs on any CI runner.

    python tests/e2e.py [--base http://localhost:3000]
"""

import argparse
import base64
import html
import http.cookiejar
import json
import re
import struct
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import zlib

parser = argparse.ArgumentParser()
parser.add_argument("--base", default="http://localhost:3000")
args = parser.parse_args()

failures = []


def tiny_png():
    """A valid 1x1 PNG, so Typst can actually decode the image."""
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))
    ihdr = struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(b"\x00\xff\x00\x00")) + chunk(b"IEND", b""))


class Session:
    """One signed-in browser: its own cookie jar."""

    def __init__(self, user):
        self.user = user
        self.jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar))
        self.me = self.sign_in()
        print(f"ok   signed in as {self.me['username']} "
              f"(admin={self.me['is_admin']}, guest={self.me['is_guest']})")

    def raw(self, method, url, data=None, headers=None):
        req = urllib.request.Request(url, data=data, headers=headers or {}, method=method)
        # Keycloak marks its cookies Secure; browsers still send them to
        # http://localhost, urllib does not. The test only talks to local services.
        for cookie in self.jar:
            cookie.secure = False
        try:
            with self.opener.open(req, timeout=60) as resp:
                return resp.status, resp.geturl(), resp.read()
        except urllib.error.HTTPError as err:
            return err.code, err.geturl(), err.read()

    def sign_in(self):
        status, url, page = self.raw("GET", f"{args.base}/api/auth/oidc/login?return_to=/api/auth/me")
        page = page.decode("utf-8", "replace")
        # Keycloak may ask for the username and the password on separate pages
        # (identifier-first login when Organizations are enabled).
        for _ in range(3):
            action, fields = login_form(page)
            if action is None:
                break
            if "username" in fields:
                fields["username"] = self.user
            if "password" in fields:
                fields["password"] = self.user
            data = urllib.parse.urlencode(fields).encode()
            status, url, page = self.raw("POST", action, data,
                                         {"Content-Type": "application/x-www-form-urlencoded"})
            page = page.decode("utf-8", "replace")
        if status != 200 or not url.endswith("/api/auth/me"):
            sys.exit(f"Sign-in of {self.user} failed: HTTP {status} at {url}\n{page[:500]}")
        return json.loads(page)

    def check(self, name, method, path, body=None, expect=(200, 201, 204), files=None, fields=None, raw=None):
        if raw is not None:
            data, headers = raw, {"Content-Type": "image/svg+xml"}
        elif files is not None:
            data, headers = multipart(fields or {}, files)
        elif body is not None:
            data, headers = json.dumps(body).encode(), {"Content-Type": "application/json"}
        else:
            data, headers = None, {}
        status, _, raw = self.raw(method, args.base + path, data, headers)
        text = raw.decode("utf-8", "replace")
        ok = status in expect
        label = f"[{self.user}] {name}"
        print(f"{'ok  ' if ok else 'FAIL'} {status} {label}" + ("" if ok else f": {text[:200]}"))
        if not ok:
            failures.append(label)
        if path.startswith("/api/export/") or path.endswith("/content"):
            return raw
        try:
            return json.loads(text) if text else None
        except json.JSONDecodeError:
            return None


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


def multipart(fields, files):
    boundary = uuid.uuid4().hex
    out = b""
    for name, value in fields.items():
        out += (f"--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n"
                f"{value}\r\n").encode()
    for filename, data in files:
        out += (f"--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; "
                f"filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").encode()
        out += data + b"\r\n"
    out += f"--{boundary}--\r\n".encode()
    return out, {"Content-Type": f"multipart/form-data; boundary={boundary}"}


def expect(condition, name):
    print(f"{'ok  ' if condition else 'FAIL'}     {name}")
    if not condition:
        failures.append(name)


def by_path(tree, path):
    return next((n for n in tree["nodes"] if n["path"] == path), None)


def compiles(session, doc_id, name):
    result = session.check(name, "POST", "/api/compile", {"document_id": doc_id}) or {}
    return bool(result.get("pages")) and not result.get("errors"), result


alice, bob, erik = Session("alice"), Session("bob"), Session("erik")

# --- Project and document -----------------------------------------------------------
project = alice.check("create project", "POST", "/api/projects", {"name": "E2E project"}) or {}
pid = project.get("id")
expect(project.get("role") == "owner", "creator is owner")
doc = alice.check("create document", "POST", f"/api/projects/{pid}/documents", {"title": "E2E"}) or {}
did = doc.get("id")
tree = alice.check("get tree", "GET", f"/api/documents/{did}/tree") or {"nodes": []}
main = by_path(tree, "main.typ") or {}
expect(tree.get("entrypoint_id") == main.get("id"), "new document has main.typ as entrypoint")

# --- Nested folders and files ---------------------------------------------------------
kapitel = alice.check("create folder", "POST", f"/api/documents/{did}/nodes",
                      {"name": "kapitel", "kind": "folder"}) or {}
alice.check("create nested text file", "POST", f"/api/documents/{did}/nodes",
            {"parent_id": kapitel.get("id"), "name": "einleitung.typ", "kind": "text",
             "content": "== Einleitung\nAus dem Unterordner."})
bilder = alice.check("create second folder", "POST", f"/api/documents/{did}/nodes",
                     {"name": "bilder", "kind": "folder"}) or {}
alice.check("upload image into folder", "POST", f"/api/documents/{did}/upload",
            fields={"parent_id": bilder.get("id")}, files=[("logo.png", tiny_png())])
alice.check("duplicate name is rejected", "POST", f"/api/documents/{did}/nodes",
            {"name": "kapitel", "kind": "folder"}, expect=(409,))
alice.check("invalid name is rejected", "POST", f"/api/documents/{did}/nodes",
            {"name": "../x.typ", "kind": "text"}, expect=(400,))
alice.check("write main.typ", "PATCH", f"/api/documents/{did}/nodes/{main.get('id')}",
            {"content": '#include "kapitel/einleitung.typ"\n#image("bilder/logo.png", width: 1cm)\n'})
content = alice.check("read nested file", "GET",
                      f"/api/documents/{did}/nodes/{by_path(alice.check('get tree', 'GET', f'/api/documents/{did}/tree'), 'kapitel/einleitung.typ')['id']}/content")
expect(b"Einleitung" in (content or b""), "nested file content round-trips")
ok, _ = compiles(alice, did, "compile with include and image")
expect(ok, "document with nested include and image compiles")

# Moving a folder changes the paths the document sees.
alice.check("move folder into folder", "PATCH", f"/api/documents/{did}/nodes/{bilder.get('id')}",
            {"parent_id": kapitel.get("id")})
tree = alice.check("get tree after move", "GET", f"/api/documents/{did}/tree") or {"nodes": []}
expect(by_path(tree, "kapitel/bilder/logo.png") is not None, "moved file has its new path")
ok, _ = compiles(alice, did, "compile after move (old path)")
expect(not ok, "old image path no longer resolves")
alice.check("update main.typ to new path", "PATCH", f"/api/documents/{did}/nodes/{main.get('id')}",
            {"content": '#include "kapitel/einleitung.typ"\n#image("kapitel/bilder/logo.png", width: 1cm)\n'})
ok, _ = compiles(alice, did, "compile after move (new path)")
expect(ok, "document compiles with the new path")
alice.check("folder cannot move into its own child", "PATCH",
            f"/api/documents/{did}/nodes/{kapitel.get('id')}", {"parent_id": bilder.get("id")}, expect=(400,))
logo = by_path(tree, "kapitel/bilder/logo.png") or {}
alice.check("image cannot be the entrypoint", "PUT", f"/api/documents/{did}/entrypoint",
            {"node_id": logo.get("id")}, expect=(400,))
pdf = alice.check("export PDF", "POST", "/api/export/pdf", {"document_id": did})
expect(isinstance(pdf, bytes) and pdf.startswith(b"%PDF"), "export returns a PDF")
alice.check("list fonts", "GET", f"/api/documents/{did}/fonts")
alice.check("browser sets the thumbnail", "PUT", f"/api/documents/{did}/thumbnail", raw=b'<svg xmlns="http://www.w3.org/2000/svg"></svg>')
alice.check("thumbnail must be an SVG", "PUT", f"/api/documents/{did}/thumbnail", raw=b"<script>x</script>", expect=(400,))

# --- Versions and comments ------------------------------------------------------------
version = alice.check("create version", "POST", f"/api/documents/{did}/versions", {"label": "v1"}) or {}
alice.check("list versions", "GET", f"/api/documents/{did}/versions")
alice.check("delete folder", "DELETE", f"/api/documents/{did}/nodes/{kapitel.get('id')}")
tree = alice.check("get tree after delete", "GET", f"/api/documents/{did}/tree") or {"nodes": []}
expect(by_path(tree, "kapitel/einleitung.typ") is None and by_path(tree, "kapitel/bilder/logo.png") is None,
       "deleting a folder removes its contents")
alice.check("restore version", "POST", f"/api/documents/{did}/versions/{version.get('id')}/restore")
tree = alice.check("get tree after restore", "GET", f"/api/documents/{did}/tree") or {"nodes": []}
expect(by_path(tree, "kapitel/bilder/logo.png") is not None, "restore brings back nested files")
ok, _ = compiles(alice, did, "compile after restore")
expect(ok, "restored document compiles")
main = by_path(tree, "main.typ") or {}
comment = alice.check("add comment on file", "POST", f"/api/documents/{did}/comments",
                      {"content": "First", "node_id": main.get("id")}) or {}
alice.check("resolve comment", "PATCH", f"/api/comments/{comment.get('id')}", {"resolved": True})
comments = alice.check("list comments", "GET", f"/api/documents/{did}/comments") or [{}]
expect(comments[0].get("resolved") is True, "comment resolved flag persisted")

# --- Packages ---------------------------------------------------------------------------
pkg_doc = alice.check("create package document", "POST", f"/api/projects/{pid}/documents", {"title": "Paket"}) or {}
pkg_did = pkg_doc.get("id")
alice.check("add typst.toml", "POST", f"/api/documents/{pkg_did}/nodes",
            {"name": "typst.toml", "kind": "text",
             "content": '[package]\nname = "gruss"\nversion = "0.1.0"\nentrypoint = "lib.typ"\n'})
alice.check("add lib.typ", "POST", f"/api/documents/{pkg_did}/nodes",
            {"name": "lib.typ", "kind": "text", "content": "#let gruss = [Hallo aus dem Paket]\n"})
alice.check("publish project package", "POST", "/api/packages/publish", {"document_id": pkg_did})
alice.check("version is immutable", "POST", "/api/packages/publish", {"document_id": pkg_did}, expect=(409,))
alice.check("list project packages", "GET", f"/api/projects/{pid}/packages")
alice.check("use project package", "PATCH", f"/api/documents/{did}/nodes/{main.get('id')}",
            {"content": '#import "@project/gruss:0.1.0": gruss\n#gruss\n'})
ok, _ = compiles(alice, did, "compile with @project import")
expect(ok, "@project package import compiles")
if alice.me["is_admin"]:
    # Instance packages persist across projects; a unique version keeps reruns working.
    override = f"0.0.{int(time.time())}"
    alice.check("publish instance package", "POST", "/api/packages/publish",
                {"document_id": pkg_did, "scope": "instance", "version": override})
    # Typst rejects a package whose manifest names another version than the import.
    shipped = alice.check("instance package for the browser", "GET",
                          f"/api/documents/{did}/packages/trykst/gruss/{override}") or []
    manifest = next((base64.b64decode(f["data"]).decode() for f in shipped if f["path"] == "typst.toml"), "")
    expect(f'version = "{override}"' in manifest, "manifest carries the published version")
alice.check("list instance packages", "GET", "/api/packages")
# The compiler in the browser fetches package files and the built-in fonts.
bundle = alice.check("package files for the browser", "GET", f"/api/documents/{did}/packages/project/gruss/0.1.0") or []
expect({"typst.toml", "lib.typ"} <= {f["path"] for f in bundle}, "package bundle has manifest and entrypoint")
alice.check("unknown package version", "GET", f"/api/documents/{did}/packages/project/gruss/9.9.9", expect=(404,))
bob.check("package files need document access", "GET", f"/api/documents/{did}/packages/project/gruss/0.1.0", expect=(404,))
fonts = alice.check("built-in fonts", "GET", "/api/fonts/default") or []
expect(len(fonts) > 0, "server lists its built-in fonts")

# --- Permissions --------------------------------------------------------------------------
bob.check("non-member cannot see project", "GET", f"/api/projects/{pid}", expect=(404,))
bob.check("non-member cannot see document", "GET", f"/api/documents/{did}", expect=(404,))
bob.check("non-member cannot compile document", "POST", "/api/compile", {"document_id": did}, expect=(404,))
alice.check("enable link sharing", "PATCH", f"/api/documents/{did}", {"public_role": "viewer"})
shared = bob.check("link viewer opens document", "GET", f"/api/documents/{did}") or {}
expect(shared.get("role") == "viewer", "link sharing grants viewer role")
bob.check("link viewer cannot write", "POST", f"/api/documents/{did}/nodes",
          {"name": "x.typ", "kind": "text"}, expect=(403,))
bob.check("link viewer cannot see project", "GET", f"/api/projects/{pid}", expect=(404,))
alice.check("disable link sharing", "PATCH", f"/api/documents/{did}", {"public_role": None})
bob.check("document hidden again", "GET", f"/api/documents/{did}", expect=(404,))

alice.check("add viewer", "POST", f"/api/projects/{pid}/members", {"email": bob.me["email"], "role": "viewer"})
alice.check("adding twice conflicts", "POST", f"/api/projects/{pid}/members",
            {"email": bob.me["email"], "role": "viewer"}, expect=(409,))
alice.check("add guest as editor", "POST", f"/api/projects/{pid}/members", {"email": erik.me["email"], "role": "editor"})
members = alice.check("list members", "GET", f"/api/projects/{pid}/members") or []
expect(len(members) == 3, "project has three members")
projects = bob.check("viewer lists projects", "GET", "/api/projects") or []
expect(any(p["id"] == pid for p in projects), "project appears for the new member")
bob.check("viewer reads document", "GET", f"/api/documents/{did}/tree")
bob.check("viewer cannot set the thumbnail", "PUT", f"/api/documents/{did}/thumbnail", raw=b"<svg></svg>", expect=(403,))
bob.check("viewer cannot write", "PATCH", f"/api/documents/{did}/nodes/{main.get('id')}",
          {"content": "x"}, expect=(403,))
bob.check("viewer cannot create documents", "POST", f"/api/projects/{pid}/documents",
          {"title": "No"}, expect=(403,))
bob.check("viewer cannot add members", "POST", f"/api/projects/{pid}/members",
          {"email": erik.me["email"], "role": "owner"}, expect=(403,))
erik.check("guest cannot create projects", "POST", "/api/projects", {"name": "Guest"}, expect=(403,))
guest_doc = erik.check("guest editor creates document", "POST", f"/api/projects/{pid}/documents",
                       {"title": "Vom Gast"}) or {}
erik.check("guest edits in project", "POST", f"/api/documents/{did}/nodes", {"name": "gast.typ", "kind": "text"})
erik.check("editor cannot delete project", "DELETE", f"/api/projects/{pid}", expect=(403,))
alice.check("last owner cannot step down", "PATCH", f"/api/projects/{pid}/members/{alice.me['id']}",
            {"role": "editor"}, expect=(409,))
alice.check("promote second owner", "PATCH", f"/api/projects/{pid}/members/{bob.me['id']}", {"role": "owner"})
bob.check("new owner manages members", "PATCH", f"/api/projects/{pid}/members/{erik.me['id']}", {"role": "viewer"})
erik.check("member leaves project", "DELETE", f"/api/projects/{pid}/members/{erik.me['id']}")
erik.check("former member loses access", "GET", f"/api/documents/{guest_doc.get('id')}", expect=(404,))

# --- Account endpoints and cleanup ----------------------------------------------------------
alice.check("create API key", "POST", "/api/keys", {"name": "e2e"})
alice.check("list API keys", "GET", "/api/keys")
alice.check("API key usage", "GET", "/api/keys/usage?period=1week")
alice.check("storage stats", "GET", "/api/auth/storage")
alice.check("admin user list", "GET", "/api/admin/users")
alice.check("delete comment", "DELETE", f"/api/comments/{comment.get('id')}")
alice.check("delete project", "DELETE", f"/api/projects/{pid}")
alice.check("documents go with the project", "GET", f"/api/documents/{did}", expect=(404,))
bob.check("project gone for other members", "GET", f"/api/projects/{pid}", expect=(404,))
alice.check("logout", "POST", "/api/auth/logout")
alice.check("session gone after logout", "GET", "/api/auth/me", expect=(401,))

if failures:
    sys.exit(f"\n{len(failures)} check(s) failed: {', '.join(failures)}")
print("\nAll checks passed.")
