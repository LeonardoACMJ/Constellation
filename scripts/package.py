"""Package an already-built Windows binary without including runtime user data."""
from pathlib import Path
import hashlib
import shutil
import subprocess
import json
import struct
import zipfile
import argparse

root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--staging-dir",type=Path,default=root/"portable/Constellation",
                    help="Build a separate package folder while the current app is running")
args=parser.parse_args()
exe=root/"target/release/constellation.exe"
if not exe.is_file(): raise SystemExit("Build first: cargo build --release --locked")
binary=exe.read_bytes()
# Inspect PE imports without requiring dumpbin or another installed tool.
pe=struct.unpack_from("<I",binary,0x3C)[0]
if binary[pe:pe+4]!=b"PE\0\0" or struct.unpack_from("<H",binary,pe+4)[0]!=0x8664:
    raise SystemExit("Expected a Windows x64 executable")
sections=struct.unpack_from("<H",binary,pe+6)[0]
optional=pe+24
section_table=optional+struct.unpack_from("<H",binary,pe+20)[0]
def offset(rva):
    for i in range(sections):
        size,address,raw_size,raw=struct.unpack_from("<IIII",binary,section_table+i*40+8)
        if address<=rva<address+max(size,raw_size):return raw+rva-address
    raise ValueError(f"Unmapped PE RVA {rva:x}")
imports_rva=struct.unpack_from("<I",binary,optional+112+8)[0]
position=offset(imports_rva)
imports=[]
while any(binary[position:position+20]):
    name=offset(struct.unpack_from("<I",binary,position+12)[0])
    imports.append(binary[name:binary.index(b"\0",name)].decode("ascii"))
    position+=20
if any(name.lower().startswith(("vcruntime","msvcp")) for name in imports):
    raise SystemExit("Portable packaging requires static CRT; rebuild using .cargo/config.toml")
folder=args.staging_dir if args.staging_dir.is_absolute() else root/args.staging_dir
folder.mkdir(parents=True,exist_ok=True)
shutil.copy2(exe,folder/"constellation.exe")
shutil.copy2(root/"LICENSE",folder/"LICENSE")
shutil.copy2(root/"docs/USER-GUIDE.md",folder/"README.md")
shutil.copy2(root/"docs/PERSONALIZATION-STUDY.md",folder/"PERSONALIZATION-STUDY.md")
shutil.copy2(root/"docs/MANUAL-CHECKS.md",folder/"MANUAL-CHECKS.md")
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
version=next(p["version"] for p in metadata["packages"] if p["name"]=="constellation")
archive=dist/f"Constellation-{version}-windows-x64.zip"
with zipfile.ZipFile(archive,"w",zipfile.ZIP_DEFLATED) as z:
    # Explicit list excludes data/config.json, even when packaging over an older folder.
    for name in ["constellation.exe","LICENSE","README.md","PERSONALIZATION-STUDY.md","MANUAL-CHECKS.md","THIRD-PARTY-NOTICES.txt"]:
        z.write(folder/name,f"Constellation/{name}")
digest=hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix(".zip.sha256").write_text(f"{digest}  {archive.name}\n",encoding="ascii")
print(archive)
print("SHA-256:",digest)
print("PE imports:",", ".join(imports))
