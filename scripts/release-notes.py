#!/usr/bin/env python3
"""Print a reviewed version section; never publish boilerplate or Unreleased."""
import pathlib,re,sys
if len(sys.argv)!=2 or not re.fullmatch(r"v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?",sys.argv[1]):
    raise SystemExit("Usage: release-notes.py vMAJOR.MINOR.PATCH")
tag="v"+sys.argv[1].removeprefix("v")
text=(pathlib.Path(__file__).resolve().parents[1]/"CHANGELOG.md").read_text(encoding="utf-8")
match=re.search(r"^## \["+re.escape(tag)+r"\]\s*\n(.*?)(?=^## \[|\Z)",text,re.M|re.S)
if not match or not match.group(1).strip():
    raise SystemExit("Add reviewed, batched release notes to CHANGELOG.md before tagging "+tag)
body=re.sub(r"^### ","## ",match.group(1).strip(),flags=re.M)
if not re.search(r"^- \S",body,re.M):
    raise SystemExit("Release notes must contain actual bullet-point changes")
print(body)
