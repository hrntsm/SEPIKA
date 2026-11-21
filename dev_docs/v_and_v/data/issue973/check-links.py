from pathlib import Path
import re


root = Path(__file__).resolve().parents[4]
files = [
    root / "CONTEXT.md",
    root / "dev_docs/adr/0087-gui-owned-study-and-local-ai.md",
    root / "dev_docs/v_and_v/UI検討_Issue973_接続確認.md",
    root / "dev_docs/adr/README.md",
    root / "dev_docs/v_and_v/README.md",
    root / "dev_docs/v_and_v/未検証一覧.md",
]
errors = []
count = 0
for file in files:
    content = file.read_text(encoding="utf-8-sig")
    if file.name in ("README.md", "未検証一覧.md"):
        content = "\n".join(
            line for line in content.splitlines() if "973" in line or "0087" in line
        )
    for target in re.findall(r"\]\(([^)]+)\)", content):
        if re.match(r"https?://", target):
            continue
        path, _, anchor = target.partition("#")
        destination = (file.parent / path).resolve() if path else file
        count += 1
        if not destination.exists():
            errors.append(f"{file.relative_to(root)}: missing {target}")
            continue
        if anchor and destination.suffix == ".md":
            markdown = destination.read_text(encoding="utf-8-sig")
            headings = re.findall(r"^#+\s+(.+)$", markdown, re.M)
            explicit_ids = re.findall(r'<[^>]+\bid="([^"]+)"', markdown)
            slugs = [
                re.sub(r"[^\w\-\s]", "", heading.lower()).replace(" ", "-")
                for heading in headings
            ]
            if anchor not in slugs and anchor not in explicit_ids:
                errors.append(f"{file.relative_to(root)}: missing anchor {target}")
print(f"Local links and anchors: {count} checked; {len(errors)} errors")
for error in errors:
    print(error)
raise SystemExit(bool(errors))
