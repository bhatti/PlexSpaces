#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Tag a PlexSpaces release.
#
# Usage:
#   ./scripts/tag-release.sh [--patch | --minor | --major | vX.Y.Z]
#
# Bump modes:
#   --patch   (default) 0.1.4 → 0.1.5
#   --minor             0.1.4 → 0.2.0
#   --major             0.1.4 → 1.0.0
#   vX.Y.Z   explicit version, must be > current
#
# What it does:
#   1. Reads current version from [workspace.package] in Cargo.toml
#   2. Computes the next version (bump or explicit)
#   3. Updates Cargo.toml, pyproject.toml, package.json in-place
#   4. Updates compatible-release pins in all example dependency files
#   5. Prints the git commands for the user to run (Rule #5: never runs git)
#
# Example dependency files updated:
#   examples/python/apps/*/requirements.txt     → plexspaces~=MAJOR.MINOR
#   examples/typescript/apps/*/package.json     → @plexspaces/sdk:^MAJOR.MINOR
#
# After running this script:
#   git add -A
#   git commit -m "chore: bump version to vX.Y.Z"
#   git tag -a vX.Y.Z          -m "Release vX.Y.Z"
#   git tag -a sdks/go/vX.Y.Z  -m "Go SDK vX.Y.Z"
#   git push && git push --tags

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT_CARGO="$REPO_ROOT/Cargo.toml"
PYTHON_SDK_TOML="$REPO_ROOT/sdks/python/pyproject.toml"
TS_SDK_PKG="$REPO_ROOT/sdks/typescript/package.json"

# ---------------------------------------------------------------------------
# 1. Read current version from Cargo.toml [workspace.package]
# ---------------------------------------------------------------------------
CURRENT=$(python3 - "$ROOT_CARGO" <<'PY'
import sys, re
with open(sys.argv[1]) as f:
    content = f.read()
in_section = False
for line in content.splitlines():
    if re.match(r'^\[workspace\.package\]', line):
        in_section = True; continue
    if in_section and re.match(r'^\[', line):
        in_section = False
    if in_section and re.match(r'^version\s*=\s*"', line):
        m = re.search(r'"([^"]+)"', line)
        if m: print(m.group(1)); sys.exit(0)
print("0.0.0")
PY
)

IFS='.' read -r CUR_MAJOR CUR_MINOR CUR_PATCH <<< "$CURRENT"

# ---------------------------------------------------------------------------
# 2. Compute next version
# ---------------------------------------------------------------------------
ARG="${1:---patch}"

case "$ARG" in
    --patch)
        NEW_MAJOR=$CUR_MAJOR; NEW_MINOR=$CUR_MINOR; NEW_PATCH=$((CUR_PATCH + 1))
        ;;
    --minor)
        NEW_MAJOR=$CUR_MAJOR; NEW_MINOR=$((CUR_MINOR + 1)); NEW_PATCH=0
        ;;
    --major)
        NEW_MAJOR=$((CUR_MAJOR + 1)); NEW_MINOR=0; NEW_PATCH=0
        ;;
    v[0-9]*.[0-9]*.[0-9]*)
        SEMVER_ARG="${ARG#v}"
        IFS='.' read -r NEW_MAJOR NEW_MINOR NEW_PATCH <<< "$SEMVER_ARG"
        # Validate explicit version is strictly greater
        if [[ "$NEW_MAJOR" -lt "$CUR_MAJOR" ]] || \
           { [[ "$NEW_MAJOR" -eq "$CUR_MAJOR" ]] && [[ "$NEW_MINOR" -lt "$CUR_MINOR" ]]; } || \
           { [[ "$NEW_MAJOR" -eq "$CUR_MAJOR" ]] && [[ "$NEW_MINOR" -eq "$CUR_MINOR" ]] && [[ "$NEW_PATCH" -le "$CUR_PATCH" ]]; }; then
            echo -e "${RED}ERROR: explicit version v${SEMVER_ARG} must be > current v${CURRENT}${NC}"
            exit 1
        fi
        ;;
    *)
        echo -e "${RED}ERROR: unrecognised argument: $ARG${NC}"
        echo "Usage: $0 [--patch | --minor | --major | vX.Y.Z]"
        exit 1
        ;;
esac

NEW_SEMVER="${NEW_MAJOR}.${NEW_MINOR}.${NEW_PATCH}"
NEW_TAG="v${NEW_SEMVER}"
# Compatible-release prefix used in examples: ~=MAJOR.MINOR (Python) / ^MAJOR.MINOR (npm)
COMPAT_PREFIX="${NEW_MAJOR}.${NEW_MINOR}"

echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  PlexSpaces Release Tagging                                      ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  Current version : ${BLUE}${CURRENT}${NC}"
echo -e "  New version     : ${GREEN}${NEW_SEMVER}${NC}  (tag: ${NEW_TAG})"
echo -e "  Bump type       : ${ARG}"
echo ""

# ---------------------------------------------------------------------------
# 3. Update [workspace.package] version in Cargo.toml
# ---------------------------------------------------------------------------
echo -e "${GREEN}[1/5] Updating Cargo.toml [workspace.package] version...${NC}"
python3 - "$ROOT_CARGO" "$NEW_SEMVER" <<'PY'
import sys, re
cargo_file, new_version = sys.argv[1], sys.argv[2]
with open(cargo_file) as f:
    lines = f.readlines()
in_section = replaced = False
result = []
for line in lines:
    if re.match(r'^\[workspace\.package\]', line):
        in_section = True
    elif in_section and re.match(r'^\[', line):
        in_section = False
    if in_section and not replaced and re.match(r'^version\s*=\s*"', line):
        line = f'version = "{new_version}"\n'
        replaced = True
    result.append(line)
if not replaced:
    print("ERROR: [workspace.package] version not found", file=sys.stderr); sys.exit(1)
with open(cargo_file, 'w') as f:
    f.writelines(result)
print(f"  Cargo.toml → {new_version}")
PY
echo -e "${GREEN}  ✅ done${NC}"

# ---------------------------------------------------------------------------
# 4. Update Python SDK pyproject.toml
# ---------------------------------------------------------------------------
echo -e "${GREEN}[2/5] Updating sdks/python/pyproject.toml version...${NC}"
python3 - "$PYTHON_SDK_TOML" "$NEW_SEMVER" <<'PY'
import sys, re
toml_file, new_version = sys.argv[1], sys.argv[2]
with open(toml_file) as f:
    lines = f.readlines()
in_project = replaced = False
result = []
for line in lines:
    if re.match(r'^\[project\]', line):
        in_project = True
    elif in_project and re.match(r'^\[', line):
        in_project = False
    if in_project and not replaced and re.match(r'^version\s*=\s*"', line):
        line = f'version = "{new_version}"\n'
        replaced = True
    result.append(line)
if not replaced:
    print("ERROR: [project] version not found", file=sys.stderr); sys.exit(1)
with open(toml_file, 'w') as f:
    f.writelines(result)
print(f"  pyproject.toml → {new_version}")
PY
echo -e "${GREEN}  ✅ done${NC}"

# ---------------------------------------------------------------------------
# 5. Update TypeScript SDK package.json
# ---------------------------------------------------------------------------
echo -e "${GREEN}[3/5] Updating sdks/typescript/package.json version...${NC}"
python3 - "$TS_SDK_PKG" "$NEW_SEMVER" <<'PY'
import sys, json
pkg_file, new_version = sys.argv[1], sys.argv[2]
with open(pkg_file) as f:
    data = json.load(f)
data['version'] = new_version
with open(pkg_file, 'w') as f:
    json.dump(data, f, indent=2)
    f.write('\n')
print(f"  package.json → {new_version}")
PY
echo -e "${GREEN}  ✅ done${NC}"

# ---------------------------------------------------------------------------
# 6. Update example dependency files to compatible-release pins
#    Python: plexspaces~=MAJOR.MINOR  (accepts any MAJOR.MINOR.x)
#    npm:    @plexspaces/sdk:^MAJOR.MINOR  (accepts any MAJOR.MINOR.x)
# ---------------------------------------------------------------------------
echo -e "${GREEN}[4/5] Updating example dependency pins to ~=${COMPAT_PREFIX} / ^${COMPAT_PREFIX}...${NC}"

# Python requirements.txt files
UPDATED_PY=0
while IFS= read -r -d '' req_file; do
    if grep -q 'plexspaces' "$req_file" 2>/dev/null; then
        sed -i.bak "s|plexspaces[^=]*==[^ ]*|plexspaces~=${COMPAT_PREFIX}|g" "$req_file"
        rm -f "${req_file}.bak"
        echo "  updated: $req_file"
        UPDATED_PY=$((UPDATED_PY + 1))
    fi
done < <(find "$REPO_ROOT/examples" -name "requirements.txt" -print0)
echo "  Python: ${UPDATED_PY} file(s) updated"

# TypeScript package.json files (examples only, not sdks/)
UPDATED_TS=0
while IFS= read -r -d '' pkg_file; do
    if grep -q '@plexspaces/sdk' "$pkg_file" 2>/dev/null; then
        python3 - "$pkg_file" "$COMPAT_PREFIX" <<'PY'
import sys, json
pkg_file, compat = sys.argv[1], sys.argv[2]
with open(pkg_file) as f:
    data = json.load(f)
changed = False
for section in ('dependencies', 'devDependencies', 'peerDependencies'):
    if section in data and '@plexspaces/sdk' in data[section]:
        data[section]['@plexspaces/sdk'] = f'^{compat}'
        changed = True
if changed:
    with open(pkg_file, 'w') as f:
        json.dump(data, f, indent=2)
        f.write('\n')
    print(f"  updated: {pkg_file}")
PY
        UPDATED_TS=$((UPDATED_TS + 1))
    fi
done < <(find "$REPO_ROOT/examples" -name "package.json" -not -path "*/node_modules/*" -print0)
echo "  TypeScript: ${UPDATED_TS} file(s) updated"

echo -e "${GREEN}  ✅ done${NC}"

# ---------------------------------------------------------------------------
# 7. Summary and git instructions
# ---------------------------------------------------------------------------
echo ""
echo -e "${GREEN}[5/5] Files modified — run these git commands to complete the release:${NC}"
echo ""
echo -e "${YELLOW}  git add Cargo.toml sdks/python/pyproject.toml sdks/typescript/package.json${NC}"
echo -e "${YELLOW}  git add \$(find examples -name 'requirements.txt' -o -name 'package.json' | grep -v node_modules)${NC}"
echo -e "${YELLOW}  git commit -m \"chore: bump version to ${NEW_TAG}\"${NC}"
echo -e "${YELLOW}  git tag -a ${NEW_TAG}          -m \"Release ${NEW_TAG}\"${NC}"
echo -e "${YELLOW}  git tag -a sdks/go/${NEW_TAG}  -m \"Go SDK ${NEW_TAG}\"${NC}"
echo -e "${YELLOW}  git push && git push --tags${NC}"
echo ""
echo "After pushing tags, publish Python + npm SDKs:"
echo -e "${YELLOW}  ./scripts/publish-sdks.sh${NC}"
echo ""
echo "How consumers import each SDK after tagging:"
echo "  Rust     :  plexspaces-sdk = { git = \"https://github.com/plexobject/plexspaces\", tag = \"${NEW_TAG}\" }"
echo "  Go       :  require github.com/plexobject/plexspaces/sdks/go ${NEW_TAG}"
echo "  Python   :  pip install 'plexspaces~=${COMPAT_PREFIX}'"
echo "  npm      :  npm install @plexspaces/sdk@^${COMPAT_PREFIX}"
echo ""
echo -e "${GREEN}✅ tag-release.sh complete — v${CURRENT} → v${NEW_SEMVER}${NC}"
