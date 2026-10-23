"""Package an already-built Windows binary without including runtime user data."""
from pathlib import Path
import hashlib
import shutil
import subprocess
import json
import zipfile

root=Path(__file__).resolve().parents[1]
exe=root/"target/release/constellation.exe"
if not exe.is_file(): raise SystemExit("Build first: cargo build --release --locked")
folder=root/"portable/Constellation"
folder.mkdir(parents=True,exist_ok=True)
shutil.copy2(exe,folder/"constellation.exe")
shutil.copy2(root/"docs/USER-GUIDE.md",folder/"README.md")
metadata=json.loads(subprocess.check_output(["cargo","metadata","--format-version","1","--locked","--offline"],cwd=root,text=True))
notices=["THIRD-PARTY NOTICES\n\nDependencies included in the build are listed below with their available license texts.\n"]
for package in sorted(metadata["packages"],key=lambda p:p["name"]):
    if package["name"]=="constellation":continue
    directory=Path(package["manifest_path"]).parent
    notices.append(f"\n{'='*60}\n{package['name']} {package['version']}\nLicense: {package.get('license') or 'see package license file'}\n")
    candidates=sorted({*directory.glob("LICENSE*"),*directory.glob("COPYING*")})
    if package.get("license_file"):candidates.append(Path(package["license_file"]))
    for path in candidates:
        if path.is_file():notices.append(f"\n--- {path.name} ---\n{path.read_text(encoding='utf-8',errors='replace')}\n")
(folder/"THIRD-PARTY-NOTICES.txt").write_text("".join(notices),encoding="utf-8")
dist=root/"dist";dist.mkdir(exist_ok=True)
archive=dist/"Constellation-0.1.0-windows-x64.zip"
with zipfile.ZipFile(archive,"w",zipfile.ZIP_DEFLATED) as z:
    # Explicit list excludes data/config.json, even when packaging over an older folder.
    for name in ["constellation.exe","README.md","THIRD-PARTY-NOTICES.txt"]:
        z.write(folder/name,f"Constellation/{name}")
digest=hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix(".zip.sha256").write_text(f"{digest}  {archive.name}\n",encoding="ascii")
print(archive)
print("SHA-256:",digest)
