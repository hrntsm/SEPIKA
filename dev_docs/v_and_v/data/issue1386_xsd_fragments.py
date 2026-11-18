"""Validate the Issue #1386 shape fragments against the supplied official ZIP."""

import argparse
from hashlib import sha256
from pathlib import Path
from zipfile import ZipFile

from lxml import etree


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("official_zip", type=Path)
    args = parser.parse_args()
    with ZipFile(args.official_zip) as archive:
        original = archive.read("ST-Bridge_v202.xsd")

    prefix = bytes.fromhex("6f3b3f")
    assert original.startswith(prefix)
    try:
        etree.fromstring(original)
    except etree.XMLSyntaxError as error:
        print(f"Original XSD: {error}")
    else:
        raise AssertionError("Unexpectedly parsed the original XSD")

    validation_copy = original[len(prefix):]
    print("ZIP SHA-256:", sha256(args.official_zip.read_bytes()).hexdigest())
    print("original XSD SHA-256:", sha256(original).hexdigest())
    print("validation XSD SHA-256:", sha256(validation_copy).hexdigest())
    print("lxml / libxml2:", etree.LXML_VERSION, etree.LIBXML_VERSION)
    root = etree.fromstring(validation_copy)
    schema = etree.XMLSchema(root)
    namespace = root.get("targetNamespace")

    def validate(profile, attributes):
        element = etree.Element(f"{{{namespace}}}StbSecRoll-{profile}", **attributes)
        return schema.validate(element)

    common = dict(name="example", A="200", B="90", t1="8", t2="14")
    elements = [
        ("L", dict(type="SINGLE", r1="14", r2="7"), ("r1", "r2")),
        ("C", dict(type="SINGLE", r1="14", r2="7"), ("r1", "r2")),
        ("T", dict(type="T", r="8"), ("r",)),
        ("T", dict(type="ST", r="8"), ("r",)),
    ]
    print("profile type attribute: positive, zero, negative, NaN, INF, missing")
    for profile, fields, radii in elements:
        for radius in radii:
            base = {**common, **fields}
            missing = {key: value for key, value in base.items() if key != radius}
            observed = [
                validate(profile, {**base, radius: value})
                for value in (fields[radius], "0", "-1", "NaN", "INF")
            ]
            observed.append(validate(profile, missing))
            print(profile, fields["type"], radius, observed)
            assert observed[:3] == [True, False, False] and not observed[-1]

    l_example = dict(common, type="FACETOFACE", r1="14", r2="7")
    impossible_l = dict(
        l_example, type="SINGLE", A="100", B="75", t1="7", t2="7", r1="1000", r2="5"
    )
    t_with_extra_radii = dict(common, type="T", r="8", r1="8", r2="4")
    for description, profile, attrs, expected in (
        ("L FACETOFACE", "L", l_example, True),
        ("L with non-fitting radius", "L", impossible_l, True),
        ("T with r1 and r2", "T", t_with_extra_radii, False),
    ):
        observed = validate(profile, attrs)
        print(f"{description}: {observed}")
        assert observed == expected


if __name__ == "__main__":
    main()
