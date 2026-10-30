"""Check source/docs/package consistency, optionally against published asset metadata."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import tomllib
from urllib.parse import unquote
import zipfile

ROOT = Path(__file__).resolve().parents[1]
FILES = {"constellation.exe", "LICENSE", "README.md", "PORTABLE-MANIFESTO.md", "THIRD-PARTY-NOTICES.txt"}


def check(archive=None, published_json=None):
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    assert next(p for p in lock["package"] if p["name"] == "constellation")["version"] == version, "Cargo.lock version differs"
    assert (ROOT / f"docs/releases/{version}.md").is_file(), "Missing current release notes"
    for source in [ROOT / "README.md", ROOT / "CONTRIBUTING.md", *sorted((ROOT / "docs").rglob("*.md"))]:
        for link in re.findall(r"\]\(([^)\s]+)\)", source.read_text(encoding="utf-8")):
            if ":" in link or link.startswith(("#", "/")):
                continue
            target = unquote(link.split("#", 1)[0])
            assert (source.parent / target).exists(), f"Broken relative link in {source.name}: {link}"
    if not archive:
        assert not published_json, "--published-json requires the qualified local --archive"
        return version
    archive = Path(archive)
    assert archive.name == f"Constellation-{version}-windows-x64.zip", "Archive name/version mismatch"
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    checksum = archive.with_suffix(".zip.sha256").read_text(encoding="ascii").split()
    assert checksum == [digest, archive.name], "Checksum file mismatch"
    with zipfile.ZipFile(archive) as z:
        assert z.testzip() is None, "Corrupt archive"
        assert set(z.namelist()) == {f"Constellation/{name}" for name in FILES}, "Unexpected package contents"
        assert len(z.namelist()) == len(FILES), "Duplicate archive members"
        assert hashlib.sha256(z.read("Constellation/constellation.exe")).digest() == hashlib.sha256((ROOT / "target/release/constellation.exe").read_bytes()).digest(), "Archive executable differs from qualified build"
        for name, source in [("LICENSE", "LICENSE"), ("README.md", "docs/USER-GUIDE.md"), ("PORTABLE-MANIFESTO.md", "docs/PORTABLE-MANIFESTO.md")]:
            assert z.read(f"Constellation/{name}") == (ROOT / source).read_bytes(), f"Stale packaged {name}"
    if published_json:
        metadata = json.loads(Path(published_json).read_text(encoding="utf-8"))
        assert metadata["tag_name"] == f"v{version}", "Published tag differs"
        assert not metadata["draft"], "Release is still a draft"
        asset = next(a for a in metadata["assets"] if a["name"] == archive.name)
        assert asset.get("digest") == f"sha256:{digest}", "Published asset digest missing or differs from qualified archive"
        assert asset["size"] == archive.stat().st_size, "Published asset size differs"
    return version


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--published-json", type=Path, help="Saved GitHub release API response; no credentials are read by this checker")
    args = parser.parse_args()
    print(f"Release consistency OK: {check(args.archive, args.published_json)}")
