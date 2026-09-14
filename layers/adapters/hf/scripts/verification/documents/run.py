"""Check repository-authored Markdown encoding and local links."""

from pathlib import Path
import re


def main() -> int:
    root = Path(__file__).resolve().parents[3]
    paths = sorted(root.rglob("*.md"))
    errors = []
    links = 0
    for path in paths:
        raw = path.read_bytes()
        text = raw.decode("utf-8")
        if b"\r" in raw or raw.startswith(b"\xef\xbb\xbf") or not raw.endswith(b"\n"):
            errors.append(f"{path.relative_to(root)}: expected UTF-8/LF with final newline")
        if text.count("```") % 2:
            errors.append(f"{path.relative_to(root)}: unbalanced code fences")
        for destination in re.findall(r"(?<!!)\[[^\]]+\]\(([^)]+)\)", text):
            if destination.startswith(("https://", "http://")):
                continue
            target = (path.parent / destination.split("#", 1)[0]).resolve()
            links += 1
            if not target.exists():
                errors.append(f"{path.relative_to(root)}: broken link {destination}")
    readme = (root / "README.md").read_text(encoding="utf-8")
    for name in ("overview", "architecture", "api", "usage", "constraints", "internals", "testing"):
        if f"docs/{name}.md" not in readme:
            errors.append(f"README: missing {name} index")
    for error in errors:
        print(error)
    print(f"documents={len(paths)} local_links={links} errors={len(errors)}; external web pages not checked")
    return bool(errors)


if __name__ == "__main__":
    raise SystemExit(main())
