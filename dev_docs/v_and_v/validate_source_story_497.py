"""採用 XSD の版・原本 hash を固定し、原階 fixture と実出力を全文検証する。"""

import hashlib
import sys
from pathlib import Path

from lxml import etree

raw = Path(sys.argv[1]).read_bytes()
assert hashlib.sha256(raw).hexdigest() == "56cc1b80062c2385c15f8ab0745f4e96ac5b9400a4743307031e813d0253a1fa"
assert raw[:3] == b"o;?"
normalized = raw[3:]
assert hashlib.sha256(normalized).hexdigest() == "d854bf431d395ace4a7917a63ebed431d251e5bb2fbf7fbea26e9e195fdfa2b6"
schema = etree.XMLSchema(etree.fromstring(normalized))
fixture = Path(sys.argv[2])
assert hashlib.sha256(fixture.read_bytes()).hexdigest() == "94aff5d2e84e2a1fd60a2e0fdc7d332468cb2a279f992b0748f081e9b500969d"
documents = [etree.parse(str(fixture)), etree.parse(sys.argv[3])]
for document in documents:
    schema.assertValid(document)

ns = {"s": "https://www.building-smart.or.jp/dl"}


def stories(document):
    return sorted(
        [(dict(s.attrib), [n.get("id") for n in s.findall("s:StbNodeIdList/s:StbNodeId", ns)])
        for s in document.findall("s:StbModel/s:StbStories/s:StbStory", ns)],
        key=lambda item: int(item[0]["id"]),
    )


def nodes(document):
    return sorted(
        (n.get("id"), n.get("guid"), tuple(float(n.get(axis)) for axis in ("X", "Y", "Z")))
        for n in document.findall("s:StbModel/s:StbNodes/s:StbNode", ns)
    )


assert stories(documents[0]) == stories(documents[1])
assert nodes(documents[0]) == nodes(documents[1])
print(f"STB 2.0.2 入力・実出力の全文 schema と原階 {len(stories(documents[0]))} 件・節点 {len(nodes(documents[0]))} 件が一致")
print("出力 SHA256", hashlib.sha256(Path(sys.argv[3]).read_bytes()).hexdigest())
